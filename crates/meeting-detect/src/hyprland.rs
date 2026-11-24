//! Naming browser calls from Hyprland's window list. Pure parsing and
//! matching; the socket request lives in the Linux module.
//!
//! In a browser, the mic stream only says "Google Chrome" or "Firefox".
//! The browser's windows say more: a web app started with `--app=URL` gets
//! a window class built from the URL (`chrome-meet.google.com__-Default`,
//! `brave-…`, `msedge-…`), and a normal window's title is the active tab's
//! (`Meet – abc-defg-hij - Chromium`).

use serde_json::Value;

use crate::{AppKind, MeetingApp};

/// One window from Hyprland's `j/clients`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Window {
    pub class: String,
    pub title: String,
    pub pid: u32,
    /// 0 is the most recently focused window; Hyprland gives -1 to windows
    /// that were never focused.
    pub focus_history_id: i64,
}

/// Windows in the reply to Hyprland's `j/clients` request.
pub fn parse_clients(json: &str) -> serde_json::Result<Vec<Window>> {
    let v: Value = serde_json::from_str(json)?;
    Ok(v.as_array()
        .into_iter()
        .flatten()
        .filter_map(|w| {
            Some(Window {
                class: w.get("class")?.as_str()?.to_string(),
                title: w.get("title")?.as_str()?.to_string(),
                pid: u32::try_from(w.get("pid")?.as_i64()?).ok()?,
                focus_history_id: w
                    .get("focusHistoryID")
                    .and_then(Value::as_i64)
                    .unwrap_or(-1),
            })
        })
        .collect())
}

struct CallService {
    name: &'static str,
    /// The site's host, or a domain it ends in.
    hosts: &'static [&'static str],
    /// The tab title starts with one of these...
    title_prefixes: &'static [&'static str],
    /// ...or contains one of these.
    title_marks: &'static [&'static str],
}

const CALL_SERVICES: &[CallService] = &[
    CallService {
        name: "Google Meet",
        hosts: &["meet.google.com"],
        // "Meet – abc-defg-hij" (en dash), also seen with a hyphen.
        title_prefixes: &["Meet – ", "Meet - ", "Google Meet"],
        title_marks: &[],
    },
    CallService {
        name: "Microsoft Teams",
        hosts: &[
            "teams.microsoft.com",
            "teams.live.com",
            "teams.cloud.microsoft",
        ],
        title_prefixes: &[],
        title_marks: &["Microsoft Teams"],
    },
    CallService {
        name: "Zoom",
        hosts: &["zoom.us"],
        title_prefixes: &["Zoom"],
        title_marks: &[],
    },
    CallService {
        name: "Slack",
        hosts: &["app.slack.com"],
        title_prefixes: &[],
        title_marks: &["Slack"],
    },
    CallService {
        name: "Whereby",
        hosts: &["whereby.com"],
        title_prefixes: &[],
        title_marks: &["Whereby"],
    },
    CallService {
        name: "Jitsi Meet",
        hosts: &["meet.jit.si"],
        title_prefixes: &[],
        title_marks: &["Jitsi Meet"],
    },
    CallService {
        name: "Discord",
        hosts: &["discord.com"],
        title_prefixes: &[],
        title_marks: &["Discord"],
    },
    CallService {
        name: "Webex",
        hosts: &["webex.com"],
        title_prefixes: &[],
        title_marks: &["Webex"],
    },
];

/// The host in a browser web-app window class: `chrome-meet.google.com__-Default`
/// gives `meet.google.com`. Chromium builds the class as
/// `<browser>-<host>__<path>-<profile>`; normal browser windows have no `__`.
fn app_class_host(class: &str) -> Option<&str> {
    let (head, _) = class.split_once("__")?;
    let (_, host) = head.split_once('-')?;
    Some(host)
}

fn host_matches(host: &str, domain: &str) -> bool {
    let host = host.to_ascii_lowercase();
    host == domain || host.ends_with(&format!(".{domain}"))
}

