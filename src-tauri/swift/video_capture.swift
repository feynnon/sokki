// Screen and camera recording, and rendering a finished look from them.
// C ABI in video_capture.h; Rust side in src/video/bridge.rs.
//
// Screen: ScreenCaptureKit (macOS 12.3+), Sokki's own windows left out so the
// camera bubble never lands in the screen video. Camera: AVCaptureSession.
// Both are written with AVAssetWriter as HEVC .mov files. Frame times are
// host-clock times; the first one is turned into wall-clock microseconds so it
// lines up with the meeting's audio tracks.
//
// Render: AVMutableComposition of screen, camera and the audio tracks, with a
// Core Image compositor that draws the look (bubble, side by side, vertical).

import AppKit
import AVFoundation
import CoreGraphics
import CoreImage
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

// MARK: - Camera

/// The camera session, shared by the bubble (live preview) and the recording,
/// so the camera is opened once. Runs while either needs it.
private final class SharedCamera: NSObject, AVCaptureVideoDataOutputSampleBufferDelegate {
    static let shared = SharedCamera()

    let session = AVCaptureSession()
    private let queue = DispatchQueue(label: "org.sokki.video.camera")
    private var deviceID: String?
    private var configured = false
    private var writer: VideoWriter?
    var previewing = false

    /// Opens `deviceID` (nil: the default camera), or keeps the open one.
    func open(_ wanted: String?) throws {
        if configured && (wanted == nil || wanted == deviceID) { return }
        let device = wanted.flatMap { AVCaptureDevice(uniqueID: $0) } ?? AVCaptureDevice.default(for: .video)
        guard let device else {
            throw NSError(domain: "Sokki", code: 2, userInfo: [NSLocalizedDescriptionKey: "No camera found."])
        }
        let input = try AVCaptureDeviceInput(device: device)
        session.beginConfiguration()
        defer { session.commitConfiguration() }
        for old in session.inputs { session.removeInput(old) }
        if session.canSetSessionPreset(.hd1920x1080) {
            session.sessionPreset = .hd1920x1080
        }
        guard session.canAddInput(input) else {
            throw NSError(domain: "Sokki", code: 3, userInfo: [NSLocalizedDescriptionKey: "The camera is busy."])
        }
        session.addInput(input)
        if session.outputs.isEmpty {
            let output = AVCaptureVideoDataOutput()
            output.videoSettings = [kCVPixelBufferPixelFormatTypeKey as String: kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange]
            output.alwaysDiscardsLateVideoFrames = true
            output.setSampleBufferDelegate(self, queue: queue)
            guard session.canAddOutput(output) else {
                throw NSError(domain: "Sokki", code: 4, userInfo: [NSLocalizedDescriptionKey: "The camera could not be set up."])
            }
            session.addOutput(output)
        }
        deviceID = device.uniqueID
        configured = true
    }

    func run() {
        if !session.isRunning { session.startRunning() }
    }

    func record(into writer: VideoWriter) {
        queue.sync { self.writer = writer }
        run()
    }

    /// Stops writing; keeps the session for the bubble while it shows.
    func stopRecording() {
        let writer = queue.sync { () -> VideoWriter? in
            let w = self.writer
            self.writer = nil
            return w
        }
        writer?.finish()
        idleCheck()
    }

    func idleCheck() {
        let recording = queue.sync { writer != nil }
        if !recording && !previewing && session.isRunning {
            session.stopRunning()
        }
    }

    func captureOutput(_ output: AVCaptureOutput, didOutput buffer: CMSampleBuffer, from connection: AVCaptureConnection) {
        writer?.append(buffer)
    }
}

private final class CameraRecorder {
    let writer: VideoWriter

    init(path: String, deviceID: String?) throws {
        writer = try VideoWriter(url: URL(fileURLWithPath: path))
        try SharedCamera.shared.open(deviceID)
    }

    func start() { SharedCamera.shared.record(into: writer) }

    func stop() { SharedCamera.shared.stopRecording() }
}

