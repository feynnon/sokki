// Screen-recording compression bench: encodes the made-up test screens
// (make_screens.py) with a given setting and checks small text with OCR.
//
// Build: swiftc -O scripts/screen-eval/bench.swift -o "$SOKKI_EVALS/runs/screen-capture/bench"
// Run:   bench <screens dir> <out dir> <name> <codec hevc|h264> <height 0=native> <fps>
//              <quality 0..1, or 0 for the encoder default> [bitrate bit/s] [--realtime]
//
// The 75-second timeline behaves like ScreenCaptureKit: a frame is written
// only when the screen changed, at most <fps> per second.
//   0–15 s  slides (next slide every 5 s)
//  15–30 s  code editor, typing (a character every 0.25 s, cursor blink)
//  30–45 s  document, static
//  45–65 s  web page: scrolls one screen in 3 s, then rests 2 s (×3, then rests)
//  65–75 s  video call: four tiles with moving shapes and camera noise
//
// Prints one JSON line: size, MB per minute, frames, CPU seconds (encode only,
// frame drawing excluded), and per screen the share of small-text words and
// numbers OCR finds in the encoded frame, relative to OCR of the original.

import AVFoundation
import CoreGraphics
import Foundation
import ImageIO
import Vision

let args = CommandLine.arguments
guard args.count >= 8 else {
    FileHandle.standardError.write("usage: bench screens out name codec height fps quality [bitrate] [--realtime]\n".data(using: .utf8)!)
    exit(2)
}
let screensDir = URL(fileURLWithPath: args[1])
let outDir = URL(fileURLWithPath: args[2])
let name = args[3]
let codec: AVVideoCodecType = args[4] == "h264" ? .h264 : .hevc
let maxHeight = Int(args[5])!
let fps = Double(args[6])!
let quality = Double(args[7])!
let bitrate = args.count > 8 && !args[8].hasPrefix("--") ? Int(args[8])! : 0
let realtime = args.contains("--realtime")
try? FileManager.default.createDirectory(at: outDir, withIntermediateDirectories: true)

func load(_ n: String) -> CGImage {
    let src = CGImageSourceCreateWithURL(screensDir.appendingPathComponent("\(n).png") as CFURL, nil)!
    return CGImageSourceCreateImageAtIndex(src, 0, nil)!
}
let slides = load("slides"), code = load("code"), document = load("document"), web = load("web")
let nativeW = slides.width, nativeH = slides.height
var outH = maxHeight == 0 ? nativeH : min(maxHeight, nativeH)
var outW = Int((Double(nativeW) * Double(outH) / Double(nativeH)).rounded())
outW &= ~1; outH &= ~1

// MARK: timeline

let duration = 75.0

/// What is on screen at time t, as a key: a new key means the screen changed.
func stateKey(_ t: Double) -> String {
    switch t {
    case ..<15: return "slides\(Int(t / 5))"
    case ..<30:
        let typed = Int((t - 15) / 0.25)
        let blink = Int((t - 15) / 0.5) % 2
        return "code\(typed)-\(blink)"
    case ..<45: return "doc"
    case ..<65:
        let y = webScroll(t)
        return "web\(Int(y))"
    default: return "call\(Int(t * 1000))"
    }
}

/// Scroll position in native pixels.
func webScroll(_ t: Double) -> Double {
    let local = t - 45
    let step = Double(nativeH)
    let cycle = Int(local / 5)
    let inCycle = local - Double(cycle) * 5
    if cycle >= 3 { return step * 3 }
    return step * (Double(cycle) + min(inCycle / 3, 1))
}

let space = CGColorSpaceCreateDeviceRGB()
var rng = SystemRandomNumberGenerator()

