//! Night test: fast typing while autocorrect swaps a word, in a real app (Robin, 27 Nov 2026:
//! "I type, it corrects, but I've already typed the next letter, and then it goes wrong").
//! `race <pid> <bundle id> <out file> <hold|nohold> <keys per second> <runs>`, started from the
//! lab bundle (`scripts/autocorrect-lab.sh race`), so it runs with the lab's Accessibility grant.
//!
//! Unlike `live`, it runs Sokki's keyboard hook (`sokki_keys`) in this process, as Sokki does,
//! and types real key events at the HID level, so they pass the hook. `nohold` makes the field
//! refuse to hold keys: Sokki before 27 Nov 2026. Each run types a made-up sentence with two
//! typos into the app's focused field (cleared with ⌘A, Delete first) and reads it back.
//! The hook holds keys at most 300 ms; it is gone when this process exits.
//! Posts keys into the front window: run ONLY when nobody uses the Mac.
use std::ffi::c_void;
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use sokki_autocorrect::{mac, Autocorrect, Field, Held, Outcome};

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn CGEventCreateKeyboardEvent(source: *const c_void, key: u16, down: bool) -> *const c_void;
    fn CGEventKeyboardSetUnicodeString(e: *const c_void, n: usize, s: *const u16);
    fn CGEventSetFlags(e: *const c_void, flags: u64);
    fn CGEventPost(tap: u32, e: *const c_void);
}
#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFRelease(cf: *const c_void);
}

const TYPED: &str = "Das ist nihct gut und wroks nicht so ";
const OK: [&str; 4] = [
    "Das ist nicht gut und works nicht so ",
    "Das ist nihct gut und works nicht so ",
    "Das ist nicht gut und wroks nicht so ",
    "Das ist nihct gut und wroks nicht so ",
];

/// One key press and release at the HID level, as a keyboard sends it.
fn key(code: u16, text: Option<char>, flags: u64) {
    for down in [true, false] {
        unsafe {
            let e = CGEventCreateKeyboardEvent(std::ptr::null(), code, down);
            if e.is_null() {
                continue;
            }
            CGEventSetFlags(e, flags);
            if let Some(c) = text {
                let mut b = [0u16; 2];
                let u = c.encode_utf16(&mut b);
                CGEventKeyboardSetUnicodeString(e, u.len(), u.as_ptr());
            }
            CGEventPost(0, e); // kCGHIDEventTap
            CFRelease(e);
        }
        std::thread::sleep(Duration::from_millis(4));
    }
}

/// The field, with or without the hook holding keys.
struct Tested<'a> {
    f: &'a mac::AxField,
    hold: bool,
}
impl Field for Tested<'_> {
    fn app(&self) -> &str {
        self.f.app()
    }
    fn value(&self) -> Option<String> {
        self.f.value()
    }
    fn selection(&self) -> Option<(usize, usize)> {
        self.f.selection()
    }
    fn replaceable(&self) -> bool {
        self.f.replaceable()
    }
    fn select(&self, s: usize, l: usize) -> bool {
        self.f.select(s, l)
    }
    fn insert(&self, t: &str) -> bool {
        self.f.insert(t)
    }
    fn settle_within(&self, ms: u64, ok: &dyn Fn(&str) -> bool) -> bool {
        self.f.settle_within(ms, ok)
    }
    fn hold_keys(&self) -> Option<Held> {
        if self.hold {
            self.f.hold_keys()
        } else {
            None
        }
    }
    fn release_keys(&self) {
        self.f.release_keys()
    }
}

