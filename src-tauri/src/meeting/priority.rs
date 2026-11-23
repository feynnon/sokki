//! Recording wins. Robin, 2026-11-23: "Meeting recording should always be
//! able to start immediately, even after having finished the old one.
//! Prioritise recording, always."
//!
//! Starting a meeting never waits for processing: they share no lock, and
//! the captures run on Core Audio's real-time threads. What processing an
//! earlier meeting can still do is compete for the CPU, the GPU and the
//! disk while the new one records. So while a meeting records:
//! - the processing thread runs in macOS's "background" class (efficiency
//!   cores, throttled disk), switched at every step and chunk;
//! - the diarization worker process is put in the background band too;
//! - the summary waits: the note is saved without it, and the background
//!   retry adds it once nothing records (`process::retry_summaries`).
//!
//! The live transcript of the new meeting shares the speech engine with the
//! old meeting's transcription, so its first pieces may come later.

use std::cell::Cell;
use std::sync::atomic::{AtomicBool, Ordering};

/// A meeting is recording. Set by `MeetingManager::start`/`stop`.
static RECORDING: AtomicBool = AtomicBool::new(false);

pub fn set_recording(on: bool) {
    RECORDING.store(on, Ordering::Relaxed);
}

pub fn recording() -> bool {
    RECORDING.load(Ordering::Relaxed) && !crate::test_mic::no_yield()
}

/// Scheduling class of a thread that does work nobody waits on right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    /// The thread's normal class.
    Normal,
    /// Below everything the person is doing: a meeting records.
    Background,
}

/// The class processing should run in.
pub fn class_for(recording: bool) -> Class {
    if recording {
        Class::Background
    } else {
        Class::Normal
    }
}

thread_local! {
    /// The class this thread is in, for threads that follow the recording
    /// (`None`: this thread doesn't).
    static FOLLOWING: Cell<Option<Class>> = const { Cell::new(None) };
}

/// The calling thread (processing a meeting) yields to recordings from now
/// on, starting now. Other threads calling [`yield_now`] are left alone, so
/// the live transcript and dictation keep their own priority.
pub fn follow_recording() {
    let wanted = class_for(recording());
    set_thread_class(wanted);
    FOLLOWING.with(|c| c.set(Some(wanted)));
}

/// At a step or chunk of processing: drops the calling thread to the
/// background while a meeting records, and back once it stops. Does nothing
/// on threads that did not call [`follow_recording`].
pub fn yield_now() {
    FOLLOWING.with(|c| {
        let Some(current) = c.get() else { return };
        let wanted = class_for(recording());
        if wanted != current {
            set_thread_class(wanted);
            c.set(Some(wanted));
        }
    });
}

#[cfg(target_os = "macos")]
fn set_thread_class(class: Class) {
    let qos = match class {
        Class::Normal => libc::qos_class_t::QOS_CLASS_DEFAULT,
        Class::Background => libc::qos_class_t::QOS_CLASS_BACKGROUND,
    };
    // SAFETY: changes only the calling thread's own scheduling class.
    let r = unsafe { libc::pthread_set_qos_class_self_np(qos, 0) };
    if r != 0 {
        log::debug!("Processing priority not changed (error {r})");
    }
}

#[cfg(not(target_os = "macos"))]
fn set_thread_class(_class: Class) {}

/// Puts a worker process (the diarizer) in the background band while a
/// meeting records and takes it out after. `state` is the band it is in;
/// call this from the loop that waits for the child.
pub fn follow_recording_child(pid: u32, state: &mut Class) {
    let wanted = class_for(recording());
    if wanted != *state && set_process_class(pid, wanted) {
        *state = wanted;
    }
}

#[cfg(target_os = "macos")]
fn set_process_class(pid: u32, class: Class) -> bool {
    let band = match class {
        Class::Normal => 0,
        Class::Background => libc::PRIO_DARWIN_BG,
    };
    // SAFETY: plain syscall on our own child's id.
    let r = unsafe { libc::setpriority(libc::PRIO_DARWIN_PROCESS, pid as libc::id_t, band) };
    r == 0
}

#[cfg(not(target_os = "macos"))]
fn set_process_class(_pid: u32, _class: Class) -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn processing_goes_to_the_background_while_recording() {
        assert_eq!(class_for(true), Class::Background);
        assert_eq!(class_for(false), Class::Normal);
    }

    #[cfg(target_os = "macos")]
    const BACKGROUND: u32 = libc::qos_class_t::QOS_CLASS_BACKGROUND as u32;
    #[cfg(target_os = "macos")]
    const DEFAULT: u32 = libc::qos_class_t::QOS_CLASS_DEFAULT as u32;

    /// The calling thread's class, as its number.
    #[cfg(target_os = "macos")]
    fn qos_self() -> u32 {
        let mut qos = libc::qos_class_t::QOS_CLASS_UNSPECIFIED;
        let mut rel = 0;
        // SAFETY: reads the calling thread's own class.
        unsafe { libc::pthread_get_qos_class_np(libc::pthread_self(), &mut qos, &mut rel) };
        qos as u32
    }

    /// One test for everything that reads the global flag, so parallel
    /// tests can't flip it under each other.
    #[cfg(target_os = "macos")]
    #[test]
    fn a_following_thread_and_child_switch_with_the_recording() {
        // A thread that follows: background while recording, back after.
        let classes = std::thread::spawn(|| {
            follow_recording();
            let before = qos_self();
            set_recording(true);
            yield_now();
            let during = qos_self();
            set_recording(false);
            yield_now();
            (before, during, qos_self())
        })
        .join()
        .unwrap();
        assert_eq!(classes.1, BACKGROUND);
        assert_eq!(classes.2, DEFAULT);
        assert_eq!(classes.0, DEFAULT);

        // A thread that doesn't follow keeps its class.
        let other = std::thread::spawn(|| {
            set_recording(true);
            yield_now();
            let q = qos_self();
            set_recording(false);
            q
        })
        .join()
        .unwrap();
        assert_ne!(other, BACKGROUND);

        // A child process goes into the background band and out again.
        let mut child = std::process::Command::new("/bin/sleep")
            .arg("5")
            .spawn()
            .unwrap();
        let pid = child.id();
        // Its scheduling priority: 31 normally, 4 in the background band
        // (getpriority doesn't report the band for another process).
        let band = || {
            let out = std::process::Command::new("/bin/ps")
                .args(["-o", "pri=", "-p", &pid.to_string()])
                .output()
                .unwrap();
            String::from_utf8_lossy(&out.stdout).trim().parse::<i32>().unwrap()
        };
        let before = band();
        let mut state = Class::Normal;
        set_recording(true);
        follow_recording_child(pid, &mut state);
        assert_eq!(state, Class::Background);
        let during = band();
        set_recording(false);
        follow_recording_child(pid, &mut state);
        assert_eq!(state, Class::Normal);
        let after = band();
        let _ = child.kill();
        let _ = child.wait();
        assert!(during < before, "not in the background band: {before} → {during}");
        assert_eq!(after, before);
    }
}