// MARK: - Bubble

/// The live camera bubble: a round, draggable panel at the bottom left that
/// floats over every app. It is Sokki's own window, so the screen recording
/// leaves it out.
private final class Bubble {
    static var panel: NSPanel?

    static func show(deviceID: String?) {
        let camera = SharedCamera.shared
        do { try camera.open(deviceID) } catch {
            NSLog("Sokki bubble: \(error.localizedDescription)")
            return
        }
        camera.previewing = true
        camera.run()
        if panel != nil { return }
        let size: CGFloat = 180
        let screen = NSScreen.main?.visibleFrame ?? NSRect(x: 0, y: 0, width: 1440, height: 900)
        let frame = NSRect(x: screen.minX + 32, y: screen.minY + 96, width: size, height: size)
        let p = NSPanel(contentRect: frame, styleMask: [.borderless, .nonactivatingPanel], backing: .buffered, defer: false)
        p.level = .floating
        p.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary]
        p.isMovableByWindowBackground = true
        p.backgroundColor = .clear
        p.isOpaque = false
        p.hasShadow = true
        let view = NSView(frame: NSRect(x: 0, y: 0, width: size, height: size))
        view.wantsLayer = true
        view.layer?.cornerRadius = size / 2
        view.layer?.masksToBounds = true
        view.layer?.backgroundColor = NSColor(white: 0.1, alpha: 1).cgColor
        let preview = AVCaptureVideoPreviewLayer(session: camera.session)
        preview.videoGravity = .resizeAspectFill
        preview.frame = view.bounds
        view.layer?.addSublayer(preview)
        p.contentView = view
        p.orderFrontRegardless()
        panel = p
    }

    static func hide() {
        panel?.orderOut(nil)
        panel = nil
        SharedCamera.shared.previewing = false
        SharedCamera.shared.idleCheck()
    }
}

@_cdecl("sokki_video_bubble")
public func sokki_video_bubble(_ show: Bool, _ deviceID: UnsafePointer<CChar>?) {
    let id = deviceID.map { String(cString: $0) }
    DispatchQueue.main.async {
        if show { Bubble.show(deviceID: id) } else { Bubble.hide() }
    }
}

// MARK: - Recording handle

private final class Recording {
    var screen: AnyObject?
    var camera: CameraRecorder?
}

// MARK: - Sources and permissions

@_cdecl("sokki_video_sources")
public func sokki_video_sources() -> UnsafeMutablePointer<CChar>? {
    var displays: [[String: Any]] = []
    var count: UInt32 = 0
    CGGetActiveDisplayList(0, nil, &count)
    var ids = [CGDirectDisplayID](repeating: 0, count: Int(count))
    CGGetActiveDisplayList(count, &ids, &count)
    let names = Dictionary(uniqueKeysWithValues: NSScreenNames.all())
    for id in ids.prefix(Int(count)) {
        let bounds = CGDisplayBounds(id)
        displays.append([
            "id": id,
            "name": names[id] ?? (CGDisplayIsBuiltin(id) != 0 ? "Built-in Display" : "Display"),
            "width": Int(bounds.width),
            "height": Int(bounds.height),
            "main": CGDisplayIsMain(id) != 0,
        ])
    }
    var types: [AVCaptureDevice.DeviceType] = [.builtInWideAngleCamera]
    if #available(macOS 14.0, *) {
        types = [.builtInWideAngleCamera, .external, .continuityCamera]
    }
    let cameras = AVCaptureDevice.DiscoverySession(deviceTypes: types, mediaType: .video, position: .unspecified)
        .devices.map { ["id": $0.uniqueID, "name": $0.localizedName] }

    let camera: String
    switch AVCaptureDevice.authorizationStatus(for: .video) {
    case .authorized: camera = "allowed"
    case .denied: camera = "denied"
    case .restricted: camera = "restricted"
    default: camera = "not_asked"
    }
    return json([
        "screen": CGPreflightScreenCaptureAccess() ? "allowed" : "not_allowed",
        "camera": camera,
        "displays": displays,
        "cameras": cameras,
    ])
}

