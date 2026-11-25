// System-audio capture for meeting recording, via Core Audio process taps
// (macOS 14.2+). Captures everything the default output plays, except this
// process, so Sokki's own feedback sounds are not recorded.
//
// Requires the "System Audio Recording" TCC permission, prompted through
// NSAudioCaptureUsageDescription in Info.plist.
//
// The tap can go dead mid-meeting: on 2026-11-24 it delivered exact zeros
// from 10:02:20 on while Chrome kept playing the call into a Bluetooth
// headset (coreaudiod logged Bluetooth decode errors and IO overloads that
// second). Rust watches for that (`system_tap.rs`, watchdog) and calls
// `sokki_tap_rebuild`, which builds the tap and its aggregate anew.

import CoreAudio
import Foundation

private let unknownObject = AudioObjectID(kAudioObjectUnknown)

/// Largest number of frames handed to Rust in one callback. Core Audio can
/// deliver larger buffers, so they are split into chunks of this size.
private let maxFramesPerCallback = 4096
private let maxChannels = 8

private func setError(_ out: UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>?, _ message: String) {
    out?.pointee = strdup(message)
}

private func checkStatus(_ status: OSStatus, _ what: String) throws {
    if status != noErr {
        throw TapError.coreAudio(what, status)
    }
}

private enum TapError: Error, CustomStringConvertible {
    case unsupportedOS
    case coreAudio(String, OSStatus)
    case noOutputDevice
    case noSelfProcessObject

    var description: String {
        switch self {
        case .unsupportedOS:
            return "System audio capture requires macOS 14.2 or later."
        case let .coreAudio(what, status):
            return "\(what) failed (OSStatus \(status))."
        case .noOutputDevice:
            return "No default output device is available."
        case .noSelfProcessObject:
            return "Could not find Sokki's audio process object."
        }
    }
}

@available(macOS 14.2, *)
private final class SystemTap {
    let callback: @convention(c) (UnsafeMutableRawPointer?, UnsafePointer<Float>?, Int, UInt32, Double) -> Void
    let user: UnsafeMutableRawPointer?

    private var tapID = unknownObject
    private var aggregateID = unknownObject
    private var procID: AudioDeviceIOProcID?
    private var sampleRate: Double = 0
    private var channels: UInt32 = 0
    private var nonInterleaved = false
    private let scratch: UnsafeMutablePointer<Float>

    init(callback: @escaping @convention(c) (UnsafeMutableRawPointer?, UnsafePointer<Float>?, Int, UInt32, Double) -> Void,
         user: UnsafeMutableRawPointer?) {
        self.callback = callback
        self.user = user
        self.scratch = UnsafeMutablePointer<Float>.allocate(capacity: maxFramesPerCallback * maxChannels)
    }

    deinit {
        scratch.deallocate()
    }