macro_rules! say {
    ($out:expr, $($t:tt)*) => {{ let _ = writeln!($out, $($t)*); let _ = $out.flush(); }};
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let pid: i32 = a[1].parse().unwrap();
    let app = a[2].clone();
    let mut out = std::fs::File::create(&a[3]).unwrap();
    let hold = a[4] == "hold";
    let per_s: f64 = a[5].parse().unwrap();
    let runs: usize = a[6].parse().unwrap();
    say!(out, "trusted: {}", mac::trusted());
    if !mac::trusted() {
        return;
    }
    let _hook = match sokki_keys::KeyboardListener::new() {
        Ok(h) => h,
        Err(e) => {
            say!(out, "no keyboard hook: {e}");
            return;
        }
    };
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let typos = sokki_typos::Typos::load(
        &root.join("mobile/keyboard/src/main/assets/sokki"),
        &root.join("crates/sokki-typos/data"),
    )
    .unwrap();
    let mut ac = Autocorrect::new(typos, None);
    mac::prepare(Some((pid, app.clone())), &[]);
    say!(out, "brought to front: {}", mac::bring_to_front(pid));
    // Chrome shows its fields to Accessibility about 2 s after it is asked
    std::thread::sleep(Duration::from_secs(8));
    let (mut wrong, mut fixed) = (0, 0);
    let mut seed = 0x9e37_79b9_7f4a_7c15u64;
    for run in 0..runs {
        if !mac::is_front(pid) {
            say!(out, "run {run}: the app is not in front, stopping");
            break;
        }
        // clear the field: ⌘A, Delete
        key(0, Some('a'), 1 << 20);
        key(51, None, 0);
        std::thread::sleep(Duration::from_millis(300));
        let typing = Arc::new(AtomicBool::new(true));
        let gaps: Vec<f64> = TYPED
            .chars()
            .map(|_| {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                (1000.0 / per_s) * (0.5 + (seed % 1000) as f64 / 1000.0)
            })
            .collect();
        let t = typing.clone();
        let typist = std::thread::spawn(move || {
            for (c, gap) in TYPED.chars().zip(gaps) {
                std::thread::sleep(Duration::from_micros((gap * 1000.0) as u64));
                key(if c == ' ' { 49 } else { 0 }, Some(c), 0);
            }
            t.store(false, Ordering::SeqCst);
        });
        // Sokki's thread, as runner.rs: look every 15 ms after a key
        let mut seen = sokki_keys::key_downs();
        let mut key_at = Instant::now();
        let mut ms: Vec<u128> = Vec::new();
        let mut done_at: Option<Instant> = None;
        loop {
            std::thread::sleep(Duration::from_millis(15));
            if !typing.load(Ordering::SeqCst) {
                let d = *done_at.get_or_insert_with(Instant::now);
                if d.elapsed() > Duration::from_millis(800) {
                    break;
                }
            }
            let now = sokki_keys::key_downs();
            if now == seen && !(ac.stale() && key_at.elapsed() < Duration::from_millis(300)) {
                continue;
            }
            if now != seen {
                seen = now;
                key_at = Instant::now();
            }
            let Some(f) = mac::focused(Some((pid, app.clone())), &[]) else {
                ac.reset();
                continue;
            };
            let started = Instant::now();
            let o = ac.on_key(&Tested { f: &f, hold }, &sokki_keys::key_downs);
            match &o {
                Outcome::Fixed { .. } => {
                    fixed += 1;
                    ms.push(started.elapsed().as_millis());
                }
                Outcome::Nothing => {}
                o => say!(out, "run {run}: {o:?}"),
            }
        }
        let _ = typist.join();
        let text = mac::focused(Some((pid, app.clone())), &[])
            .and_then(|f| f.value())
            .unwrap_or_default()
            .replace('\u{a0}', " ");
        let ok = OK.contains(&text.as_str());
        wrong += usize::from(!ok);
        say!(
            out,
            "run {run}: {} {text:?} fix ms {ms:?}",
            if ok { "ok" } else { "WRONG" }
        );
    }
    say!(
        out,
        "result: {} {per_s}/s, {runs} runs, {wrong} wrong, {fixed} of {} typos fixed",
        if hold { "hold" } else { "nohold" },
        runs * 2
    );
}
