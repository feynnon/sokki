//! Part of the lab: what an app shows Accessibility, step by step, with error codes.
use std::ffi::c_void;
use std::ptr;
type R = *const c_void;
#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXUIElementCreateApplication(pid: i32) -> R;
    fn AXUIElementCreateSystemWide() -> R;
    fn AXUIElementCopyAttributeValue(e: R, a: R, out: *mut R) -> i32;
    fn AXUIElementSetAttributeValue(e: R, a: R, v: R) -> i32;
    fn AXUIElementCopyAttributeNames(e: R, out: *mut R) -> i32;
    fn AXUIElementGetPid(e: R, pid: *mut i32) -> i32;
    fn AXValueCreate(kind: u32, v: *const c_void) -> R;
    fn CGEventCreateKeyboardEvent(source: R, key: u16, down: bool) -> R;
    fn CGEventKeyboardSetUnicodeString(e: R, n: usize, s: *const u16);
    fn CGEventSetFlags(e: R, flags: u64);
    fn CGEventPostToPid(pid: i32, e: R);
    fn CGEventPost(tap: u32, e: R);
    fn AXUIElementCopyParameterizedAttributeNames(e: R, out: *mut R) -> i32;
    fn AXUIElementIsAttributeSettable(e: R, a: R, out: *mut u8) -> i32;
}
#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    static kCFBooleanTrue: R;
    fn CFStringCreateWithBytes(a: R, b: *const u8, n: isize, e: u32, x: u8) -> R;
    fn CFCopyDescription(r: R) -> R;
    fn CFStringGetCString(s: R, b: *mut u8, n: isize, e: u32) -> u8;
    fn CFArrayGetCount(a: R) -> isize;
    fn CFArrayGetValueAtIndex(a: R, i: isize) -> R;
}
fn s(x: &str) -> R {
    unsafe { CFStringCreateWithBytes(ptr::null(), x.as_ptr(), x.len() as isize, 0x0800_0100, 0) }
}
fn desc(r: R) -> String {
    desc_n(r, 160)
}
fn desc_n(r: R, n: usize) -> String {
    if r.is_null() {
        return "null".into();
    }
    unsafe {
        let d = CFCopyDescription(r);
        let mut b = vec![0u8; 4096];
        CFStringGetCString(d, b.as_mut_ptr(), 4096, 0x0800_0100);
        let e = b.iter().position(|&c| c == 0).unwrap_or(0);
        String::from_utf8_lossy(&b[..e]).chars().take(n).collect()
    }
}
fn get(e: R, a: &str) -> (i32, R) {
    let mut o: R = ptr::null();
    let err = unsafe { AXUIElementCopyAttributeValue(e, s(a), &mut o) };
    (err, o)
}
fn show(label: &str, e: R) {
    let (_, role) = get(e, "AXRole");
    let (_, sub) = get(e, "AXSubrole");
    let mut pid = 0;
    unsafe { AXUIElementGetPid(e, &mut pid) };
    println!(
        "{label}: pid {pid} role {} subrole {}",
        desc(role),
        desc(sub)
    );
}
fn walk(e: R, depth: usize, budget: &mut usize) {
    if *budget == 0 || depth > 25 {
        return;
    }
    *budget -= 1;
    let (_, role) = get(e, "AXRole");
    let r = desc(role);
    let (fe, foc) = get(e, "AXFocused");
    let (ve, val) = get(e, "AXValue");
    let interesting = r.contains("Text") || r.contains("WebArea") || r.contains("ComboBox");
    if interesting {
        println!(
            "{}{} focused {} value {}",
            "  ".repeat(depth.min(12)),
            r.rsplit('"').nth(1).unwrap_or(&r),
            if fe == 0 {
                desc(foc)
            } else {
                format!("err {fe}")
            },
            if ve == 0 {
                desc(val)
            } else {
                format!("err {ve}")
            }
        );
    }
    let (err, kids) = get(e, "AXChildren");
    if err == 0 && !kids.is_null() {
        for i in 0..unsafe { CFArrayGetCount(kids) } {
            walk(
                unsafe { CFArrayGetValueAtIndex(kids, i) },
                depth + 1,
                budget,
            );
        }
    }
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let pid: i32 = a[1].parse().unwrap();
    let mode = a.get(2).map(|s| s.as_str()).unwrap_or("");
    let app = unsafe { AXUIElementCreateApplication(pid) };
    if mode.contains("manual") {
        println!("set AXManualAccessibility: {}", unsafe {
            AXUIElementSetAttributeValue(app, s("AXManualAccessibility"), kCFBooleanTrue)
        });
    }
    if mode.contains("front") {
        println!("front: {}", unsafe {
            AXUIElementSetAttributeValue(app, s("AXFrontmost"), kCFBooleanTrue)
        });
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
    if mode.contains("enhanced") {
        println!("set AXEnhancedUserInterface: {}", unsafe {
            AXUIElementSetAttributeValue(app, s("AXEnhancedUserInterface"), kCFBooleanTrue)
        });
    }
    if mode.contains("wait") {
        std::thread::sleep(std::time::Duration::from_millis(1500));
    }
    let mut names: R = ptr::null();
    unsafe { AXUIElementCopyAttributeNames(app, &mut names) };
    println!("app attributes: {}", desc(names).replace('\n', " "));
    let (err, f) = get(app, "AXFocusedUIElement");
    println!("app AXFocusedUIElement err {err}");
    if err == 0 {
        show("  focused", f)
    }
    if mode.contains("keys") && err == 0 {
        let val = |f: R| desc(get(f, "AXValue").1);
        println!("before: {}", val(f));
        let r: [isize; 2] = [4, 5];
        let rv = unsafe { AXValueCreate(4, r.as_ptr() as *const c_void) };
        println!("select 4..9: {}", unsafe {
            AXUIElementSetAttributeValue(f, s("AXSelectedTextRange"), rv)
        });
        std::thread::sleep(std::time::Duration::from_millis(200));
        println!("selected: {}", desc(get(f, "AXSelectedText").1));
        let units: Vec<u16> = "nicht".encode_utf16().collect();
        let keycode: u16 = if mode.contains("kc49") { 49 } else { 0 };
        for down in [true, false] {
            unsafe {
                let e = CGEventCreateKeyboardEvent(ptr::null(), keycode, down);
                CGEventSetFlags(e, 0);
                CGEventKeyboardSetUnicodeString(e, units.len(), units.as_ptr());
                if mode.contains("hid") {
                    CGEventPost(0, e)
                } else {
                    CGEventPostToPid(pid, e)
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        for ms in [50, 200, 600] {
            std::thread::sleep(std::time::Duration::from_millis(ms));
            println!("after {ms} ms: {}", val(f));
        }
    }
    if mode.contains("write") && err == 0 {
        let val = |f: R| desc(get(f, "AXValue").1);
        let mut pn: R = ptr::null();
        unsafe { AXUIElementCopyParameterizedAttributeNames(f, &mut pn) };
        println!("parameterized: {}", desc_n(pn, 4000).replace('\n', " "));
        for a in ["AXValue", "AXSelectedText", "AXSelectedTextRange"] {
            let mut st = 0u8;
            let e = unsafe { AXUIElementIsAttributeSettable(f, s(a), &mut st) };
            println!("settable {a}: err {e} {st}");
        }
        println!("before: {}", val(f));
        let r: [isize; 2] = [0, 3];
        let rv = unsafe { AXValueCreate(4, r.as_ptr() as *const c_void) };
        println!("select 0..3: {}", unsafe {
            AXUIElementSetAttributeValue(f, s("AXSelectedTextRange"), rv)
        });
        std::thread::sleep(std::time::Duration::from_millis(100));
        println!("selected: {}", desc(get(f, "AXSelectedText").1));
        println!("set AXSelectedText: {}", unsafe {
            AXUIElementSetAttributeValue(f, s("AXSelectedText"), s("DAS"))
        });
        std::thread::sleep(std::time::Duration::from_millis(300));
        println!("after AXSelectedText: {}", val(f));
        if mode.contains("value") {
            println!("set AXValue: {}", unsafe {
                AXUIElementSetAttributeValue(f, s("AXValue"), s("das nicht mein ding "))
            });
            std::thread::sleep(std::time::Duration::from_millis(300));
            println!("after AXValue: {}", val(f));
        }
    }
    let (err, w) = get(app, "AXFocusedWindow");
    println!("app AXFocusedWindow err {err}");
    if err == 0 {
        show("  window", w);
        println!("  title: {}", desc(get(w, "AXTitle").1));
        let (err, kids) = get(w, "AXChildren");
        if err == 0 {
            println!("  window children {}", unsafe { CFArrayGetCount(kids) });
        }
        if mode.contains("walk") {
            let mut b = 3000;
            walk(w, 0, &mut b);
            println!("walked {} elements", 3000 - b);
        }
    }
    let (err, m) = get(app, "AXMainWindow");
    println!("app AXMainWindow err {err}");
    if err == 0 {
        let _ = m;
    }
    let sys = unsafe { AXUIElementCreateSystemWide() };
    let (err, f) = get(sys, "AXFocusedUIElement");
    println!("system-wide AXFocusedUIElement err {err}");
    if err == 0 {
        show("  focused", f)
    }
    let (err, fa) = get(sys, "AXFocusedApplication");
    println!("system-wide AXFocusedApplication err {err}");
    if err == 0 {
        show("  app", fa)
    }
}