    func start() throws {
        let selfProcess = try Self.processObjectForCurrentPID()
        let description = CATapDescription(stereoGlobalTapButExcludeProcesses: [selfProcess])
        description.uuid = UUID()
        description.name = "Sokki system audio"
        description.isPrivate = true
        // muteBehavior defaults to unmuted: the user still hears the audio.

        var newTap = unknownObject
        try checkStatus(AudioHardwareCreateProcessTap(description, &newTap), "AudioHardwareCreateProcessTap")
        tapID = newTap

        let format = try Self.tapFormat(tapID)
        sampleRate = format.mSampleRate
        channels = format.mChannelsPerFrame
        nonInterleaved = (format.mFormatFlags & kAudioFormatFlagIsNonInterleaved) != 0
        guard channels > 0, channels <= UInt32(maxChannels) else {
            throw TapError.coreAudio("Tap channel count \(channels) is not supported", noErr)
        }

        let outputUID = try Self.defaultOutputUID()
        let tapUID = description.uuid.uuidString
        let aggregateDescription: [String: Any] = [
            kAudioAggregateDeviceNameKey: "Sokki System Audio",
            kAudioAggregateDeviceUIDKey: UUID().uuidString,
            kAudioAggregateDeviceMainSubDeviceKey: outputUID,
            kAudioAggregateDeviceIsPrivateKey: true,
            kAudioAggregateDeviceIsStackedKey: false,
            kAudioAggregateDeviceTapAutoStartKey: true,
            kAudioAggregateDeviceSubDeviceListKey: [
                [kAudioSubDeviceUIDKey: outputUID],
            ],
            kAudioAggregateDeviceTapListKey: [
                [
                    kAudioSubTapDriftCompensationKey: true,
                    kAudioSubTapUIDKey: tapUID,
                ],
            ],
        ]
        var newAggregate = unknownObject
        try checkStatus(
            AudioHardwareCreateAggregateDevice(aggregateDescription as CFDictionary, &newAggregate),
            "AudioHardwareCreateAggregateDevice"
        )
        aggregateID = newAggregate

        var newProc: AudioDeviceIOProcID?
        try checkStatus(
            AudioDeviceCreateIOProcIDWithBlock(&newProc, aggregateID, nil) { [weak self] _, inputData, _, _, _ in
                self?.handleInput(inputData)
            },
            "AudioDeviceCreateIOProcIDWithBlock"
        )
        procID = newProc

        try checkStatus(AudioDeviceStart(aggregateID, procID), "AudioDeviceStart")
    }

    /// Tears the tap and aggregate down and builds them anew, for the same
    /// callback. No callback runs in between (`stop` is synchronous).
    func rebuild() throws {
        stop()
        try start()
    }

    func stop() {
        if let procID, aggregateID != unknownObject {
            // Stop is synchronous: no IO callback runs after it returns.
            AudioDeviceStop(aggregateID, procID)
            AudioDeviceDestroyIOProcID(aggregateID, procID)
            self.procID = nil
        }
        if aggregateID != unknownObject {
            AudioHardwareDestroyAggregateDevice(aggregateID)
            aggregateID = unknownObject
        }
        if tapID != unknownObject {
            AudioHardwareDestroyProcessTap(tapID)
            tapID = unknownObject
        }
    }

    /// Runs on Core Audio's IO thread. Converts to interleaved f32 in the
    /// preallocated scratch buffer and forwards it to Rust without allocating.
    private func handleInput(_ inputData: UnsafePointer<AudioBufferList>?) {
        guard let inputData else { return }
        let list = UnsafeMutableAudioBufferListPointer(UnsafeMutablePointer(mutating: inputData))
        let ch = Int(channels)

        if nonInterleaved {
            // One buffer per channel, each holding mono samples.
            guard list.count >= ch else { return }
            let frames = Int(list[0].mDataByteSize) / MemoryLayout<Float>.size
            var start = 0
            while start < frames {
                let count = min(maxFramesPerCallback, frames - start)
                for c in 0 ..< ch {
                    guard let data = list[c].mData?.assumingMemoryBound(to: Float.self) else { continue }
                    for f in 0 ..< count {
                        scratch[f * ch + c] = data[start + f]
                    }
                }
                deliver(count)
                start += count
            }
        } else {
            // A single buffer that is already interleaved.
            guard list.count >= 1, let raw = list[0].mData?.assumingMemoryBound(to: Float.self) else { return }
            let totalFrames = Int(list[0].mDataByteSize) / (MemoryLayout<Float>.size * ch)
            var start = 0
            while start < totalFrames {
                let count = min(maxFramesPerCallback, totalFrames - start)
                let src = raw + start * ch
                scratch.update(from: src, count: count * ch)
                deliver(count)
                start += count
            }
        }
    }

    private func deliver(_ frames: Int) {
        callback(user, UnsafePointer(scratch), frames, channels, sampleRate)
    }

    // MARK: Core Audio queries