/// Display names from NSScreen, without importing AppKit into the C surface.
private enum NSScreenNames {
    static func all() -> [(CGDirectDisplayID, String)] {
        guard let screenClass = NSClassFromString("NSScreen") as? NSObject.Type,
              let screens = screenClass.value(forKey: "screens") as? [NSObject]
        else { return [] }
        return screens.compactMap { screen in
            guard
                let description = screen.value(forKey: "deviceDescription") as? [String: Any],
                let number = description["NSScreenNumber"] as? NSNumber
            else { return nil }
            let name = (screen.value(forKey: "localizedName") as? String) ?? "Display"
            return (CGDirectDisplayID(number.uint32Value), name)
        }
    }
}

@_cdecl("sokki_video_request_screen")
public func sokki_video_request_screen() -> Bool {
    if CGPreflightScreenCaptureAccess() { return true }
    return CGRequestScreenCaptureAccess()
}

@_cdecl("sokki_video_request_camera")
public func sokki_video_request_camera() -> Bool {
    if AVCaptureDevice.authorizationStatus(for: .video) == .authorized { return true }
    return blocking { await AVCaptureDevice.requestAccess(for: .video) }
}

// MARK: - Start / stop

@_cdecl("sokki_video_start")
public func sokki_video_start(
    _ config: UnsafePointer<CChar>?,
    _ outError: UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>?
) -> UnsafeMutableRawPointer? {
    let cfg = parse(config)
    let recording = Recording()
    do {
        if let camera = cfg["camera"] as? [String: Any], let path = camera["path"] as? String {
            let recorder = try CameraRecorder(path: path, deviceID: camera["device"] as? String)
            recording.camera = recorder
        }
        if let screen = cfg["screen"] as? [String: Any], let path = screen["path"] as? String {
            guard #available(macOS 12.3, *) else {
                throw NSError(domain: "Sokki", code: 5, userInfo: [NSLocalizedDescriptionKey: "Screen recording needs macOS 12.3 or later."])
            }
            let recorder = try ScreenRecorder(path: path)
            let display = (screen["display"] as? NSNumber)?.uint32Value ?? CGMainDisplayID()
            let error: Error? = blocking {
                do { try await recorder.start(displayID: display); return nil } catch { return error }
            }
            if let error { throw error }
            recording.screen = recorder
        }
        recording.camera?.start()
    } catch {
        if #available(macOS 12.3, *), let screen = recording.screen as? ScreenRecorder {
            blocking { await screen.stop() }
        }
        recording.camera?.stop()
        setError(outError, error.localizedDescription)
        return nil
    }
    return Unmanaged.passRetained(recording).toOpaque()
}

@_cdecl("sokki_video_stop")
public func sokki_video_stop(_ handle: UnsafeMutableRawPointer?) -> UnsafeMutablePointer<CChar>? {
    guard let handle else { return json(["screen": NSNull(), "camera": NSNull(), "errors": []]) }
    let recording = Unmanaged<Recording>.fromOpaque(handle).takeRetainedValue()
    var errors: [String] = []
    var screenReport: Any = NSNull()
    if #available(macOS 12.3, *), let screen = recording.screen as? ScreenRecorder {
        blocking { await screen.stop() }
        if let e = screen.stopError { errors.append("Screen: \(e)") }
        if screen.writer.frames > 0 { screenReport = screen.writer.report }
        if let e = screen.writer.error { errors.append("Screen: \(e)") }
    }
    var cameraReport: Any = NSNull()
    if let camera = recording.camera {
        camera.stop()
        if camera.writer.frames > 0 { cameraReport = camera.writer.report }
        if let e = camera.writer.error { errors.append("Camera: \(e)") }
    }
    return json(["screen": screenReport, "camera": cameraReport, "errors": errors])
}

