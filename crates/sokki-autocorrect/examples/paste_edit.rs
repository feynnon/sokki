//! Lab test of the instant paste's edit (`sokki_autocorrect::tail`) in a real app's focused
//! text field: paste a made-up draft, wait as long as the cleanup's last step takes, replace
//! its end, then press Cmd+Z twice and look at what comes back.
//! `paste_edit <pid> <bundle id> <wait ms> <out file>`, started from the lab bundle
//! (`scripts/autocorrect-lab.sh paste-edit …`) so it runs with the lab's Accessibility grant.
//! Needs the app's window in front: run it only when nobody uses the Mac (the night run).
//! Prints only what happened and the field's made-up text.
use std::io::Write;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use sokki_autocorrect::tail::{replace_tail, same_text, TailEdit};
use sokki_autocorrect::{mac, Field};

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGEventCreateKeyboardEvent(source: *const u8, key: u16, down: bool) -> *const u8;
    fn CGEventSetFlags(e: *const u8, flags: u64);
    fn CGEventPostToPid(pid: i32, e: *const u8);
}
#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFRelease(cf: *const u8);
}

const CMD: u64 = 0x0010_0000;
const KEY_V: u16 = 9;
const KEY_Z: u16 = 6;

const DRAFT: &str = "Hi Lea, kurz zum Samstag: Wir treffen uns um vier bei mir im Garten.\n\nalso ähm für die party bräuchten wir noch den grill den bringt tom mit und getränke besorg ich";
const CLEANED: &str = "Hi Lea, kurz zum Samstag: Wir treffen uns um vier bei mir im Garten.\n\nFür die Party bräuchten wir noch:\n- den Grill, den bringt Tom mit\n- Getränke, die besorge ich.";

fn cmd_key(pid: i32, key: u16) {
    for down in [true, false] {
        unsafe {
            let e = CGEventCreateKeyboardEvent(std::ptr::null(), key, down);
            CGEventSetFlags(e, CMD);
            CGEventPostToPid(pid, e);
            CFRelease(e);
        }
    }
}

fn copy(text: &str) -> bool {
    let Ok(mut c) = Command::new("pbcopy").stdin(Stdio::piped()).spawn() else {
        return false;
    };
    let ok = c
        .stdin
        .take()
        .map(|mut i| i.write_all(text.as_bytes()).is_ok())
        .unwrap_or(false);
    ok && c.wait().map(|s| s.success()).unwrap_or(false)
}

fn paste(pid: i32, text: &str) -> bool {
    let ok = copy(text);
    std::thread::sleep(Duration::from_millis(40));
    cmd_key(pid, KEY_V);
    ok
}

/// Waits up to 1 s until the field's text passes `ok`.
fn wait_for(field: &dyn Field, ok: &dyn Fn(&str) -> bool) -> bool {
    let until = Instant::now() + Duration::from_secs(1);
    while Instant::now() < until {
        if field.value().is_some_and(|v| ok(&v)) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    false
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let pid: i32 = a[1].parse().unwrap();
    let app = a[2].clone();
    let wait_ms: u64 = a[3].parse().unwrap_or(2000);
    let mut out = std::fs::File::create(&a[4]).unwrap();
    let mut say = |s: String| {
        let _ = writeln!(out, "{s}");
        let _ = out.flush();
    };
    say(format!("trusted {}", mac::trusted()));
    mac::wake(Some((pid, app.clone())));
    std::thread::sleep(Duration::from_millis(2500));
    let front = || Some((pid, app.clone()));
    let Some(field) = mac::pasted_into(front()) else {
        say(format!("RESULT no field: {}", mac::describe(pid)));
        return;
    };
    let before = field.value().unwrap_or_default();
    say(format!(
        "field: {} chars before, selection {:?}, replaceable {}",
        before.chars().count(),
        field.selection(),
        field.replaceable()
    ));
    let t0 = Instant::now();
    paste(pid, DRAFT);
    let draft_in = wait_for(&field, &|v| same_text(v, &format!("{before}{DRAFT}")));
    say(format!(
        "draft pasted: {draft_in} after {} ms",
        t0.elapsed().as_millis()
    ));
    std::thread::sleep(Duration::from_millis(wait_ms));
    let t1 = Instant::now();
    let r = replace_tail(&field, DRAFT, CLEANED, &|| 0, 0, &|s| paste(pid, s));
    say(format!("edit: {r:?} in {} ms", t1.elapsed().as_millis()));
    let now = field.value().unwrap_or_default();
    let cleaned_in = same_text(&now, &format!("{before}{CLEANED}"));
    say(format!(
        "field holds the cleaned text: {cleaned_in}; text: {now:?}"
    ));
    // Undo: the first Cmd+Z should bring the draft back, the second the field before.
    cmd_key(pid, KEY_Z);
    let undo1 = wait_for(&field, &|v| same_text(v, &format!("{before}{DRAFT}")));
    let after1 = field.value().unwrap_or_default();
    cmd_key(pid, KEY_Z);
    let undo2 = wait_for(&field, &|v| same_text(v, &before));
    let after2 = field.value().unwrap_or_default();
    say(format!(
        "undo 1 brings the draft back: {undo1}; text: {after1:?}"
    ));
    say(format!(
        "undo 2 brings the field back: {undo2}; text: {after2:?}"
    ));
    say(format!(
        "RESULT app={app} draft={draft_in} edit={} exact={cleaned_in} undo1={undo1} undo2={undo2}",
        match r {
            TailEdit::Replaced { .. } => "replaced",
            TailEdit::Same => "same",
            TailEdit::Skipped(w) => w,
        }
    ));
}
