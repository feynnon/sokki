//! Reading the focused text field through Accessibility.
//!
//! Never read: password fields (`AXSecureTextField`), fields an app marks as
//! protected (`AXProtectedContent`), anything while macOS hides keys from apps
//! (secure event input: password managers, sudo), Sokki itself, and the
//! excluded apps. Every Accessibility call waits at most `TIMEOUT_S` for the
//! app, so a hung app costs this thread a moment and nobody else anything.

use std::collections::HashSet;

use meeting_chat::ax::Element;
use objc2_app_kit::NSRunningApplication;

use crate::entry::Observation;

/// How long one Accessibility call may wait for an app.
const TIMEOUT_S: f32 = 0.25;

/// Longer texts (a whole document) are not a piece of writing to keep.
pub const MAX_CHARS: usize = 20_000;

#[link(name = "Carbon", kind = "framework")]
extern "C" {
    // Boolean is an unsigned char.
    fn IsSecureEventInputEnabled() -> u8;
}

/// Whether macOS hides typed keys from apps right now.
pub fn secure_input() -> bool {
    unsafe { IsSecureEventInputEnabled() != 0 }
}

/// Which field a read came from: the app and the element.
pub struct FieldKey {
    pid: i32,
    element: Element,
}

impl PartialEq for FieldKey {
    fn eq(&self, other: &Self) -> bool {
        self.pid == other.pid && self.element == other.element
    }
}

fn bundle_id(pid: i32) -> Option<String> {
    let app = NSRunningApplication::runningApplicationWithProcessIdentifier(pid)?;
    app.bundleIdentifier().map(|s| s.to_string())
}

fn is_text_field(e: &Element) -> bool {
    matches!(
        e.role().as_str(),
        "AXTextField" | "AXTextArea" | "AXComboBox"
    )
}

fn is_protected(e: &Element) -> bool {
    e.string("AXSubrole").as_deref() == Some("AXSecureTextField")
        || e.bool("AXProtectedContent") == Some(true)
}

/// Reads the focused field. Knows which apps it already asked to switch on
/// their Accessibility tree (Electron apps build it only when asked).
pub struct Reader {
    own_pid: i32,
    /// Test mode: read this app instead of the one in front.
    test_pid: Option<i32>,
    asked: HashSet<i32>,
    /// Test mode: the last reason a read found nothing, logged on change.
    last_reason: String,
}

impl Reader {
    pub fn new(own_pid: i32, test_pid: Option<i32>) -> Self {
        Reader {
            own_pid,
            test_pid,
            asked: HashSet::new(),
            last_reason: String::new(),
        }
    }

    /// The focused field, if Sokki may read it. In test mode, logs why not.
    pub fn read(&mut self, excluded: &[String]) -> Read {
        let read = self.read_inner(excluded);
        if self.test_pid.is_some() {
            let reason = match &read {
                Read::Field(o) => {
                    format!("a field of {} ({} chars)", o.app, o.text.chars().count())
                }
                Read::NoField => self.last_reason.clone(),
                Read::NoAnswer => "no answer".into(),
            };
            static LAST: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());
            if let Ok(mut last) = LAST.lock() {
                if *last != reason {
                    log::info!("journal test: read {reason}");
                    *last = reason;
                }
            }
        }
        read
    }

    fn no_field(&mut self, why: &str) -> Read {
        if self.test_pid.is_some() {
            self.last_reason = why.to_string();
        }
        Read::NoField
    }

    fn read_inner(&mut self, excluded: &[String]) -> Read {
        if secure_input() {
            return self.no_field("nothing: secure input is on");
        }
        let pid = match self.test_pid {
            Some(pid) => pid,
            None => match Element::frontmost_pid() {
                Some(pid) => pid,
                None => return Read::NoAnswer,
            },
        };
        if pid == self.own_pid {
            return self.no_field("nothing: Sokki itself");
        }
        let app_id = bundle_id(pid).unwrap_or_default();
        if excluded.iter().any(|x| x.eq_ignore_ascii_case(&app_id)) {
            return self.no_field(&format!("nothing: {app_id} is excluded"));
        }
        let app = Element::app_with_timeout(pid, TIMEOUT_S);
        let focused = match app.element("AXFocusedUIElement") {
            Some(f) if is_text_field(&f) => f,
            found => {
                // A busy or hung app answers nothing at all: that's no news,
                // not a field left.
                if found.is_none() && app.string("AXRole").is_none() {
                    return Read::NoAnswer;
                }
                // Electron (Slack, VS Code, …) shows its fields once a tool asks
                // with AXManualAccessibility, its documented switch. Asked once
                // per app; others ignore it. Chrome's own switch
                // (AXEnhancedUserInterface) upsets window managers: never set.
                if self.asked.insert(pid) {
                    let code = app.set_bool_code("AXManualAccessibility", true);
                    log::debug!("journal: AXManualAccessibility on {app_id}: {code}");
                }
                let why = match &found {
                    Some(f) => format!("nothing: focus on {} in {app_id}", f.role()),
                    None => format!("nothing: no focused element in {app_id}"),
                };
                return self.no_field(&why);
            }
        };
        if is_protected(&focused) {
            return self.no_field("nothing: a password field");
        }
        let Some(text) = focused.string("AXValue") else {
            return self.no_field("nothing: the field has no value");
        };
        if text.chars().count() > MAX_CHARS {
            return self.no_field("nothing: text too long");
        }
        let hint = ["AXPlaceholderValue", "AXDescription", "AXTitle"]
            .iter()
            .filter_map(|a| focused.string(a))
            .find(|s| !s.trim().is_empty())
            .unwrap_or_default();
        // read-only check: autocorrect selects a word and types over it
        let replaceable = Some(focused.settable("AXSelectedTextRange"));
        Read::Field(Observation {
            key: FieldKey {
                pid,
                element: focused,
            },
            app: app_id,
            hint,
            text,
            replaceable,
        })
    }
}

/// What one read found.
pub enum Read {
    Field(Observation<FieldKey>),
    /// No field Sokki may read: none focused, a password field, secure
    /// input, Sokki itself or an excluded app.
    NoField,
    /// The app didn't answer in time: no news.
    NoAnswer,
}

/// Whether the Mac's spell checker knows `word` in any of `languages`
/// ("de", "en", …; empty: the user's own languages). Names it doesn't know
/// are what Sokki may learn.
pub fn spell_known(word: &str, languages: &[String]) -> bool {
    use objc2_app_kit::NSSpellChecker;
    use objc2_foundation::NSString;
    let checker = NSSpellChecker::sharedSpellChecker();
    let text = NSString::from_str(word);
    let check = |lang: Option<&NSString>| {
        // SAFETY: a null word count is allowed.
        let miss = unsafe {
            checker.checkSpellingOfString_startingAt_language_wrap_inSpellDocumentWithTag_wordCount(
                &text,
                0,
                lang,
                false,
                0,
                std::ptr::null_mut(),
            )
        };
        miss.length == 0
    };
    if languages.is_empty() {
        return check(None);
    }
    languages
        .iter()
        .any(|l| check(Some(&NSString::from_str(l))))
}