@_cdecl("sokki_video_free")
public func sokki_video_free(_ s: UnsafeMutablePointer<CChar>?) {
    free(s)
}

// MARK: - Render

private enum Look: String {
    case bubble, side, vertical
}

private let background = CIImage(color: CIColor(red: 0.055, green: 0.059, blue: 0.067))

private extension CIImage {
    /// Scaled to fit inside `rect` (letterboxed), centred.
    func fitted(in rect: CGRect) -> CIImage {
        let s = min(rect.width / extent.width, rect.height / extent.height)
        let scaled = transformed(by: CGAffineTransform(scaleX: s, y: s))
        let x = rect.midX - scaled.extent.width / 2 - scaled.extent.minX
        let y = rect.midY - scaled.extent.height / 2 - scaled.extent.minY
        return scaled.transformed(by: CGAffineTransform(translationX: x, y: y))
    }

    /// Scaled to fill `rect` and cropped to it, centred.
    func filled(in rect: CGRect) -> CIImage {
        let s = max(rect.width / extent.width, rect.height / extent.height)
        let scaled = transformed(by: CGAffineTransform(scaleX: s, y: s))
        let x = rect.midX - scaled.extent.width / 2 - scaled.extent.minX
        let y = rect.midY - scaled.extent.height / 2 - scaled.extent.minY
        return scaled.transformed(by: CGAffineTransform(translationX: x, y: y)).cropped(to: rect)
    }

    /// Cut to a circle inside `rect` (a square).
    func circled(in rect: CGRect) -> CIImage {
        let r = rect.width / 2
        let mask = CIFilter(name: "CIRadialGradient", parameters: [
            "inputCenter": CIVector(x: rect.midX, y: rect.midY),
            "inputRadius0": r - 1,
            "inputRadius1": r,
            "inputColor0": CIColor.white,
            "inputColor1": CIColor.clear,
        ])!.outputImage!.cropped(to: rect)
        return applyingFilter("CIBlendWithMask", parameters: [
            kCIInputBackgroundImageKey: CIImage.empty(),
            kCIInputMaskImageKey: mask,
        ])
    }
}

private func renderSize(look: Look, screen: CGSize?, camera: CGSize?) -> CGSize {
    switch look {
    case .vertical:
        return CGSize(width: 1080, height: 1920)
    case .side:
        return CGSize(width: 1920, height: 1080)
    case .bubble:
        guard let base = screen ?? camera else { return CGSize(width: 1920, height: 1080) }
        let s = min(1, 1920 / base.width)
        return CGSize(width: Int(base.width * s) & ~1, height: Int(base.height * s) & ~1)
    }
}

private func compose(look: Look, size: CGSize, screen: CIImage?, camera: CIImage?) -> CIImage {
    let full = CGRect(origin: .zero, size: size)
    var image = background.cropped(to: full)
    func over(_ top: CIImage) { image = top.composited(over: image) }

    switch (screen, camera) {
    case let (screen?, nil):
        over(screen.fitted(in: full))
    case let (nil, camera?):
        over(camera.filled(in: full))
    case let (screen?, camera?):
        switch look {
        case .bubble:
            over(screen.fitted(in: full))
            let d = (size.height * 0.26).rounded()
            let m = (size.height * 0.04).rounded()
            let rect = CGRect(x: m, y: m, width: d, height: d)
            over(camera.filled(in: rect).circled(in: rect))
        case .side:
            let camWidth = (size.width * 0.3).rounded()
            over(screen.fitted(in: CGRect(x: 0, y: 0, width: size.width - camWidth, height: size.height)))
            over(camera.filled(in: CGRect(x: size.width - camWidth, y: 0, width: camWidth, height: size.height)))
        case .vertical:
            let camHeight = (size.height * 0.4).rounded()
            over(camera.filled(in: CGRect(x: 0, y: size.height - camHeight, width: size.width, height: camHeight)))
            over(screen.fitted(in: CGRect(x: 0, y: 0, width: size.width, height: size.height - camHeight)))
        }
    case (nil, nil):
        break
    }
    return image
}

