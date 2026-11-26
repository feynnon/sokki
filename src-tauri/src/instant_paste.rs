//! A cleaned-up dictation appears in the text field right after the stop, and its last part
//! is cleaned in place about two seconds later (Robin, 26.11.2026: "Text erscheint SOFORT …
//! und wird dann nochmal nach 2,4 s edited").
//!
//! 1. At the stop, with the dictation cleanup on, Sokki checks that it can read and select
//!    text in the focused field (Accessibility). If not, it pastes once, cleaned, as before.
//! 2. Right after the second transcription the cleanup hands over its draft
//!    (`sokki_engine::cleanup::Draft`): the paragraphs cleaned while speaking, then the rest
//!    as transcribed. That is pasted at once.
//! 3. When the last cleanup step is done, only the end that changed is replaced
//!    (`sokki_autocorrect::tail`): selected through Accessibility and pasted over. Only while
//!    the same app is in front and its field still ends with exactly the draft at the cursor,
//!    with no key pressed since. Otherwise the field is left as it is; the history has the
//!    cleaned text.
//!
//! Between the paste and the edit the live panel shrinks to its pill ("Verarbeite …").
//! Undo in the app: the first Cmd+Z brings the draft back, the second removes the paste.
//! In test mode nothing is pasted; with `SOKKI_TEST_INSTANT_PASTE=1` the draft and the edit
//! are logged with their times after the stop.

use log::{info, warn};
use sokki_engine::cleanup::Draft;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};
use tauri::AppHandle;

/// How long the edit waits for the draft's paste to finish.
const PASTE_WAIT: Duration = Duration::from_secs(3);

enum State {
    /// No draft yet.
    Waiting,
    /// The draft is being pasted.
    Pasting,
    Pasted(Pasted),
    /// The draft's paste failed: the cleaned text is pasted as before.
    Failed,
}

struct Pasted {
    /// Exactly what was pasted, with the trailing space if the setting adds one.
    text: String,
    trailing_space: bool,
    /// The app it went into: (pid, bundle id).
    front: Option<(i32, String)>,
    /// Key presses counted right after the paste.
    keys: u64,
}

/// One dictation's instant paste.
#[derive(Clone)]
pub struct InstantPaste {
    app: AppHandle,
    stop: Instant,
    state: Arc<(Mutex<State>, Condvar)>,
}

impl InstantPaste {
    /// An instant paste for the dictation that stopped at `stop`, if Sokki can edit the
    /// focused field afterwards; `None` means paste once, as before.
    pub fn at_stop(app: &AppHandle, stop: Instant) -> Option<InstantPaste> {
        if let Err(why) = eligible(app) {
            info!("Instant paste: not here ({why}); pasting once, cleaned");
            return None;
        }
        Some(InstantPaste {
            app: app.clone(),
            stop,
            state: Arc::new((Mutex::new(State::Waiting), Condvar::new())),
        })
    }

    fn set(&self, s: State) {
        let (m, c) = &*self.state;
        *m.lock().unwrap_or_else(|e| e.into_inner()) = s;
        c.notify_all();
    }

    /// For the cleanup's `on_draft`: pastes the draft on a thread of its own, so the last
    /// cleanup steps start at once.
    pub fn on_draft(&self) -> Box<dyn FnOnce(Draft) + Send> {
        let me = self.clone();
        Box::new(move |draft: Draft| {
            me.set(State::Pasting);
            let spawned = std::thread::Builder::new()
                .name("instant-paste".into())
                .spawn({
                    let me = me.clone();
                    move || me.paste_draft(draft.text())
                });
            if spawned.is_err() {
                me.set(State::Failed);
            }
        })
    }

    fn paste_draft(&self, text: String) {
        let settings = crate::settings::get_settings(&self.app);
        let trailing_space = settings.append_trailing_space;
        let front = front();
        let pasted = if crate::test_mic::active() {
            info!(
                "Test mode, first paste ({:?} after the stop): {text}",
                self.stop.elapsed()
            );
            Ok(crate::utils::PasteOutcome::Pasted)
        } else {
            crate::utils::paste(text.clone(), self.app.clone())
        };
        match pasted {
            Ok(crate::utils::PasteOutcome::Pasted) => {
                // The text is in the field: the panel shrinks to its pill
                // ("Verarbeite …") until the end is cleaned, so it covers
                // nothing (Robin chose this of four versions, 26.11.2026).
                use tauri_specta::Event;
                let _ = crate::managers::transcription::StreamTextEvent {
                    cleaned: String::new(),
                    committed: String::new(),
                    tentative: String::new(),
                    cleanup: true,
                }
                .emit(&self.app);
                // The paste's own Cmd+V may still reach the key counter.
                std::thread::sleep(Duration::from_millis(60));
                info!(
                    "Instant paste: draft pasted {} ms after the stop ({} chars)",
                    self.stop.elapsed().as_millis(),
                    text.chars().count()
                );
                self.set(State::Pasted(Pasted {
                    text: if trailing_space {
                        format!("{text} ")
                    } else {
                        text
                    },
                    trailing_space,
                    front,
                    keys: sokki_keys::key_downs(),
                }));
            }
            other => {
                warn!("Instant paste: the draft's paste failed ({other:?}); pasting once");
                self.set(State::Failed);
            }
        }
    }

