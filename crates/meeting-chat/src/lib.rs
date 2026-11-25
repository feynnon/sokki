//! meeting-chat: post one line into the chat of the meeting the person is
//! in, from their own account, and watch that chat for a "stop".
//!
//! No bot and no API: the meeting app's own chat field, driven through
//! macOS Accessibility (the permission Sokki already has for pasting).
//! Wispr Flow posts its recording notice the same way.
//!
//! # How a post works
//!
//! 1. Find the app's process and its meeting window (for a browser: the
//!    window whose active tab is Google Meet).
//! 2. Find the chat's text field; if the chat panel is closed, open it with
//!    its own button (or the app's chat shortcut) and look again.
//! 3. Bring the app to the front for a moment, put the text into the field
//!    (set as its value, else typed key by key), press Return, then give the
//!    front back to the app that had it.
//! 4. Check that the field emptied, and find the sent line in the chat. The
//!    chat region found that way is what [`Posted::stop_requested`] watches.
//!
//! Key presses go to the meeting app's process only (`CGEventPostToPid`), so
//! no keystroke can land in another app if the person switches meanwhile.
//! A field that already holds a draft is never touched.
//!
//! # Platforms
//!
//! macOS only. Elsewhere [`post`] returns [`PostError::Unsupported`].

#[cfg(target_os = "macos")]
mod ax;
#[cfg(target_os = "macos")]
mod keys;
#[cfg(target_os = "macos")]
mod mac;

use std::fmt;

/// Where the meeting runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// The Zoom app.
    Zoom,
    /// The Microsoft Teams app (new and classic).
    Teams,
    /// Google Meet in a Chromium browser; the bundle ids of that browser.
    Meet { browser: &'static str, bundle_ids: &'static [&'static str] },
}

impl Target {
    /// The target for a `meeting-detect` app id ("zoom", "chrome", …), if
    /// Sokki can post into that app's meeting chat.
    pub fn for_app(app_id: &str) -> Option<Target> {
        Some(match app_id {
            "zoom" => Target::Zoom,
            "teams" => Target::Teams,
            "chrome" => Target::Meet {
                browser: "Google Chrome",
                bundle_ids: &[
                    "com.google.Chrome",
                    "com.google.Chrome.beta",
                    "com.google.Chrome.dev",
                    "com.google.Chrome.canary",
                ],
            },
            "arc" => Target::Meet { browser: "Arc", bundle_ids: &["company.thebrowser.Browser"] },
            "edge" => Target::Meet {
                browser: "Microsoft Edge",
                bundle_ids: &[
                    "com.microsoft.edgemac",
                    "com.microsoft.edgemac.Beta",
                    "com.microsoft.edgemac.Dev",
                    "com.microsoft.edgemac.Canary",
                ],
            },
            "brave" => Target::Meet { browser: "Brave", bundle_ids: &["com.brave.Browser"] },
            _ => return None,
        })
    }

    /// The name people know the meeting by: "Zoom", "Teams", "Google Meet".
    pub fn name(&self) -> &'static str {
        match self {
            Target::Zoom => "Zoom",
            Target::Teams => "Teams",
            Target::Meet { .. } => "Google Meet",
        }
    }

    pub(crate) fn bundle_ids(&self) -> &'static [&'static str] {
        match self {
            Target::Zoom => &["us.zoom.xos"],
            Target::Teams => &["com.microsoft.teams2", "com.microsoft.teams"],
            Target::Meet { bundle_ids, .. } => bundle_ids,
        }
    }
}

/// Why a post did not happen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PostError {
    /// Not on macOS.
    Unsupported,
    /// This process has no Accessibility permission.
    NotTrusted,
    /// The app isn't running.
    NotRunning,
    /// No meeting window (for a browser: no window shows Meet in front).
    NoMeeting,
    /// The meeting has no chat field Sokki can find, even after trying to
    /// open the chat (chat switched off by the host, waiting room, an app
    /// version that looks different).
    NoChatField,
    /// The person has an unsent draft in the chat field; Sokki leaves it.
    DraftInField,
    /// The text went into the field but pressing Return did not send it.
    NotSent,
}