private final class LookInstruction: NSObject, AVVideoCompositionInstructionProtocol {
    let timeRange: CMTimeRange
    let enablePostProcessing = false
    let containsTweening = true
    let requiredSourceTrackIDs: [NSValue]?
    let passthroughTrackID = kCMPersistentTrackID_Invalid
    let look: Look
    let screenTrack: CMPersistentTrackID?
    let cameraTrack: CMPersistentTrackID?
    let screenTransform: CGAffineTransform
    let cameraTransform: CGAffineTransform

    init(range: CMTimeRange, look: Look, screen: AVCompositionTrack?, camera: AVCompositionTrack?,
         screenTransform: CGAffineTransform, cameraTransform: CGAffineTransform) {
        timeRange = range
        self.look = look
        screenTrack = screen?.trackID
        cameraTrack = camera?.trackID
        self.screenTransform = screenTransform
        self.cameraTransform = cameraTransform
        requiredSourceTrackIDs = [screen, camera].compactMap { $0 }.map { NSNumber(value: $0.trackID) }
    }
}

private final class LookCompositor: NSObject, AVVideoCompositing {
    private let context = CIContext(options: [.cacheIntermediates: false])
    let sourcePixelBufferAttributes: [String: any Sendable]? = [
        kCVPixelBufferPixelFormatTypeKey as String: kCVPixelFormatType_32BGRA,
    ]
    let requiredPixelBufferAttributesForRenderContext: [String: any Sendable] = [
        kCVPixelBufferPixelFormatTypeKey as String: kCVPixelFormatType_32BGRA,
    ]

    func renderContextChanged(_ newRenderContext: AVVideoCompositionRenderContext) {}

    func startRequest(_ request: AVAsynchronousVideoCompositionRequest) {
        guard
            let instruction = request.videoCompositionInstruction as? LookInstruction,
            let out = request.renderContext.newPixelBuffer()
        else {
            request.finish(with: NSError(domain: "Sokki", code: 10))
            return
        }
        func frame(_ id: CMPersistentTrackID?, _ transform: CGAffineTransform) -> CIImage? {
            guard let id, let buffer = request.sourceFrame(byTrackID: id) else { return nil }
            let image = CIImage(cvPixelBuffer: buffer).transformed(by: transform)
            return image.transformed(by: CGAffineTransform(translationX: -image.extent.minX, y: -image.extent.minY))
        }
        let size = request.renderContext.size
        let image = compose(
            look: instruction.look,
            size: size,
            screen: frame(instruction.screenTrack, instruction.screenTransform),
            camera: frame(instruction.cameraTrack, instruction.cameraTransform)
        )
        context.render(image, to: out, bounds: CGRect(origin: .zero, size: size), colorSpace: CGColorSpace(name: CGColorSpace.sRGB))
        request.finish(withComposedVideoFrame: out)
    }
}

private struct Source {
    let url: URL
    let firstMicros: UInt64
}

private func source(_ value: Any?) -> Source? {
    guard let dict = value as? [String: Any], let path = dict["path"] as? String else { return nil }
    let first = (dict["first_us"] as? NSNumber)?.uint64Value ?? 0
    return Source(url: URL(fileURLWithPath: path), firstMicros: first)
}