    /// After the cleanup: `Some(final_text)` when the caller still has to paste it (no
    /// draft was pasted), `None` when the draft is in the field (and edited if possible).
    /// `cleaned`: the cleaned dictation, `None` when the cleanup failed.
    pub fn finish(&self, final_text: String, cleaned: Option<&str>) -> Option<String> {
        let (m, c) = &*self.state;
        let mut s = m.lock().unwrap_or_else(|e| e.into_inner());
        let until = Instant::now() + PASTE_WAIT;
        while matches!(*s, State::Pasting) && Instant::now() < until {
            s = c
                .wait_timeout(s, until.saturating_duration_since(Instant::now()))
                .map(|r| r.0)
                .unwrap_or_else(|e| e.into_inner().0);
        }
        let pasted = match std::mem::replace(&mut *s, State::Waiting) {
            State::Pasted(p) => p,
            State::Waiting | State::Failed => return Some(final_text),
            State::Pasting => {
                warn!("Instant paste: the draft's paste didn't finish; leaving the field");
                return None;
            }
        };
        drop(s);
        let Some(cleaned) = cleaned else {
            info!("Instant paste: the cleanup failed; the draft stays as pasted");
            return None;
        };
        let wanted = if pasted.trailing_space {
            format!("{cleaned} ")
        } else {
            cleaned.to_string()
        };
        let t0 = Instant::now();
        let outcome = self.edit(&pasted, &wanted);
        info!(
            "Instant paste: {} {} ms after the stop ({} ms)",
            describe(&outcome),
            self.stop.elapsed().as_millis(),
            t0.elapsed().as_millis()
        );
        None
    }

    fn edit(&self, pasted: &Pasted, wanted: &str) -> sokki_autocorrect::tail::TailEdit {
        use sokki_autocorrect::tail::TailEdit;
        if crate::test_mic::active() {
            info!(
                "Test mode, edited ({:?} after the stop): {wanted}",
                self.stop.elapsed()
            );
            return if sokki_autocorrect::tail::same_text(&pasted.text, wanted) {
                TailEdit::Same
            } else {
                TailEdit::Replaced {
                    exact: true,
                    chars: 0,
                }
            };
        }
        #[cfg(target_os = "macos")]
        {
            let now = front();
            if now.as_ref().map(|f| f.0) != pasted.front.as_ref().map(|f| f.0) {
                return TailEdit::Skipped("another app in front");
            }
            let Some(field) = sokki_autocorrect::mac::pasted_into(now) else {
                return TailEdit::Skipped("no text field");
            };
            let app = self.app.clone();
            sokki_autocorrect::tail::replace_tail(
                &field,
                &pasted.text,
                wanted,
                &sokki_keys::key_downs,
                pasted.keys,
                &|s| crate::clipboard::paste_plain(s.to_string(), app.clone()).is_ok(),
            )
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = (pasted, wanted);
            TailEdit::Skipped("not on this system")
        }
    }
}

fn describe(o: &sokki_autocorrect::tail::TailEdit) -> String {
    use sokki_autocorrect::tail::TailEdit;
    match o {
        TailEdit::Same => "nothing to edit, the cleanup kept the draft".into(),
        TailEdit::Replaced { exact: true, chars } => format!("edited the end ({chars} chars)"),
        TailEdit::Replaced {
            exact: false,
            chars,
        } => {
            format!("edited the end ({chars} chars), but the field doesn't hold what was meant")
        }
        TailEdit::Skipped(why) => format!("left the draft: {why}"),
    }
}

fn front() -> Option<(i32, String)> {
    #[cfg(target_os = "macos")]
    {
        crate::autocorrect::front_app()
    }
    #[cfg(not(target_os = "macos"))]
    None
}

/// Wakes the app in front so its text field is readable at the stop (Chrome needs about 2 s
/// the first time). Called when a dictation with the cleanup starts.
pub fn wake() {
    #[cfg(target_os = "macos")]
    if !crate::test_mic::active() && crate::accessibility::trusted() {
        let _ = std::thread::Builder::new()
            .name("instant-paste-wake".into())
            .spawn(|| sokki_autocorrect::mac::wake(front()));
    }
}

/// Why the dictation is pasted once instead, or Ok.
fn eligible(app: &AppHandle) -> Result<(), &'static str> {
    if crate::test_mic::active() {
        return match std::env::var("SOKKI_TEST_INSTANT_PASTE").as_deref() {
            Ok("1") => Ok(()),
            _ => Err("test mode"),
        };
    }
    let s = crate::settings::get_settings(app);
    if s.paste_method != crate::settings::PasteMethod::CtrlV {
        return Err("paste method");
    }
    if s.auto_submit {
        return Err("auto-submit is on");
    }
    if crate::transcription_hook::present(app) {
        return Err("a transcription hook changes the text");
    }
    #[cfg(target_os = "macos")]
    {
        use sokki_autocorrect::Field;
        if !crate::accessibility::trusted() {
            return Err("no Accessibility access");
        }
        let Some(field) = sokki_autocorrect::mac::pasted_into(front()) else {
            return Err("no readable text field in front");
        };
        if field.value().is_none() || field.selection().is_none() {
            return Err("the field's text isn't readable");
        }
        if !field.replaceable() {
            return Err("the field's selection can't be set");
        }
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    Err("not on this system")
}