/// The call service a browser window shows, from its class (web apps) or
/// title (tabs).
pub fn call_service(class: &str, title: &str) -> Option<&'static str> {
    if let Some(host) = app_class_host(class) {
        if let Some(s) = CALL_SERVICES
            .iter()
            .find(|s| s.hosts.iter().any(|d| host_matches(host, d)))
        {
            return Some(s.name);
        }
    }
    CALL_SERVICES
        .iter()
        .find(|s| {
            s.title_prefixes.iter().any(|p| title.starts_with(p))
                || s.title_marks.iter().any(|m| title.contains(m))
        })
        .map(|s| s.name)
}

fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// Set [`MeetingApp::call`] for every browser in `apps`: the call service
/// of that browser's most recently focused window that shows one.
///
/// A window belongs to a browser when both run the same executable
/// (`exe_of(window pid)` against the app's `exe_path`). Not by pid: Chrome
/// captures audio in a utility process, not in the one that owns the
/// windows. Native apps are left alone.
pub fn name_calls(
    apps: &mut [MeetingApp],
    windows: &[Window],
    exe_of: impl Fn(u32) -> Option<String>,
) {
    let exes: std::collections::HashMap<u32, Option<String>> =
        windows.iter().map(|w| (w.pid, exe_of(w.pid))).collect();
    name_calls_by(apps, windows, |app, w| {
        let binary = app.exe_path.as_deref().map(file_name);
        binary.is_some() && exes[&w.pid].as_deref().map(file_name) == binary
    });
}