private func render(_ cfg: [String: Any]) async throws {
    guard let look = Look(rawValue: cfg["look"] as? String ?? "") else {
        throw NSError(domain: "Sokki", code: 11, userInfo: [NSLocalizedDescriptionKey: "Unknown look."])
    }
    guard let out = cfg["out"] as? String else {
        throw NSError(domain: "Sokki", code: 12, userInfo: [NSLocalizedDescriptionKey: "No output path."])
    }
    let screen = source(cfg["screen"])
    let camera = source(cfg["camera"])
    let audio = (cfg["audio"] as? [Any] ?? []).compactMap(source)
    guard screen != nil || camera != nil else {
        throw NSError(domain: "Sokki", code: 13, userInfo: [NSLocalizedDescriptionKey: "Nothing to render."])
    }

    // Everything starts relative to the earliest track.
    let starts = ([screen, camera].compactMap { $0 } + audio).map(\.firstMicros).filter { $0 > 0 }
    let origin = starts.min() ?? 0
    func offset(_ s: Source) -> CMTime {
        let us = s.firstMicros > origin ? s.firstMicros - origin : 0
        return CMTime(value: CMTimeValue(us), timescale: 1_000_000)
    }

    let composition = AVMutableComposition()
    var end = CMTime.zero

    func addVideo(_ s: Source?) async throws -> (AVMutableCompositionTrack, CGSize, CGAffineTransform)? {
        guard let s else { return nil }
        let asset = AVURLAsset(url: s.url)
        guard let track = try await asset.loadTracks(withMediaType: .video).first else { return nil }
        let (range, natural, transform) = try await track.load(.timeRange, .naturalSize, .preferredTransform)
        guard let target = composition.addMutableTrack(withMediaType: .video, preferredTrackID: kCMPersistentTrackID_Invalid) else { return nil }
        try target.insertTimeRange(range, of: track, at: offset(s))
        end = CMTimeMaximum(end, CMTimeAdd(offset(s), range.duration))
        let oriented = natural.applying(transform)
        return (target, CGSize(width: abs(oriented.width), height: abs(oriented.height)), transform)
    }

    let screenTrack = try await addVideo(screen)
    let cameraTrack = try await addVideo(camera)
    guard screenTrack != nil || cameraTrack != nil else {
        throw NSError(domain: "Sokki", code: 14, userInfo: [NSLocalizedDescriptionKey: "The recorded video could not be read."])
    }

    var audioMix: [AVMutableAudioMixInputParameters] = []
    for s in audio {
        let asset = AVURLAsset(url: s.url)
        guard let track = try? await asset.loadTracks(withMediaType: .audio).first,
              let range = try? await track.load(.timeRange),
              let target = composition.addMutableTrack(withMediaType: .audio, preferredTrackID: kCMPersistentTrackID_Invalid)
        else { continue }
        // Audio runs only as long as the video.
        try target.insertTimeRange(range, of: track, at: offset(s))
        audioMix.append(AVMutableAudioMixInputParameters(track: target))
    }

    let size = renderSize(look: look, screen: screenTrack?.1, camera: cameraTrack?.1)
    let video = AVMutableVideoComposition()
    video.customVideoCompositorClass = LookCompositor.self
    video.renderSize = size
    video.frameDuration = CMTime(value: 1, timescale: 30)
    video.instructions = [LookInstruction(
        range: CMTimeRange(start: .zero, duration: end),
        look: look,
        screen: screenTrack?.0,
        camera: cameraTrack?.0,
        screenTransform: screenTrack?.2 ?? .identity,
        cameraTransform: cameraTrack?.2 ?? .identity
    )]

    let outURL = URL(fileURLWithPath: out)
    try? FileManager.default.removeItem(at: outURL)
    guard let export = AVAssetExportSession(asset: composition, presetName: AVAssetExportPresetHighestQuality) else {
        throw NSError(domain: "Sokki", code: 15, userInfo: [NSLocalizedDescriptionKey: "The video could not be exported."])
    }
    export.videoComposition = video
    if !audioMix.isEmpty {
        let mix = AVMutableAudioMix()
        mix.inputParameters = audioMix
        export.audioMix = mix
    }
    export.timeRange = CMTimeRange(start: .zero, duration: end)
    export.outputURL = outURL
    export.outputFileType = .mp4
    export.shouldOptimizeForNetworkUse = true
    await withCheckedContinuation { (done: CheckedContinuation<Void, Never>) in
        export.exportAsynchronously { done.resume() }
    }
    if export.status != .completed {
        throw export.error ?? NSError(domain: "Sokki", code: 16, userInfo: [NSLocalizedDescriptionKey: "The video could not be exported."])
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
