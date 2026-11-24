// Screen recording for "Meeting with screen", and one video of it with the
// meeting's sound. C ABI in video_capture.h; Rust side in src/video/bridge.rs.
//
// Screen: ScreenCaptureKit (macOS 12.3+), Sokki's own windows (the recording
// pill) left out. Written with AVAssetWriter as an HEVC .mov file. Frame times
// are host-clock times; the first one is turned into wall-clock microseconds
// so it lines up with the meeting's audio tracks.
//
// Render: AVMutableComposition of the screen video and the audio tracks,
// exported as one .mp4.

import AVFoundation
import CoreGraphics
import CoreMedia
import Foundation
import ScreenCaptureKit

// MARK: - Helpers

private func cString(_ s: String) -> UnsafeMutablePointer<CChar>? { strdup(s) }

private func json(_ value: Any) -> UnsafeMutablePointer<CChar>? {
    let data = (try? JSONSerialization.data(withJSONObject: value)) ?? Data("{}".utf8)
    return cString(String(decoding: data, as: UTF8.self))
}

private func parse(_ config: UnsafePointer<CChar>?) -> [String: Any] {
    guard let config else { return [:] }
    let data = Data(String(cString: config).utf8)
    return (try? JSONSerialization.jsonObject(with: data)) as? [String: Any] ?? [:]
}

private func setError(_ out: UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>?, _ message: String) {
    out?.pointee = strdup(message)
}

/// Wall-clock microseconds of a host-clock time.
private func wallMicros(ofHostTime time: CMTime) -> UInt64 {
    let hostNow = CMClockGetTime(CMClockGetHostTimeClock())
    let ago = CMTimeGetSeconds(CMTimeSubtract(hostNow, time))
    let wall = Date().timeIntervalSince1970 - max(0, ago)
    return UInt64(wall * 1_000_000)
}

/// Waits for an async call from a C function. The work runs on a detached task.
private func blocking<T>(_ work: @escaping () async -> T) -> T {
    let semaphore = DispatchSemaphore(value: 0)
    var result: T?
    Task.detached {
        result = await work()
        semaphore.signal()
    }
    semaphore.wait()
    return result!
}

// MARK: - Writer

/// One video file written from sample buffers as they arrive.
private final class VideoWriter {
    let url: URL
    private let writer: AVAssetWriter
    private var input: AVAssetWriterInput?
    private let queue = DispatchQueue(label: "org.sokki.video.writer")
    private(set) var frames = 0
    private(set) var firstMicros: UInt64 = 0
    private var started = false
    private var finished = false
    private(set) var error: String?

    init(url: URL) throws {
        self.url = url
        try? FileManager.default.removeItem(at: url)
        writer = try AVAssetWriter(outputURL: url, fileType: .mov)
    }

    func append(_ buffer: CMSampleBuffer) {
        queue.sync {
            guard !finished, let image = CMSampleBufferGetImageBuffer(buffer) else { return }
            if input == nil {
                let width = CVPixelBufferGetWidth(image)
                let height = CVPixelBufferGetHeight(image)
                let settings: [String: Any] = [
                    AVVideoCodecKey: AVVideoCodecType.hevc,
                    AVVideoWidthKey: width,
                    AVVideoHeightKey: height,
                ]
                let input = AVAssetWriterInput(mediaType: .video, outputSettings: settings)
                input.expectsMediaDataInRealTime = true
                guard writer.canAdd(input) else {
                    error = "The video file could not be set up."
                    finished = true
                    return
                }
                writer.add(input)
                self.input = input
            }
            let time = CMSampleBufferGetPresentationTimeStamp(buffer)
            if !started {
                guard writer.startWriting() else {
                    error = writer.error?.localizedDescription ?? "The video file could not be started."
                    finished = true
                    return
                }
                writer.startSession(atSourceTime: time)
                firstMicros = wallMicros(ofHostTime: time)
                started = true
                // Kept next to the file, so a recording cut by a quit still
                // lines up with its sound.
                try? String(firstMicros).write(toFile: url.path + ".first_us", atomically: true, encoding: .utf8)
            }
            if let input, input.isReadyForMoreMediaData, input.append(buffer) {
                frames += 1
            }
        }
    }

    func finish() {
        var wait = false
        queue.sync {
            guard !finished else { return }
            finished = true
            if started {
                input?.markAsFinished()
                wait = true
            }
        }
        guard wait else {
            try? FileManager.default.removeItem(at: url)
            return
        }
        let done = DispatchSemaphore(value: 0)
        writer.finishWriting { done.signal() }
        done.wait()
        if writer.status != .completed {
            error = writer.error?.localizedDescription ?? "The video file could not be finished."
        }
    }