impl PostError {
    /// Worth trying again in a few seconds: the meeting may not be on
    /// screen yet, or the person may still be joining or typing.
    pub fn retryable(&self) -> bool {
        matches!(self, PostError::NoMeeting | PostError::NoChatField | PostError::DraftInField)
    }
}

impl fmt::Display for PostError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            PostError::Unsupported => "posting into a meeting chat is macOS only",
            PostError::NotTrusted => "no Accessibility permission",
            PostError::NotRunning => "the meeting app is not running",
            PostError::NoMeeting => "no meeting window in front",
            PostError::NoChatField => "no chat field found",
            PostError::DraftInField => "the chat field holds an unsent draft",
            PostError::NotSent => "the message was not sent",
        })
    }
}

impl std::error::Error for PostError {}

/// A line Sokki posted, and the chat it went into.
pub struct Posted {
    pub target: Target,
    #[cfg(target_os = "macos")]
    inner: mac::Chat,
}

impl Posted {
    /// Whether someone has written "stop" (or "stopp") into the chat since
    /// the post, as a message of its own. `None` when the chat can't be
    /// read any more (panel closed, meeting over).
    pub fn stop_requested(&mut self) -> Option<bool> {
        #[cfg(target_os = "macos")]
        {
            self.inner.stop_requested()
        }
        #[cfg(not(target_os = "macos"))]
        {
            None
        }
    }

    /// Posts a second line into the same chat (the "stopped" reply).
    pub fn reply(&mut self, text: &str) -> Result<(), PostError> {
        #[cfg(target_os = "macos")]
        {
            self.inner.reply(text)
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = text;
            Err(PostError::Unsupported)
        }
    }
}

/// Posts `text` into the chat of the meeting running in `target`.
pub fn post(target: Target, text: &str) -> Result<Posted, PostError> {
    #[cfg(target_os = "macos")]
    {
        mac::post(target, text).map(|inner| Posted { target, inner })
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (target, text);
        Err(PostError::Unsupported)
    }
}

/// Prints the accessibility tree of the app's windows (for the `chat`
/// example, to see how an app version names its chat).
pub fn dump(target: Target, max: usize) -> Result<String, PostError> {
    #[cfg(target_os = "macos")]
    {
        mac::dump(target, max)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (target, max);
        Err(PostError::Unsupported)
    }
}

/// A chat message that asks to stop: "stop" or "stopp", any case, alone,
/// with or without quotes and punctuation ("Stopp!", "'stop'", "stop 🙏").
pub fn is_stop(message: &str) -> bool {
    let word: String = message
        .trim()
        .trim_matches(|c: char| !c.is_alphanumeric())
        .to_lowercase();
    matches!(word.as_str(), "stop" | "stopp")
}

/// Text normalised for comparing what was typed with what the chat shows:
/// whitespace collapsed, typographic quotes made plain.
pub fn normalize(s: &str) -> String {
    s.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .replace(['\u{2018}', '\u{2019}', '\u{201A}', '\u{201B}'], "'")
        .replace(['\u{201C}', '\u{201D}', '\u{201E}', '\u{201F}'], "\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stop_words() {
        for yes in ["stop", "Stop", "STOPP", " stopp! ", "'stop'", "„Stopp“", "stop 🙏", "stop."] {
            assert!(is_stop(yes), "{yes:?}");
        }
        for no in [
            "",
            "stop the recording please",
            "Stop Video",
            "Not okay? Type 'stop'. sokki.org",
            "unstoppable",
            "top",
        ] {
            assert!(!is_stop(no), "{no:?}");
        }
    }

    #[test]
    fn targets() {
        assert_eq!(Target::for_app("zoom"), Some(Target::Zoom));
        assert_eq!(Target::for_app("teams").map(|t| t.name()), Some("Teams"));
        assert_eq!(Target::for_app("brave").map(|t| t.name()), Some("Google Meet"));
        assert_eq!(Target::for_app("slack"), None);
        assert_eq!(Target::for_app("safari"), None);
    }

    #[test]
    fn normalizing() {
        assert_eq!(normalize("Type  ‘stop’.\n x"), "Type 'stop'. x");
    }
}
