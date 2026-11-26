//! Runs autocorrect against a real app's focused text field, for testing on a Mac.
//! `live <pid> <bundle id> <seconds> <out file>`, started as the app bundle "Sokki Autocorrect
//! Lab" (`scripts/autocorrect-lab.sh`) so macOS checks its own Accessibility grant, not the
//! shell's. It has no keyboard hook, so taking its grant away can't freeze input.
//! Counts a "key" for every character the field grows by (the test page types by itself),
//! so it needs no keyboard hook. Prints only what happened and the field's text, which in
//! a test is the test page's made-up sentence.
use std::cell::Cell;
use std::ffi::c_void;
use std::io::Write;
use std::path::Path;
use std::time::{Duration, Instant};

use sokki_autocorrect::{mac, Autocorrect, Field, Outcome};

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrustedWithOptions(options: *const c_void) -> u8;
    static kAXTrustedCheckOptionPrompt: *const c_void;
}
#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    static kCFBooleanTrue: *const c_void;
    fn CFDictionaryCreate(
        a: *const c_void,
        k: *const *const c_void,
        v: *const *const c_void,
        n: isize,
        kc: *const c_void,
        vc: *const c_void,
    ) -> *const c_void;
}

/// Lists the lab in System Settings › Accessibility (switched off) so Robin can switch it on.
fn ask_for_access() {
    unsafe {
        let k = [kAXTrustedCheckOptionPrompt];
        let v = [kCFBooleanTrue];
        let d = CFDictionaryCreate(
            std::ptr::null(),
            k.as_ptr(),
            v.as_ptr(),
            1,
            std::ptr::null(),
            std::ptr::null(),
        );
        AXIsProcessTrustedWithOptions(d);
    }
}

macro_rules! say {
    ($out:expr, $($t:tt)*) => {{ let _ = writeln!($out, $($t)*); let _ = $out.flush(); }};
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.get(1).map(|s| s.as_str()) == Some("explore") {
        // the explorer, started from this bundle so it runs with the lab's grant
        let exe = std::env::current_exe().unwrap().with_file_name("explore");
        let o = std::process::Command::new(exe)
            .arg(&a[2])
            .arg(&a[3])
            .output()
            .unwrap();
        std::fs::write(&a[4], [o.stdout, o.stderr].concat()).unwrap();
        return;
    }
    let mut out = std::fs::File::create(&a[4]).unwrap();
    let pid: i32 = a[1].parse().unwrap();
    let app = a[2].clone();
    let secs: u64 = a.get(3).and_then(|s| s.parse().ok()).unwrap_or(30);
    say!(out, "trusted: {}", mac::trusted());
    if !mac::trusted() {
        if pid == 0 {
            ask_for_access();
            // stay until granted, so the prompt stays up
            for _ in 0..300 {
                std::thread::sleep(Duration::from_secs(1));
                if mac::trusted() {
                    say!(out, "trusted: true");
                    break;
                }
            }
        }
        return;
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    if pid == 0 {
        return;
    }
    let typos = sokki_typos::Typos::load(
        &root.join("mobile/keyboard/src/main/assets/sokki"),
        &root.join("crates/sokki-typos/data"),
    )
    .unwrap();
    let mut ac = Autocorrect::new(typos, None);
    // as Sokki's thread does about once a second
    mac::prepare(Some((pid, app.clone())), &[]);
    if std::env::var("LAB_FRONT").is_ok() || a.get(5).map(|s| s.as_str()) == Some("front") {
        say!(out, "brought to front: {}", mac::bring_to_front(pid));
    }
    let keys = Cell::new(0u64);
    let mut last_len = 0usize;
    let mut seen_keys = 0u64;
    let mut last_desc = String::new();
    let mut last_text = String::new();
    let end = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < end {
        std::thread::sleep(Duration::from_millis(15));
        let Some(field) = mac::focused(Some((pid, app.clone())), &[]) else {
            let d = mac::describe(pid);
            if d != last_desc {
                say!(out, "no field: {d}");
                last_desc = d;
            }
            ac.reset();
            continue;
        };
        let Some(v) = field.value() else {
            continue;
        };
        // A "key" for every character the page types, counted when the page types it (the
        // page's own clock), like Sokki's hook, not when Accessibility shows it.
        let n = v.encode_utf16().count();
        if n > last_len {
            keys.set(keys.get() + (n - last_len) as u64);
        }
        last_len = n;
        if keys.get() == seen_keys && !ac.stale() {
            if v != last_text {
                say!(out, "text: {v:?} sel {:?}", field.selection());
                last_text = v;
            }
            continue;
        }
        seen_keys = keys.get();
        let front = mac::is_front(pid);
        let o = ac.on_key(&field, &|| keys.get());
        if o != Outcome::Nothing && !front {
            say!(out, "(test window was not in front)");
        }
        if o != Outcome::Nothing {
            say!(out, "{o:?}");
        }
        if v != last_text {
            say!(out, "text: {v:?} sel {:?}", field.selection());
            last_text = v;
        }
    }
    say!(
        out,
        "final: {} front {}",
        mac::describe(pid),
        mac::is_front(pid)
    );
}