    var report: [String: Any] {
        var r: [String: Any] = ["path": url.path, "first_us": firstMicros, "frames": frames]
        if let error { r["error"] = error }
        return r
    }
}

// MARK: - Screen

@available(macOS 12.3, *)
private final class ScreenRecorder: NSObject, SCStreamOutput, SCStreamDelegate {
    let writer: VideoWriter
    private var stream: SCStream?
    private let queue = DispatchQueue(label: "org.sokki.video.screen")
    private(set) var stopError: String?

    init(path: String) throws {
        writer = try VideoWriter(url: URL(fileURLWithPath: path))
    }

    func start(displayID: CGDirectDisplayID) async throws {
        let content = try await SCShareableContent.excludingDesktopWindows(false, onScreenWindowsOnly: true)
        guard let display = content.displays.first(where: { $0.displayID == displayID }) ?? content.displays.first else {
            throw NSError(domain: "Sokki", code: 1, userInfo: [NSLocalizedDescriptionKey: "No screen to record."])
        }
        // Sokki's own windows (the camera bubble, the bar) stay out of the video.
        let ownBundle = Bundle.main.bundleIdentifier
        let own = content.applications.filter { $0.bundleIdentifier == ownBundle || $0.processID == getpid() }
        let filter = SCContentFilter(display: display, excludingApplications: own, exceptingWindows: [])

        let config = SCStreamConfiguration()
        let scale = CGFloat(CGDisplayPixelsWide(display.displayID)) / CGFloat(max(display.width, 1))
        let factor = max(scale, 1)
        config.width = Int(CGFloat(display.width) * factor) & ~1
        config.height = Int(CGFloat(display.height) * factor) & ~1
        config.minimumFrameInterval = CMTime(value: 1, timescale: 30)
        config.pixelFormat = kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange
        config.showsCursor = true
        config.queueDepth = 6

        let stream = SCStream(filter: filter, configuration: config, delegate: self)
        try stream.addStreamOutput(self, type: .screen, sampleHandlerQueue: queue)
        try await stream.startCapture()
        self.stream = stream
    }

    func stream(_ stream: SCStream, didOutputSampleBuffer buffer: CMSampleBuffer, of type: SCStreamOutputType) {
        guard type == .screen, buffer.isValid else { return }
        // Only complete frames carry an image; idle frames mean "nothing changed".
        guard
            let attachments = CMSampleBufferGetSampleAttachmentsArray(buffer, createIfNecessary: false) as? [[SCStreamFrameInfo: Any]],
            let raw = attachments.first?[.status] as? Int,
            SCFrameStatus(rawValue: raw) == .complete
        else { return }
        writer.append(buffer)
    }

    func stream(_ stream: SCStream, didStopWithError error: Error) {
        stopError = error.localizedDescription
    }

    func stop() async {
        if let stream {
            try? await stream.stopCapture()
        }
        stream = nil
        writer.finish()
    }
}

// MARK: - Recording handle

private final class Recording {
    var screen: AnyObject?
}

// MARK: - Sources and permissions

@_cdecl("sokki_video_sources")
public func sokki_video_sources() -> UnsafeMutablePointer<CChar>? {
    var displays: [[String: Any]] = []
    var count: UInt32 = 0
    CGGetActiveDisplayList(0, nil, &count)
    var ids = [CGDirectDisplayID](repeating: 0, count: Int(count))
    CGGetActiveDisplayList(count, &ids, &count)
    for id in ids.prefix(Int(count)) {
        let bounds = CGDisplayBounds(id)
        displays.append([
            "id": id,
            "width": Int(bounds.width),
            "height": Int(bounds.height),
            "main": CGDisplayIsMain(id) != 0,
        ])
    }
    return json([
        "screen": CGPreflightScreenCaptureAccess() ? "allowed" : "not_allowed",
        "displays": displays,
    ])
}

@_cdecl("sokki_video_request_screen")
public func sokki_video_request_screen() -> Bool {
    if CGPreflightScreenCaptureAccess() { return true }
    return CGRequestScreenCaptureAccess()
}

// MARK: - Start / stop

@_cdecl("sokki_video_start")
public func sokki_video_start(
    _ config: UnsafePointer<CChar>?,
    _ outError: UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>?
) -> UnsafeMutableRawPointer? {
    let cfg = parse(config)
    let recording = Recording()
    guard let screen = cfg["screen"] as? [String: Any], let path = screen["path"] as? String else {
        setError(outError, "Nothing to record.")
        return nil
    }
    guard #available(macOS 12.3, *) else {
        setError(outError, "Screen recording needs macOS 12.3 or later.")
        return nil
    }
    do {
        let recorder = try ScreenRecorder(path: path)
        let display = (screen["display"] as? NSNumber)?.uint32Value ?? CGMainDisplayID()
        let error: Error? = blocking {
            do { try await recorder.start(displayID: display); return nil } catch { return error }
        }
        if let error { throw error }
        recording.screen = recorder
    } catch {
        setError(outError, error.localizedDescription)
        return nil
    }
    return Unmanaged.passRetained(recording).toOpaque()
}