func draw(_ t: Double, into ctx: CGContext) {
    ctx.interpolationQuality = .high
    let dst = CGRect(x: 0, y: 0, width: outW, height: outH)
    switch t {
    case ..<15:
        ctx.draw(slides, in: dst)
        // A different slide: a coloured bar that moves (the text stays for OCR).
        let k = Int(t / 5)
        ctx.setFillColor(CGColor(red: 1, green: 0.6, blue: 0.2, alpha: 1))
        ctx.fill(CGRect(x: Double(outW) * (0.1 + 0.25 * Double(k)), y: Double(outH) * 0.03, width: Double(outW) * 0.2, height: Double(outH) * 0.01))
    case ..<30:
        ctx.draw(code, in: dst)
        let typed = Int((t - 15) / 0.25)
        let sx = Double(outW) / Double(nativeW)
        // "Typing" on line 62 below the code: one more block per character.
        ctx.setFillColor(CGColor(red: 0.83, green: 0.83, blue: 0.83, alpha: 1))
        for i in 0..<typed {
            ctx.fill(CGRect(x: (560 + Double(i % 80) * 14.4) * sx, y: (2234 - 2280 + Double(i / 80) * 36) * sx + 40 * sx, width: 12 * sx, height: 18 * sx))
        }
        if Int((t - 15) / 0.5) % 2 == 0 {
            ctx.fill(CGRect(x: (560 + Double(typed % 80) * 14.4) * sx, y: 40 * sx, width: 3 * sx, height: 24 * sx))
        }
    case ..<45:
        ctx.draw(document, in: dst)
    case ..<65:
        let y = webScroll(t)
        let crop = web.cropping(to: CGRect(x: 0, y: Int(y), width: nativeW, height: nativeH))!
        ctx.draw(crop, in: dst)
    default:
        ctx.setFillColor(CGColor(gray: 0.1, alpha: 1))
        ctx.fill(dst)
        let tw = Double(outW) / 2, th = Double(outH) / 2
        for i in 0..<4 {
            let ox = Double(i % 2) * tw, oy = Double(i / 2) * th
            let hue = Double(i) / 4
            ctx.setFillColor(CGColor(red: 0.3 + 0.5 * hue, green: 0.4, blue: 0.7 - 0.4 * hue, alpha: 1))
            ctx.fill(CGRect(x: ox + 6, y: oy + 6, width: tw - 12, height: th - 12))
            // A "head" that moves.
            let cx = ox + tw / 2 + sin(t * 1.3 + Double(i)) * tw * 0.08
            let cy = oy + th / 2 + cos(t * 0.9 + Double(i)) * th * 0.05
            ctx.setFillColor(CGColor(red: 0.95, green: 0.8, blue: 0.65, alpha: 1))
            ctx.fillEllipse(in: CGRect(x: cx - tw * 0.12, y: cy - th * 0.18, width: tw * 0.24, height: th * 0.36))
            // Camera noise.
            for _ in 0..<400 {
                let g = Double.random(in: 0...1, using: &rng)
                ctx.setFillColor(CGColor(gray: g, alpha: 0.15))
                ctx.fill(CGRect(x: ox + Double.random(in: 0..<tw, using: &rng), y: oy + Double.random(in: 0..<th, using: &rng), width: 4, height: 4))
            }
        }
    }
}

func makeBuffer(_ t: Double, pool: CVPixelBufferPool) -> CVPixelBuffer {
    var pb: CVPixelBuffer?
    CVPixelBufferPoolCreatePixelBuffer(nil, pool, &pb)
    let buf = pb!
    CVPixelBufferLockBaseAddress(buf, [])
    let ctx = CGContext(
        data: CVPixelBufferGetBaseAddress(buf), width: outW, height: outH, bitsPerComponent: 8,
        bytesPerRow: CVPixelBufferGetBytesPerRow(buf), space: space,
        bitmapInfo: CGImageAlphaInfo.premultipliedFirst.rawValue | CGBitmapInfo.byteOrder32Little.rawValue)!
    draw(t, into: ctx)
    CVPixelBufferUnlockBaseAddress(buf, [])
    return buf
}

func threadCPU() -> Double {
    var ts = timespec()
    clock_gettime(CLOCK_THREAD_CPUTIME_ID, &ts)
    return Double(ts.tv_sec) + Double(ts.tv_nsec) / 1e9
}

func processCPU() -> Double {
    var u = rusage()
    getrusage(RUSAGE_SELF, &u)
    return Double(u.ru_utime.tv_sec) + Double(u.ru_utime.tv_usec) / 1e6 + Double(u.ru_stime.tv_sec) + Double(u.ru_stime.tv_usec) / 1e6
}

// MARK: encode

