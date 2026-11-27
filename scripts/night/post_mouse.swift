// Real mouse events and window lookups for the night test of the
// recording pill (test-pill-click.sh). Coordinates are global, top left.
// Usage:
//   post_mouse pill            the pill's frame: "x y w h" (exit 1: none)
//   post_mouse main            "1" if Sokki's main window is on screen, else "0"
//   post_mouse click X Y       a left click at X Y
//   post_mouse drag X Y DX     press at X Y, drag DX px right, let go
//   post_mouse space left|right  Control+Arrow: switch Space
// Front-window test: run only when nobody uses the Mac (night run).
import CoreGraphics
import Foundation

let src = CGEventSource(stateID: .hidSystemState)

/// Sokki's windows on screen right now: (frame, layer).
func sokkiWindows() -> [CGRect] {
    let opts: CGWindowListOption = [.optionOnScreenOnly, .excludeDesktopElements]
    guard let list = CGWindowListCopyWindowInfo(opts, kCGNullWindowID) as? [[String: Any]] else {
        return []
    }
    return list.compactMap { w in
        guard (w[kCGWindowOwnerName as String] as? String) == "Sokki",
              let b = w[kCGWindowBounds as String] as? [String: CGFloat] else { return nil }
        return CGRect(x: b["X"] ?? 0, y: b["Y"] ?? 0, width: b["Width"] ?? 0, height: b["Height"] ?? 0)
    }
}

/// The pill: 40 px high (72 with its tooltip), narrower than 400 px.
func pill() -> CGRect? {
    sokkiWindows().first { $0.height >= 38 && $0.height <= 80 && $0.width < 400 }
}

func mouse(_ type: CGEventType, _ p: CGPoint) {
    guard let e = CGEvent(mouseEventSource: src, mouseType: type, mouseCursorPosition: p, mouseButton: .left) else {
        fatalError("could not create event")
    }
    e.post(tap: .cghidEventTap)
}

func key(_ code: CGKeyCode, down: Bool) {
    guard let e = CGEvent(keyboardEventSource: src, virtualKey: code, keyDown: down) else {
        fatalError("could not create event")
    }
    e.flags = .maskControl
    e.post(tap: .cghidEventTap)
}

let args = Array(CommandLine.arguments.dropFirst())
func num(_ i: Int) -> CGFloat {
    guard i < args.count, let v = Double(args[i]) else {
        print("bad number at \(i)"); exit(2)
    }
    return CGFloat(v)
}

switch args.first {
case "pill":
    guard let p = pill() else { exit(1) }
    print("\(Int(p.minX)) \(Int(p.minY)) \(Int(p.width)) \(Int(p.height))")
case "main":
    print(sokkiWindows().contains { $0.height > 300 } ? "1" : "0")
case "click":
    let p = CGPoint(x: num(1), y: num(2))
    mouse(.mouseMoved, p); usleep(300_000)  // hover first, as a hand would
    mouse(.leftMouseDown, p); usleep(80_000)
    mouse(.leftMouseUp, p)
case "drag":
    let p = CGPoint(x: num(1), y: num(2))
    let dx = num(3)
    mouse(.mouseMoved, p); usleep(300_000)
    mouse(.leftMouseDown, p); usleep(80_000)
    let steps = 10
    for i in 1...steps {
        mouse(.leftMouseDragged, CGPoint(x: p.x + dx * CGFloat(i) / CGFloat(steps), y: p.y))
        usleep(30_000)
    }
    usleep(100_000)
    mouse(.leftMouseUp, CGPoint(x: p.x + dx, y: p.y))
case "space":
    let code: CGKeyCode = args.count > 1 && args[1] == "left" ? 123 : 124
    key(code, down: true); usleep(40_000); key(code, down: false)
default:
    print("usage: post_mouse pill|main|click X Y|drag X Y DX|space left|right")
    exit(2)
}