    private static func processObjectForCurrentPID() throws -> AudioObjectID {
        var address = AudioObjectPropertyAddress(
            mSelector: kAudioHardwarePropertyTranslatePIDToProcessObject,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMain
        )
        var pid = getpid()
        var object = unknownObject
        var size = UInt32(MemoryLayout<AudioObjectID>.size)
        let status = AudioObjectGetPropertyData(
            AudioObjectID(kAudioObjectSystemObject),
            &address,
            UInt32(MemoryLayout<pid_t>.size),
            &pid,
            &size,
            &object
        )
        try checkStatus(status, "Looking up Sokki's audio process")
        guard object != unknownObject else { throw TapError.noSelfProcessObject }
        return object
    }

    private static func tapFormat(_ tap: AudioObjectID) throws -> AudioStreamBasicDescription {
        var address = AudioObjectPropertyAddress(
            mSelector: kAudioTapPropertyFormat,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMain
        )
        var format = AudioStreamBasicDescription()
        var size = UInt32(MemoryLayout<AudioStreamBasicDescription>.size)
        try checkStatus(AudioObjectGetPropertyData(tap, &address, 0, nil, &size, &format), "Reading tap format")
        return format
    }

    static func defaultOutputDevice() throws -> AudioObjectID {
        var address = AudioObjectPropertyAddress(
            mSelector: kAudioHardwarePropertyDefaultOutputDevice,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMain
        )
        var device = unknownObject
        var size = UInt32(MemoryLayout<AudioObjectID>.size)
        try checkStatus(
            AudioObjectGetPropertyData(AudioObjectID(kAudioObjectSystemObject), &address, 0, nil, &size, &device),
            "Reading default output device"
        )
        guard device != unknownObject else { throw TapError.noOutputDevice }
        return device
    }

    static func stringProperty(_ object: AudioObjectID, _ selector: AudioObjectPropertySelector) -> String? {
        var address = AudioObjectPropertyAddress(
            mSelector: selector,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMain
        )
        var value: Unmanaged<CFString>?
        var size = UInt32(MemoryLayout<Unmanaged<CFString>?>.size)
        guard AudioObjectGetPropertyData(object, &address, 0, nil, &size, &value) == noErr,
              let value else { return nil }
        return value.takeRetainedValue() as String
    }

    static func uint32Property(_ object: AudioObjectID, _ selector: AudioObjectPropertySelector) -> UInt32? {
        var address = AudioObjectPropertyAddress(
            mSelector: selector,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMain
        )
        var value: UInt32 = 0
        var size = UInt32(MemoryLayout<UInt32>.size)
        guard AudioObjectGetPropertyData(object, &address, 0, nil, &size, &value) == noErr else { return nil }
        return value
    }

    /// Bundle ids (or "pid N") of the other processes playing audio now, or
    /// nil when Core Audio can't say.
    static func otherAppsPlaying() -> [String]? {
        var address = AudioObjectPropertyAddress(
            mSelector: kAudioHardwarePropertyProcessObjectList,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMain
        )
        let system = AudioObjectID(kAudioObjectSystemObject)
        var size: UInt32 = 0
        guard AudioObjectGetPropertyDataSize(system, &address, 0, nil, &size) == noErr else { return nil }
        var objects = [AudioObjectID](repeating: unknownObject, count: Int(size) / MemoryLayout<AudioObjectID>.size)
        guard AudioObjectGetPropertyData(system, &address, 0, nil, &size, &objects) == noErr else { return nil }
        let me = getpid()
        var playing: [String] = []
        for object in objects.prefix(Int(size) / MemoryLayout<AudioObjectID>.size) {
            guard uint32Property(object, kAudioProcessPropertyIsRunningOutput) == 1 else { continue }
            var pid: pid_t = -1
            var pidAddress = AudioObjectPropertyAddress(
                mSelector: kAudioProcessPropertyPID,
                mScope: kAudioObjectPropertyScopeGlobal,
                mElement: kAudioObjectPropertyElementMain
            )
            var pidSize = UInt32(MemoryLayout<pid_t>.size)
            _ = AudioObjectGetPropertyData(object, &pidAddress, 0, nil, &pidSize, &pid)
            if pid == me { continue }
            let bundle = stringProperty(object, kAudioProcessPropertyBundleID) ?? ""
            playing.append(bundle.isEmpty ? "pid \(pid)" : bundle)
        }
        return playing
    }