let movURL = outDir.appendingPathComponent("\(name).mov")
try? FileManager.default.removeItem(at: movURL)
let writer = try! AVAssetWriter(outputURL: movURL, fileType: .mov)
var props: [String: Any] = [
    AVVideoExpectedSourceFrameRateKey: fps,
]
if quality > 0 { props[AVVideoQualityKey] = quality }
if bitrate > 0 { props[AVVideoAverageBitRateKey] = bitrate }
// Note: the current Sokki writer passes no compression properties at all.
let isDefault = quality == 0 && bitrate == 0
var settings: [String: Any] = [AVVideoCodecKey: codec, AVVideoWidthKey: outW, AVVideoHeightKey: outH]
if !isDefault {
    props[AVVideoMaxKeyFrameIntervalDurationKey] = 10.0
    props[AVVideoAllowFrameReorderingKey] = false
    if codec == .h264 { props[AVVideoProfileLevelKey] = AVVideoProfileLevelH264HighAutoLevel }
    settings[AVVideoCompressionPropertiesKey] = props
}
let input = AVAssetWriterInput(mediaType: .video, outputSettings: settings)
input.expectsMediaDataInRealTime = realtime
let adaptor = AVAssetWriterInputPixelBufferAdaptor(assetWriterInput: input, sourcePixelBufferAttributes: [
    kCVPixelBufferPixelFormatTypeKey as String: kCVPixelFormatType_32BGRA,
    kCVPixelBufferWidthKey as String: outW, kCVPixelBufferHeightKey as String: outH,
    kCVPixelBufferIOSurfacePropertiesKey as String: [:],
])
writer.add(input)
writer.startWriting()
writer.startSession(atSourceTime: .zero)

let cpu0 = processCPU()
let wall0 = Date()
var drawCPU = 0.0
var frames = 0
var lastKey = ""
let step = 1.0 / fps
var t = 0.0
while t < duration {
    let key = stateKey(t)
    if key != lastKey {
        lastKey = key
        while !input.isReadyForMoreMediaData { usleep(1000) }
        let d0 = threadCPU()
        let buf = makeBuffer(t, pool: adaptor.pixelBufferPool!)
        drawCPU += threadCPU() - d0
        if realtime {
            let due = wall0.addingTimeInterval(t)
            let wait = due.timeIntervalSinceNow
            if wait > 0 { usleep(UInt32(wait * 1e6)) }
        }
        adaptor.append(buf, withPresentationTime: CMTime(seconds: t, preferredTimescale: 600))
        frames += 1
    }
    t += step
}
input.markAsFinished()
writer.endSession(atSourceTime: CMTime(seconds: duration, preferredTimescale: 600))
let sem = DispatchSemaphore(value: 0)
writer.finishWriting { sem.signal() }
sem.wait()
let encodeCPU = processCPU() - cpu0 - drawCPU
let wall = Date().timeIntervalSince(wall0)
guard writer.status == .completed else {
    print("{\"name\":\"\(name)\",\"error\":\"\(writer.error?.localizedDescription ?? "?")\"}")
    exit(1)
}
let bytes = (try! FileManager.default.attributesOfItem(atPath: movURL.path)[.size] as! NSNumber).intValue

// MARK: OCR

func ocr(_ image: CGImage) -> [String] {
    let req = VNRecognizeTextRequest()
    req.recognitionLevel = .accurate
    req.recognitionLanguages = ["en-US"]
    req.usesLanguageCorrection = false  // strict: no guessing of blurry words
    try! VNImageRequestHandler(cgImage: image).perform([req])
    let text = (req.results ?? []).compactMap { $0.topCandidates(1).first?.string }.joined(separator: " ")
    return tokens(text)
}

func tokens(_ s: String) -> [String] {
    s.lowercased().components(separatedBy: CharacterSet.alphanumerics.inverted).filter { $0.count >= 2 }
}

func counts(_ a: [String]) -> [String: Int] { a.reduce(into: [:]) { $0[$1, default: 0] += 1 } }

let truthData = try! Data(contentsOf: screensDir.appendingPathComponent("truth.json"))
let truth = try! JSONSerialization.jsonObject(with: truthData) as! [String: [String]]

/// Original frame (native) for a sample time.
func original(_ t: Double) -> CGImage {
    let ctx = CGContext(data: nil, width: nativeW, height: nativeH, bitsPerComponent: 8, bytesPerRow: 0, space: space,
                        bitmapInfo: CGImageAlphaInfo.premultipliedFirst.rawValue | CGBitmapInfo.byteOrder32Little.rawValue)!
    let saveW = outW, saveH = outH
    outW = nativeW; outH = nativeH
    draw(t, into: ctx)
    outW = saveW; outH = saveH
    return ctx.makeImage()!
}