/// [`name_calls`] with the platform's own test of which windows belong to
/// a browser (`same_app(browser, window)`).
pub fn name_calls_by(
    apps: &mut [MeetingApp],
    windows: &[Window],
    same_app: impl Fn(&MeetingApp, &Window) -> bool,
) {
    let mut windows: Vec<&Window> = windows.iter().collect();
    // Never-focused windows (-1) last.
    windows.sort_by_key(|w| u64::try_from(w.focus_history_id).unwrap_or(u64::MAX));
    for app in apps.iter_mut().filter(|a| a.kind == AppKind::Browser) {
        let call = windows
            .iter()
            .filter(|w| same_app(app, w))
            .find_map(|w| call_service(&w.class, &w.title))
            .map(str::to_string);
        app.call = call;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{classify_all, AudioClient};

    // Hand-written from Hyprland's documented `hyprctl clients -j` format
    // (the same JSON the `j/clients` socket request returns); not recorded
    // from a real session. The classes follow Chromium's web-app scheme as
    // Omarchy's web apps use it.
    const CLIENTS: &str = include_str!("../tests/fixtures/hyprland-clients.json");

    fn exe_of(pid: u32) -> Option<String> {
        Some(
            match pid {
                900 => "/usr/bin/alacritty",
                1201 | 1290 => "/usr/lib/chromium/chromium",
                1500 | 1580 => "/usr/lib/firefox/firefox",
                1700 | 1790 => "/opt/brave-bin/brave",
                2000 => "/usr/lib/signal-desktop/signal-desktop",
                _ => return None,
            }
            .to_string(),
        )
    }

    #[test]
    fn parses_windows() {
        let w = parse_clients(CLIENTS).unwrap();
        assert_eq!(w.len(), 6);
        assert_eq!(
            w[1],
            Window {
                class: "chrome-teams.microsoft.com__-Default".into(),
                title: "Weekly sync | Microsoft Teams".into(),
                pid: 1201,
                focus_history_id: 1,
            }
        );
        assert!(parse_clients("[]").unwrap().is_empty());
        assert!(parse_clients("ok").is_err());
    }

    #[test]
    fn services_from_web_app_classes() {
        let cases = [
            ("chrome-meet.google.com__-Default", Some("Google Meet")),
            (
                "chrome-teams.microsoft.com__-Default",
                Some("Microsoft Teams"),
            ),
            (
                "msedge-teams.live.com__v2-Profile_1",
                Some("Microsoft Teams"),
            ),
            ("chrome-app.zoom.us__wc_home-Default", Some("Zoom")),
            ("brave-app.slack.com__client-Default", Some("Slack")),
            ("chromium-whereby.com__team-Default", Some("Whereby")),
            ("chrome-meet.jit.si__-Default", Some("Jitsi Meet")),
            ("chrome-discord.com__app-Default", Some("Discord")),
            ("chrome-acme.webex.com__-Default", Some("Webex")),
            ("chrome-web.whatsapp.com__-Default", None),
            ("chrome-notzoom.us__-Default", None),
            ("google-chrome", None),
        ];
        for (class, want) in cases {
            assert_eq!(call_service(class, ""), want, "{class}");
        }
    }

    #[test]
    fn services_from_tab_titles() {
        let cases = [
            ("Meet – abc-defg-hij - Google Chrome", Some("Google Meet")),
            ("Meet - abc-defg-hij - Chromium", Some("Google Meet")),
            ("Google Meet - Brave", Some("Google Meet")),
            ("Zoom Meeting — Mozilla Firefox", Some("Zoom")),
            ("Chat | Microsoft Teams - Chromium", Some("Microsoft Teams")),
            ("Huddle: #design - Acme - Slack", Some("Slack")),
            ("standup | Whereby — Mozilla Firefox", Some("Whereby")),
            ("Jitsi Meet - Chromium", Some("Jitsi Meet")),
            ("#general | Friends - Discord - Chromium", Some("Discord")),
            ("Meetup: Rust Berlin - Chromium", None),
            ("New Tab - Google Chrome", None),
        ];
        for (title, want) in cases {
            assert_eq!(call_service("chromium", title), want, "{title}");
        }
    }

    fn browser_apps() -> Vec<MeetingApp> {
        // The audio utility processes, not the window owners.
        let clients = [
            AudioClient::new(1290, None, Some("/usr/lib/chromium/chromium")),
            AudioClient::new(1580, None, Some("/usr/lib/firefox/firefox")),
            AudioClient::new(1790, None, Some("/opt/brave-bin/brave")),
            AudioClient::new(3000, None, Some("/opt/zoom/zoom")),
            AudioClient::new(3100, None, Some("/opt/vivaldi/vivaldi-bin")),
        ];
        classify_all(&clients, 1)
    }

    #[test]
    fn names_browser_calls_by_binary_and_focus() {
        let windows = parse_clients(CLIENTS).unwrap();
        let mut apps = browser_apps();
        name_calls(&mut apps, &windows, exe_of);
        let got: Vec<_> = apps
            .iter()
            .map(|a| (a.id, a.call.as_deref(), a.label()))
            .collect();
        assert_eq!(
            got,
            [
                ("zoom", None, "Zoom"),
                ("brave", Some("Google Meet"), "Google Meet"),
                // No Vivaldi window: the browser name stays.
                ("vivaldi", None, "Vivaldi"),
                ("firefox", Some("Zoom"), "Zoom"),
                // The Teams web app was focused after the Meet tab.
                ("chromium", Some("Microsoft Teams"), "Microsoft Teams"),
            ]
        );
    }

    #[test]
    fn never_focused_windows_come_last_and_stale_calls_clear() {
        let w = |class: &str, title: &str, focus| Window {
            class: class.into(),
            title: title.into(),
            pid: 1201,
            focus_history_id: focus,
        };
        let windows = [
            w("chromium", "Zoom - Chromium", -1),
            w("chromium", "Meet – abc - Chromium", 3),
        ];
        let mut apps = browser_apps();
        let chromium = apps.iter().position(|a| a.id == "chromium").unwrap();
        name_calls(&mut apps, &windows, exe_of);
        assert_eq!(apps[chromium].call.as_deref(), Some("Google Meet"));
        name_calls(&mut apps, &[], exe_of);
        assert_eq!(apps[chromium].call, None);
    }
}