@_cdecl("sokki_video_stop")
public func sokki_video_stop(_ handle: UnsafeMutableRawPointer?) -> UnsafeMutablePointer<CChar>? {
    guard let handle else { return json(["screen": NSNull(), "errors": []]) }
    let recording = Unmanaged<Recording>.fromOpaque(handle).takeRetainedValue()
    var errors: [String] = []
    var screenReport: Any = NSNull()
    if #available(macOS 12.3, *), let screen = recording.screen as? ScreenRecorder {
        blocking { await screen.stop() }
        if let e = screen.stopError { errors.append("Screen: \(e)") }
        if screen.writer.frames > 0 { screenReport = screen.writer.report }
        if let e = screen.writer.error { errors.append("Screen: \(e)") }
    }
    return json(["screen": screenReport, "errors": errors])
}

@_cdecl("sokki_video_free")
public func sokki_video_free(_ s: UnsafeMutablePointer<CChar>?) {
    free(s)
}

// MARK: - Render

private struct Source {
    let url: URL
    let firstMicros: UInt64
}

private func source(_ value: Any?) -> Source? {
    guard let dict = value as? [String: Any], let path = dict["path"] as? String else { return nil }
    let first = (dict["first_us"] as? NSNumber)?.uint64Value ?? 0
    return Source(url: URL(fileURLWithPath: path), firstMicros: first)
}

private func fail(_ code: Int, _ message: String) -> NSError {
    NSError(domain: "Sokki", code: code, userInfo: [NSLocalizedDescriptionKey: message])
}

/// The screen video with the meeting's sound, lined up by first-frame and
/// first-sample times, as one .mp4.
private func render(_ cfg: [String: Any]) async throws {
    guard let out = cfg["out"] as? String else { throw fail(12, "No output path.") }
    guard let screen = source(cfg["screen"]) else { throw fail(13, "Nothing to render.") }
    let audio = (cfg["audio"] as? [Any] ?? []).compactMap(source)

    // Everything starts relative to the earliest track.
    let origin = ([screen] + audio).map(\.firstMicros).filter { $0 > 0 }.min() ?? 0
    func offset(_ s: Source) -> CMTime {
        let us = s.firstMicros > origin ? s.firstMicros - origin : 0
        return CMTime(value: CMTimeValue(us), timescale: 1_000_000)
    }

    let composition = AVMutableComposition()
    let asset = AVURLAsset(url: screen.url)
    guard let track = try await asset.loadTracks(withMediaType: .video).first,
          let target = composition.addMutableTrack(withMediaType: .video, preferredTrackID: kCMPersistentTrackID_Invalid)
    else { throw fail(14, "The recorded video could not be read.") }
    let (range, transform) = try await track.load(.timeRange, .preferredTransform)
    try target.insertTimeRange(range, of: track, at: offset(screen))
    target.preferredTransform = transform
    let end = CMTimeAdd(offset(screen), range.duration)

    for s in audio {
        let a = AVURLAsset(url: s.url)
        guard let t = try? await a.loadTracks(withMediaType: .audio).first,
              let r = try? await t.load(.timeRange),
              let dst = composition.addMutableTrack(withMediaType: .audio, preferredTrackID: kCMPersistentTrackID_Invalid)
        else { continue }
        try dst.insertTimeRange(r, of: t, at: offset(s))
    }

    let outURL = URL(fileURLWithPath: out)
    try? FileManager.default.removeItem(at: outURL)
    guard let export = AVAssetExportSession(asset: composition, presetName: AVAssetExportPresetHighestQuality) else {
        throw fail(15, "The video could not be exported.")
    }
    export.timeRange = CMTimeRange(start: .zero, duration: end)
    export.outputURL = outURL
    export.outputFileType = .mp4
    export.shouldOptimizeForNetworkUse = true
    await withCheckedContinuation { (done: CheckedContinuation<Void, Never>) in
        export.exportAsynchronously { done.resume() }
    }
    if export.status != .completed {
        throw export.error ?? fail(16, "The video could not be exported.")
    }
}

@_cdecl("sokki_video_render")
public func sokki_video_render(
    _ config: UnsafePointer<CChar>?,
    _ outError: UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>?
) -> Bool {
    let cfg = parse(config)
    let error: Error? = blocking {
        do { try await render(cfg); return nil } catch { return error }
    }
    if let error {
        setError(outError, error.localizedDescription)
        return false
    }
    return true
}