func upscale(_ img: CGImage) -> CGImage {
    if img.width == nativeW { return img }
    let ctx = CGContext(data: nil, width: nativeW, height: nativeH, bitsPerComponent: 8, bytesPerRow: 0, space: space,
                        bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
    ctx.interpolationQuality = .high
    ctx.draw(img, in: CGRect(x: 0, y: 0, width: nativeW, height: nativeH))
    return ctx.makeImage()!
}

let asset = AVURLAsset(url: movURL)
let gen = AVAssetImageGenerator(asset: asset)
gen.requestedTimeToleranceBefore = CMTime(seconds: 1.0 / fps + 0.01, preferredTimescale: 600)
gen.requestedTimeToleranceAfter = .zero
gen.appliesPreferredTrackTransform = true

// Sample times: still screens, and the web page both resting and mid-scroll.
let samples: [(String, String, Double)] = [
    ("slides", "slides", 14.5), ("code", "code", 29.5), ("document", "document", 44.5),
    ("web_rest", "web", 49.5), ("web_rest", "web", 54.5), ("web_rest", "web", 64.5),
    ("web_scroll", "web", 46.5), ("web_scroll", "web", 51.5),
]
var perScreen: [String: (found: Int, total: Int, numFound: Int, numTotal: Int)] = [:]
for (label, screen, st) in samples {
    // The frame on screen at st: the last one on the fps grid.
    let at = (st / step).rounded(.down) * step
    let small = Set(tokens(truth[screen]!.joined(separator: " ")))
    let refWords = ocr(original(at)).filter { small.contains($0) }
    let ref = counts(refWords)
    let cg = try! gen.copyCGImage(at: CMTime(seconds: st, preferredTimescale: 600), actualTime: nil)
    // Shown full screen, as a viewer watches it: scaled back up.
    let got = counts(ocr(upscale(cg)))
    var r = perScreen[label] ?? (0, 0, 0, 0)
    for (w, n) in ref {
        let f = min(n, got[w] ?? 0)
        r.found += f; r.total += n
        if w.allSatisfy(\.isNumber) { r.numFound += f; r.numTotal += n }
    }
    perScreen[label] = r
    // A crop to look at: 600x300 logical points from the middle-left,
    // shown at its logical size (what a viewer sees at 100 %).
    if st == 44.5 || st == 29.5 || st == 54.5 {
        let fx = Double(cg.width) / Double(nativeW) * 2
        let crop = cg.cropping(to: CGRect(x: Int(300 * fx), y: Int(120 * fx), width: Int(600 * fx), height: Int(300 * fx)))!
        let ctx = CGContext(data: nil, width: 1200, height: 600, bitsPerComponent: 8, bytesPerRow: 0, space: space,
                            bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
        ctx.interpolationQuality = .high
        ctx.draw(crop, in: CGRect(x: 0, y: 0, width: 1200, height: 600))
        let url = outDir.appendingPathComponent("\(name)-\(screen).png")
        let dest = CGImageDestinationCreateWithURL(url as CFURL, "public.png" as CFString, 1, nil)!
        CGImageDestinationAddImage(dest, ctx.makeImage()!, nil)
        CGImageDestinationFinalize(dest)
    }
}

var result: [String: Any] = [
    "name": name, "codec": args[4], "width": outW, "height": outH, "fps": fps, "quality": quality, "bitrate": bitrate,
    "bytes": bytes, "mb_per_min": Double(bytes) / 1e6 / duration * 60, "frames": frames,
    "encode_cpu_s": encodeCPU, "draw_cpu_s": drawCPU, "wall_s": wall, "realtime": realtime,
]
for (k, v) in perScreen {
    result["ocr_\(k)"] = v.total > 0 ? Double(v.found) / Double(v.total) : -1
    result["num_\(k)"] = v.numTotal > 0 ? Double(v.numFound) / Double(v.numTotal) : -1
    result["n_\(k)"] = v.total
}
let line = try! JSONSerialization.data(withJSONObject: result, options: [.sortedKeys])
print(String(decoding: line, as: UTF8.self))
