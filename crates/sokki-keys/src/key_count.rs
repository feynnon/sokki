//! How many keys went down since the hook started, for "Keep what I write"
//! (`sokki-journal`): a change tells it the focused text field may have
//! changed, so it reads the field soon. It never learns which key.
//!
//! The hook's only work for this is one relaxed atomic add, so it can't
//! block or slow a key event, whatever the reader does.

use std::sync::atomic::{AtomicU64, Ordering};

static KEY_DOWNS: AtomicU64 = AtomicU64::new(0);

/// Called by the platform hook for each key press (not for modifiers).
#[inline]
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn note_key_down() {
    KEY_DOWNS.fetch_add(1, Ordering::Relaxed);
}

/// Key presses seen so far. Only changes in this number mean anything.
pub fn key_downs() -> u64 {
    KEY_DOWNS.load(Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[test]
    fn counting_a_key_takes_no_time_and_never_waits() {
        let before = key_downs();
        let started = Instant::now();
        for _ in 0..100_000 {
            note_key_down();
        }
        // 100k presses in well under 50 ms: nothing here waits on a reader.
        assert!(
            started.elapsed().as_millis() < 50,
            "{:?}",
            started.elapsed()
        );
        assert!(key_downs() >= before + 100_000);
    }
}
