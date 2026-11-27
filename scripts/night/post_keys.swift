// Posts real keyboard events (with FlagsChanged for Option) the way a
// keyboard does, for the night test of the meeting shortcut.
// Usage: post_keys deliberate | typing
// Front-window test: run only when nobody uses the Mac (night run).
import CoreGraphics
import Foundation

let optKey: CGKeyCode = 58  // left Option
let mKey: CGKeyCode = 46
let uKey: CGKeyCode = 32
let oKey: CGKeyCode = 31
let src = CGEventSource(stateID: .hidSystemState)

func post(_ key: CGKeyCode, down: Bool, flags: CGEventFlags, modifier: Bool = false) {
    guard let e = CGEvent(keyboardEventSource: src, virtualKey: key, keyDown: down) else {
        fatalError("could not create event")
    }
    if modifier { e.type = .flagsChanged }
    e.flags = flags
    e.post(tap: .cghidEventTap)
    usleep(40_000)  // 40 ms between keys: fast typing
}

let opt = CGEventFlags.maskAlternate
switch CommandLine.arguments.dropFirst().first {
case "deliberate":
    // Option down, M, Option up: a deliberate Option+M.
    post(optKey, down: true, flags: opt, modifier: true)
    post(mKey, down: true, flags: opt)
    post(mKey, down: false, flags: opt)
    post(optKey, down: false, flags: [], modifier: true)
case "typing":
    // "mö" typed fast on the US layout: Option goes down before M, then
    // Option+U (umlaut dead key), then o.
    post(optKey, down: true, flags: opt, modifier: true)
    post(mKey, down: true, flags: opt)
    post(mKey, down: false, flags: opt)
    post(uKey, down: true, flags: opt)
    post(uKey, down: false, flags: opt)
    post(optKey, down: false, flags: [], modifier: true)
    post(oKey, down: true, flags: [])
    post(oKey, down: false, flags: [])
default:
    print("usage: post_keys deliberate|typing")
    exit(2)
}