    private static func defaultOutputUID() throws -> String {
        let device = try defaultOutputDevice()

        var uidAddress = AudioObjectPropertyAddress(
            mSelector: kAudioDevicePropertyDeviceUID,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMain
        )
        var uid: CFString = "" as CFString
        var uidSize = UInt32(MemoryLayout<CFString>.size)
        try checkStatus(AudioObjectGetPropertyData(device, &uidAddress, 0, nil, &uidSize, &uid), "Reading output device UID")
        return uid as String
    }
}

// MARK: - C ABI

@_cdecl("sokki_tap_supported")
public func sokki_tap_supported() -> Bool {
    if #available(macOS 14.2, *) {
        return true
    }
    return false
}

@_cdecl("sokki_tap_start")
public func sokki_tap_start(
    _ callback: @escaping @convention(c) (UnsafeMutableRawPointer?, UnsafePointer<Float>?, Int, UInt32, Double) -> Void,
    _ user: UnsafeMutableRawPointer?,
    _ outError: UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>?
) -> UnsafeMutableRawPointer? {
    guard #available(macOS 14.2, *) else {
        setError(outError, TapError.unsupportedOS.description)
        return nil
    }
    let tap = SystemTap(callback: callback, user: user)
    do {
        try tap.start()
    } catch {
        tap.stop()
        setError(outError, String(describing: error))
        return nil
    }
    return Unmanaged.passRetained(tap).toOpaque()
}

@_cdecl("sokki_tap_stop")
public func sokki_tap_stop(_ handle: UnsafeMutableRawPointer?) {
    guard #available(macOS 14.2, *), let handle else { return }
    let tap = Unmanaged<SystemTap>.fromOpaque(handle).takeRetainedValue()
    tap.stop()
}

@_cdecl("sokki_tap_rebuild")
public func sokki_tap_rebuild(
    _ handle: UnsafeMutableRawPointer?,
    _ outError: UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>?
) -> Bool {
    guard #available(macOS 14.2, *), let handle else {
        setError(outError, TapError.unsupportedOS.description)
        return false
    }
    let tap = Unmanaged<SystemTap>.fromOpaque(handle).takeUnretainedValue()
    do {
        try tap.rebuild()
        return true
    } catch {
        // Leaves nothing half built; the next rebuild starts from scratch.
        tap.stop()
        setError(outError, String(describing: error))
        return false
    }
}

/// Copies `text` into `buffer` (NUL-terminated, cut to `capacity`).
private func copyOut(_ text: String, _ buffer: UnsafeMutablePointer<CChar>?, _ capacity: Int) {
    guard let buffer, capacity > 0 else { return }
    let bytes = Array(text.utf8.prefix(capacity - 1))
    for (i, b) in bytes.enumerated() {
        buffer[i] = CChar(bitPattern: b)
    }
    buffer[bytes.count] = 0
}

@_cdecl("sokki_tap_other_apps_playing")
public func sokki_tap_other_apps_playing(_ buffer: UnsafeMutablePointer<CChar>?, _ capacity: Int) -> Int32 {
    guard #available(macOS 14.2, *), let apps = SystemTap.otherAppsPlaying() else { return -1 }
    copyOut(apps.joined(separator: ", "), buffer, capacity)
    return Int32(apps.count)
}

@_cdecl("sokki_tap_default_output")
public func sokki_tap_default_output(_ buffer: UnsafeMutablePointer<CChar>?, _ capacity: Int) -> UInt32 {
    guard #available(macOS 14.2, *), let device = try? SystemTap.defaultOutputDevice() else { return 0 }
    copyOut(SystemTap.stringProperty(device, kAudioObjectPropertyName) ?? "", buffer, capacity)
    return device
}

@_cdecl("sokki_tap_free_error")
public func sokki_tap_free_error(_ error: UnsafeMutablePointer<CChar>?) {
    free(error)
}
