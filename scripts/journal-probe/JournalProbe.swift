// A stand-in app for testing "Keep what I write" (scripts/test-journal.sh):
// a window far off screen with a text area and a password field, driven by
// lines on stdin. Never activates, no Dock icon, nothing on screen.
//
//   type <s>     focus the text area and type <s> at its end, key by key
//                (30 ms apart; the key count goes to $PROBE_KEYS)
//   paste <s>    focus the text area and add <s> in one go (no keys)
//   text <s>     focus the text area and replace its text in one go
//   back <n>     press backspace n times
//   clear        empty the text area (as if the message was sent)
//   secure <s>   focus the password field and set its text
//   hang <s>     block the app for <s> seconds (a hung app)
//   quit
//
// It reports its focused element itself: an app that is never active has
// no key window, so AppKit would report none.

import AppKit

final class ProbeApp: NSApplication {
    var focused: Any?
    override func accessibilityApplicationFocusedUIElement() -> Any? { focused }
    override func accessibilityAttributeValue(_ attribute: NSAccessibility.Attribute) -> Any? {
        if attribute == .focusedUIElement { return focused }
        return super.accessibilityAttributeValue(attribute)
    }
}

let app = ProbeApp.shared as! ProbeApp
// Accessory: no Dock icon, never activated, but known to Accessibility.
app.setActivationPolicy(.accessory)

let window = NSWindow(
    contentRect: NSRect(x: -30000, y: -30000, width: 420, height: 220),
    styleMask: [.titled], backing: .buffered, defer: false)
let area = NSTextView(frame: NSRect(x: 0, y: 40, width: 420, height: 180))
area.setAccessibilityPlaceholderValue("Message")
let secure = NSSecureTextField(frame: NSRect(x: 0, y: 0, width: 420, height: 24))
window.contentView?.addSubview(area)
window.contentView?.addSubview(secure)
app.focused = area

var keys = 0
let keysFile = ProcessInfo.processInfo.environment["PROBE_KEYS"]
func press() {
    keys += 1
    if let f = keysFile { try? String(keys).write(toFile: f, atomically: true, encoding: .utf8) }
}
press(); keys = 0
if let f = keysFile { try? "0".write(toFile: f, atomically: true, encoding: .utf8) }

print("pid \(getpid())")
fflush(stdout)

DispatchQueue.global().async {
    while let line = readLine() {
        let (cmd, arg): (Substring, String) = {
            let parts = line.split(separator: " ", maxSplits: 1)
            return (parts.first ?? "", parts.count > 1 ? String(parts[1]) : "")
        }()
        if cmd == "type" {
            for ch in arg {
                DispatchQueue.main.sync {
                    app.focused = area
                    press()
                    area.string += String(ch)
                }
                Thread.sleep(forTimeInterval: 0.03)
            }
            continue
        }
        if cmd == "back" {
            for _ in 0..<(Int(arg) ?? 1) {
                DispatchQueue.main.sync {
                    press()
                    area.string = String(area.string.dropLast())
                }
                Thread.sleep(forTimeInterval: 0.03)
            }
            continue
        }
        DispatchQueue.main.sync {
            switch cmd {
            case "paste":
                app.focused = area
                area.string += arg
            case "text":
                app.focused = area
                area.string = arg
            case "clear":
                area.string = ""
            case "secure":
                app.focused = secure
                secure.stringValue = arg
            case "hang":
                Thread.sleep(forTimeInterval: Double(arg) ?? 1)
            case "quit":
                exit(0)
            default:
                FileHandle.standardError.write("unknown: \(line)\n".data(using: .utf8)!)
            }
        }
    }
    exit(0)
}
app.run()
