// Fallback used when the macOS SDK does not provide Core Audio process taps
// (CATapDescription). Every entry point reports "unsupported", so meeting
// recording is disabled in the UI and the rest of the app is unaffected.

import Foundation

@_cdecl("sokki_tap_supported")
public func sokki_tap_supported() -> Bool {
    return false
}

@_cdecl("sokki_tap_start")
public func sokki_tap_start(
    _ callback: @escaping @convention(c) (UnsafeMutableRawPointer?, UnsafePointer<Float>?, Int, UInt32, Double) -> Void,
    _ user: UnsafeMutableRawPointer?,
    _ outError: UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>?
) -> UnsafeMutableRawPointer? {
    outError?.pointee = strdup("System audio capture was not built into this Sokki binary.")
    return nil
}

@_cdecl("sokki_tap_stop")
public func sokki_tap_stop(_ handle: UnsafeMutableRawPointer?) {}

@_cdecl("sokki_tap_rebuild")
public func sokki_tap_rebuild(
    _ handle: UnsafeMutableRawPointer?,
    _ outError: UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>?
) -> Bool {
    outError?.pointee = strdup("System audio capture was not built into this Sokki binary.")
    return false
}

@_cdecl("sokki_tap_other_apps_playing")
public func sokki_tap_other_apps_playing(_ buffer: UnsafeMutablePointer<CChar>?, _ capacity: Int) -> Int32 {
    return -1
}

@_cdecl("sokki_tap_default_output")
public func sokki_tap_default_output(_ buffer: UnsafeMutablePointer<CChar>?, _ capacity: Int) -> UInt32 {
    return 0
}

@_cdecl("sokki_tap_free_error")
public func sokki_tap_free_error(_ error: UnsafeMutablePointer<CChar>?) {
    free(error)
}
