// Finds elements in an app's window by their accessible label and clicks
// them with real mouse events, types real keys, or reads a field's value:
// for night tests that drive Sokki Test's window the way a person does.
// Usage:
//   ax <pid> wait <label> [seconds]   exit 0 once an element with the label shows
//   ax <pid> click <label>            mouse click at the element's middle
//   ax <pid> value <label>            prints the element's value (a text field)
//   ax <pid> gone <label> [seconds]   exit 0 once no element has the label
//   ax type <text>                    types the text into the focused field
//   ax key escape|return|selectall    one key (selectall: Cmd+A)
// A label matches an element's title, description or value, exactly; with
// a trailing "*" it matches as a prefix. Front-window test: run ONLY when
// nobody uses the Mac (night run). Needs Accessibility for the caller.
import AppKit
import ApplicationServices
import Foundation

func fail(_ s: String) -> Never {
    FileHandle.standardError.write((s + "\n").data(using: .utf8)!)
    exit(1)
}

func attr(_ e: AXUIElement, _ name: String) -> AnyObject? {
    var v: AnyObject?
    return AXUIElementCopyAttributeValue(e, name as CFString, &v) == .success ? v : nil
}

func labels(_ e: AXUIElement) -> [String] {
    [kAXTitleAttribute, kAXDescriptionAttribute, kAXValueAttribute]
        .compactMap { attr(e, $0) as? String }
        .filter { !$0.isEmpty }
}

func matches(_ e: AXUIElement, _ label: String) -> Bool {
    if label.hasSuffix("*") {
        let p = String(label.dropLast())
        return labels(e).contains { $0.hasPrefix(p) }
    }
    return labels(e).contains(label)
}

func find(_ pid: pid_t, _ label: String) -> AXUIElement? {
    let app = AXUIElementCreateApplication(pid)
    var queue: [(AXUIElement, Int)] = ((attr(app, kAXWindowsAttribute) as? [AXUIElement]) ?? []).map { ($0, 0) }
    var seen = 0
    while !queue.isEmpty && seen < 20_000 {
        let (e, depth) = queue.removeFirst()
        seen += 1
        if matches(e, label) { return e }
        if depth < 60, let kids = attr(e, kAXChildrenAttribute) as? [AXUIElement] {
            queue.append(contentsOf: kids.map { ($0, depth + 1) })
        }
    }
    return nil
}

func frame(_ e: AXUIElement) -> CGRect? {
    guard let p = attr(e, kAXPositionAttribute), let s = attr(e, kAXSizeAttribute) else { return nil }
    var point = CGPoint.zero, size = CGSize.zero
    AXValueGetValue(p as! AXValue, .cgPoint, &point)
    AXValueGetValue(s as! AXValue, .cgSize, &size)
    return CGRect(origin: point, size: size)
}

let src = CGEventSource(stateID: .hidSystemState)

func click(_ at: CGPoint) {
    for t in [CGEventType.mouseMoved, .leftMouseDown, .leftMouseUp] {
        CGEvent(mouseEventSource: src, mouseType: t, mouseCursorPosition: at, mouseButton: .left)?
            .post(tap: .cghidEventTap)
        usleep(60_000)
    }
}

func key(_ code: CGKeyCode, flags: CGEventFlags = []) {
    for down in [true, false] {
        let e = CGEvent(keyboardEventSource: src, virtualKey: code, keyDown: down)!
        e.flags = flags
        e.post(tap: .cghidEventTap)
        usleep(40_000)
    }
}

func type(_ text: String) {
    for ch in text.utf16 {
        for down in [true, false] {
            let e = CGEvent(keyboardEventSource: src, virtualKey: 0, keyDown: down)!
            var c = ch
            e.keyboardSetUnicodeString(stringLength: 1, unicodeString: &c)
            e.post(tap: .cghidEventTap)
        }
        usleep(40_000)
    }
}

func waitFor(_ pid: pid_t, _ label: String, _ secs: Double, present: Bool) -> Bool {
    let end = Date().addingTimeInterval(secs)
    repeat {
        if (find(pid, label) != nil) == present { return true }
        usleep(250_000)
    } while Date() < end
    return false
}

var args = Array(CommandLine.arguments.dropFirst())
guard AXIsProcessTrusted() else { fail("no Accessibility permission for this process") }
if args.first == "type", args.count == 2 { type(args[1]); exit(0) }
if args.first == "key", args.count == 2 {
    switch args[1] {
    case "escape": key(53)
    case "return": key(36)
    case "selectall": key(0, flags: .maskCommand)
    default: fail("unknown key \(args[1])")
    }
    exit(0)
}
guard args.count >= 3, let pid = pid_t(args[0]) else { fail("usage: see the top of ax.swift") }
let (cmd, label) = (args[1], args[2])
let secs = args.count > 3 ? Double(args[3]) ?? 10 : 10
switch cmd {
case "wait":
    exit(waitFor(pid, label, secs, present: true) ? 0 : 1)
case "gone":
    exit(waitFor(pid, label, secs, present: false) ? 0 : 1)
case "click":
    guard waitFor(pid, label, secs, present: true), let e = find(pid, label), let f = frame(e) else {
        fail("not found: \(label)")
    }
    NSRunningApplication(processIdentifier: pid)?.activate()
    usleep(300_000)
    click(CGPoint(x: f.midX, y: f.midY))
case "value":
    guard let e = find(pid, label) else { fail("not found: \(label)") }
    print((attr(e, kAXValueAttribute) as? String) ?? "")
default:
    fail("unknown command \(cmd)")
}
