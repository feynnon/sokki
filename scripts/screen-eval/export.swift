// What Sokki's render did before 2026-11-27: AVAssetExportPresetHighestQuality
// of the screen video into .mp4. Prints its size as one JSON line.
// Usage: export <in.mov> <out.mp4>

import AVFoundation

let args = CommandLine.arguments
let inURL = URL(fileURLWithPath: args[1]), outURL = URL(fileURLWithPath: args[2])
try? FileManager.default.removeItem(at: outURL)
let asset = AVURLAsset(url: inURL)
let export = AVAssetExportSession(asset: asset, presetName: AVAssetExportPresetHighestQuality)!
export.outputURL = outURL
export.outputFileType = .mp4
export.shouldOptimizeForNetworkUse = true
let t0 = Date()
let sem = DispatchSemaphore(value: 0)
export.exportAsynchronously { sem.signal() }
sem.wait()
let bytes = (try? FileManager.default.attributesOfItem(atPath: outURL.path)[.size] as? NSNumber)?.intValue ?? 0
let duration = CMTimeGetSeconds(asset.duration)
print("{\"name\":\"current-export\",\"status\":\(export.status.rawValue),\"bytes\":\(bytes),\"mb_per_min\":\(Double(bytes) / 1e6 / duration * 60),\"wall_s\":\(Date().timeIntervalSince(t0))}")
