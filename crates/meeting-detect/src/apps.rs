//! Which audio-input clients are meeting apps. Pure functions over
//! [`AudioClient`] lists, so everything here is testable with fixtures.

use crate::AudioClient;

/// Native meeting app or a browser (where the meeting is a web page).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AppKind {
    Native,
    Browser,
}

/// Which signal identified the app.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MatchedBy {
    /// The client's own bundle id.
    BundleId,
    /// The client's executable path (helper processes without a bundle id).
    ExePath,
    /// The path of the process macOS holds responsible for the client
    /// (e.g. an app that owns an XPC helper).
    ResponsibleApp,
    /// A WebKit GPU/WebContent helper whose owning app could not be
    /// resolved; assumed to be Safari.
    WebKitHelper,
}

/// A known meeting app that is holding the microphone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MeetingApp {
    /// Stable identifier, e.g. `"zoom"`, `"chrome"`. The watcher treats two
    /// observations with the same id as the same meeting.
    pub id: &'static str,
    /// Display name, e.g. `"Zoom"`, `"Google Chrome"`.
    pub name: &'static str,
    pub kind: AppKind,
    /// The process that holds the mic (may be a helper of the app).
    pub pid: u32,
    pub bundle_id: Option<String>,
    pub exe_path: Option<String>,
    pub matched_by: MatchedBy,
    /// For a browser: the call service its windows show, e.g.
    /// `"Google Meet"`, when that is known (Linux with Hyprland).
    pub call: Option<String>,
}

impl MeetingApp {
    /// What to call this meeting in the UI: the call service if known,
    /// else the app name.
    pub fn label(&self) -> &str {
        self.call.as_deref().unwrap_or(self.name)
    }
}

struct KnownApp {
    id: &'static str,
    name: &'static str,
    kind: AppKind,
    /// Bundle ids. A client matches if its bundle id equals one of these or
    /// starts with it followed by `.` (so `com.google.Chrome` also matches
    /// `com.google.Chrome.helper` but `com.microsoft.teams` does not match
    /// `com.microsoft.teams2`). Case-insensitive.
    bundle_ids: &'static [&'static str],
    /// Substrings of the executable path. Case-insensitive. Written with a
    /// leading `/` (and usually a trailing `.app/`) so they match a path
    /// component, not arbitrary text.
    paths: &'static [&'static str],
    /// Executable file names on Linux, which has no bundle ids. Matched
    /// against the last component of the executable path,
    /// case-insensitive.
    bins: &'static [&'static str],
}

/// Known meeting apps, in preference order: when several hold the mic at
/// once, the earliest entry wins. Native meeting apps come before browsers
/// (a browser tab may hold the mic for something unrelated while the native
/// call runs); among browsers, order is arbitrary but fixed.
const KNOWN_APPS: &[KnownApp] = &[
    // ---- native meeting apps ----
    KnownApp {
        id: "zoom",
        name: "Zoom",
        kind: AppKind::Native,
        bundle_ids: &["us.zoom.xos"],
        paths: &["/zoom.us.app/"],
        bins: &["zoom"],
    },
    KnownApp {
        id: "teams",
        name: "Microsoft Teams",
        kind: AppKind::Native,
        // com.microsoft.teams2 = new Teams, com.microsoft.teams = classic.
        bundle_ids: &["com.microsoft.teams2", "com.microsoft.teams"],
        // "Microsoft Teams.app", "Microsoft Teams classic.app",
        // "Microsoft Teams (work or school).app" and their WebView helpers.
        // Listed before Edge: new Teams' helper path also contains
        // "Microsoft Edge Framework".
        paths: &["/microsoft teams"],
        bins: &["teams-for-linux", "teams"],
    },
    KnownApp {
        id: "webex",
        name: "Webex",
        kind: AppKind::Native,
        bundle_ids: &[
            "Cisco-Systems.Spark",
            "com.cisco.webexmeetingsapp",
            "com.webex.meetingmanager",
        ],
        paths: &[
            "/webex.app/",
            "/cisco webex meetings.app/",
            "/webex meetings.app/",
        ],
        bins: &["CiscoCollabHost"],
    },
    KnownApp {
        id: "slack",
        name: "Slack",
        kind: AppKind::Native,
        bundle_ids: &["com.tinyspeck.slackmacgap"],
        paths: &["/slack.app/"],
        bins: &["slack"],
    },
    KnownApp {
        id: "facetime",
        name: "FaceTime",
        kind: AppKind::Native,
        // avconferenced is the daemon that runs FaceTime call audio and
        // also iPhone calls taken on the Mac (Continuity). Those are
        // offered too, on purpose (Robin, 2026-11-25): macOS gives call
        // state (FaceTime or phone) only to Apple's own apps, and Robin
        // wants phone calls on the Mac recordable. MEETINGS.md, "Other
        // people's voices", point 4.
        bundle_ids: &["com.apple.FaceTime", "com.apple.avconference"],
        paths: &["/facetime.app/", "/avconferenced"],
        bins: &[],
    },
    KnownApp {
        id: "discord",
        name: "Discord",
        kind: AppKind::Native,
        bundle_ids: &[
            "com.hnc.Discord",
            "com.hnc.DiscordPTB",
            "com.hnc.DiscordCanary",
        ],
        paths: &["/discord.app/", "/discord ptb.app/", "/discord canary.app/"],
        bins: &["Discord", "DiscordPTB", "DiscordCanary"],
    },
    KnownApp {
        id: "skype",
        name: "Skype",
        kind: AppKind::Native,
        bundle_ids: &["com.skype.skype"],
        paths: &["/skype.app/"],
        bins: &["skypeforlinux"],
    },
    KnownApp {
        id: "whatsapp",
        name: "WhatsApp",
        kind: AppKind::Native,
        bundle_ids: &["net.whatsapp.WhatsApp", "desktop.WhatsApp"],
        paths: &["/whatsapp.app/"],
        bins: &[],
    },
    KnownApp {
        id: "signal",
        name: "Signal",
        kind: AppKind::Native,
        bundle_ids: &["org.whispersystems.signal-desktop"],
        paths: &["/signal.app/"],
        bins: &["signal-desktop", "signal-desktop-beta"],
    },
    KnownApp {
        id: "telegram",
        name: "Telegram",
        kind: AppKind::Native,
        bundle_ids: &["ru.keepcoder.Telegram", "org.telegram.desktop"],
        paths: &["/telegram.app/", "/telegram desktop.app/"],
        bins: &["telegram-desktop", "Telegram"],
    },
    KnownApp {
        id: "element",
        name: "Element",
        kind: AppKind::Native,
        // Linux only for now; on macOS Element is not in the table.
        bundle_ids: &[],
        paths: &[],
        bins: &["element-desktop"],
    },
    KnownApp {
        id: "around",
        name: "Around",
        kind: AppKind::Native,
        bundle_ids: &["co.teamport.around"],
        paths: &["/around.app/"],
        bins: &[],
    },
    KnownApp {
        id: "tuple",
        name: "Tuple",
        kind: AppKind::Native,
        bundle_ids: &["app.tuple.app"],
        paths: &["/tuple.app/"],
        bins: &[],
    },
    KnownApp {
        id: "gotomeeting",
        name: "GoTo Meeting",
        kind: AppKind::Native,
        bundle_ids: &["com.logmein.GoToMeeting"],
        paths: &["/goto meeting.app/", "/gotomeeting.app/"],
        bins: &[],
    },
    KnownApp {
        id: "chime",
        name: "Amazon Chime",
        kind: AppKind::Native,
        bundle_ids: &["com.amazon.Amazon-Chime"],
        paths: &["/amazon chime.app/"],
        bins: &[],
    },
    // ---- browsers (Google Meet, Teams/Zoom/Whereby web, ...) ----
    KnownApp {
        id: "chrome",
        name: "Google Chrome",
        kind: AppKind::Browser,
        // Prefix match covers .helper, .beta, .canary, .dev.
        bundle_ids: &["com.google.Chrome"],
        // Covers "Google Chrome.app", "Google Chrome Beta.app", "Google
        // Chrome Canary.app" and "Google Chrome Helper.app".
        paths: &["/google chrome"],
        bins: &["chrome"],
    },
    KnownApp {
        id: "arc",
        name: "Arc",
        kind: AppKind::Browser,
        bundle_ids: &["company.thebrowser.Browser"],
        paths: &["/arc.app/"],
        bins: &[],
    },
    KnownApp {
        id: "dia",
        name: "Dia",
        kind: AppKind::Browser,
        bundle_ids: &["company.thebrowser.dia"],
        paths: &["/dia.app/"],
        bins: &[],
    },
    KnownApp {
        id: "edge",
        name: "Microsoft Edge",
        kind: AppKind::Browser,
        bundle_ids: &["com.microsoft.edgemac"],
        paths: &[
            "/microsoft edge.app/",
            "/microsoft edge beta.app/",
            "/microsoft edge dev.app/",
            "/microsoft edge canary.app/",
        ],
        bins: &["msedge"],
    },
    KnownApp {
        id: "brave",
        name: "Brave",
        kind: AppKind::Browser,
        bundle_ids: &["com.brave.Browser"],
        paths: &["/brave browser"],
        bins: &["brave"],
    },
    KnownApp {
        id: "vivaldi",
        name: "Vivaldi",
        kind: AppKind::Browser,
        bundle_ids: &["com.vivaldi.Vivaldi"],
        paths: &["/vivaldi.app/"],
        bins: &["vivaldi-bin"],
    },
    KnownApp {
        id: "opera",
        name: "Opera",
        kind: AppKind::Browser,
        bundle_ids: &["com.operasoftware.Opera", "com.operasoftware.OperaGX"],
        paths: &["/opera.app/", "/opera gx.app/"],
        bins: &["opera"],
    },
    KnownApp {
        id: "firefox",
        name: "Firefox",
        kind: AppKind::Browser,
        bundle_ids: &[
            "org.mozilla.firefox",
            "org.mozilla.nightly",
            "org.mozilla.firefoxdeveloperedition",
        ],
        // Firefox.app, Firefox Nightly.app, Firefox Developer Edition.app,
        // including plugin-container inside them.
        paths: &["/firefox"],
        bins: &["firefox", "firefox-bin"],
    },
    KnownApp {
        id: "chromium",
        name: "Chromium",
        kind: AppKind::Browser,
        bundle_ids: &["org.chromium.Chromium"],
        paths: &["/chromium.app/"],
        bins: &["chromium"],
    },
    KnownApp {
        id: "helium",
        name: "Helium",
        kind: AppKind::Browser,
        bundle_ids: &[],
        paths: &[],
        bins: &["helium"],
    },
    KnownApp {
        id: "safari",
        name: "Safari",
        kind: AppKind::Browser,
        bundle_ids: &["com.apple.Safari", "com.apple.SafariTechnologyPreview"],
        paths: &["/safari.app/", "/safari technology preview.app/"],
        bins: &[],
    },
];

const SAFARI_INDEX: usize = KNOWN_APPS.len() - 1;

/// Dictation and transcription tools. They hold the mic for their own
/// reasons; their mic use is never a meeting. Matched like [`KNOWN_APPS`],
/// on the client itself and on its responsible process.
const EXCLUDED_BUNDLE_IDS: &[&str] = &[
    "com.pais.handy",                  // Handy
    "com.kole.lark",                   // Lark
    "com.superduper.superwhisper",     // superwhisper
    "com.electron.wispr-flow",         // Wispr Flow
    "com.goodsnooze.MacWhisper",       // MacWhisper
    "com.prakashjoshipax.VoiceInk",    // VoiceInk
    "com.apple.SpeechRecognitionCore", // Apple dictation (speechrecognitiond)
];

const EXCLUDED_PATHS: &[&str] = &[
    "/handy.app/",
    "/superwhisper.app/",
    "/wispr flow.app/",
    "/macwhisper.app/",
    "/voiceink.app/",
    "/speechrecognitioncore.framework/",
];

/// Bundle-id / path markers of WebKit's out-of-process helpers. They serve
/// Safari but also every app that embeds a WKWebView (including Tauri apps
/// such as Sokki), so they are attributed via the responsible process.
const WEBKIT_BUNDLE_PREFIX: &str = "com.apple.webkit.";
const WEBKIT_PATH_MARKER: &str = "/com.apple.webkit.";

fn bundle_matches(bundle_id: &str, pattern: &str) -> bool {
    let b = bundle_id.to_ascii_lowercase();
    let p = pattern.to_ascii_lowercase();
    b == p || (b.starts_with(&p) && b.as_bytes().get(p.len()) == Some(&b'.'))
}

fn path_matches(path: &str, fragment: &str) -> bool {
    path.to_ascii_lowercase()
        .contains(&fragment.to_ascii_lowercase())
}

/// Index into [`KNOWN_APPS`] of the first app matching by bundle id, then by
/// path.
fn match_known(bundle_id: Option<&str>, path: Option<&str>) -> Option<(usize, MatchedBy)> {
    if let Some(bid) = bundle_id {
        if let Some(i) = KNOWN_APPS
            .iter()
            .position(|k| k.bundle_ids.iter().any(|p| bundle_matches(bid, p)))
        {
            return Some((i, MatchedBy::BundleId));
        }
    }
    if let Some(path) = path {
        let file = path.rsplit('/').next().unwrap_or(path);
        if let Some(i) = KNOWN_APPS.iter().position(|k| {
            k.paths.iter().any(|f| path_matches(path, f))
                || k.bins.iter().any(|b| file.eq_ignore_ascii_case(b))
        }) {
            return Some((i, MatchedBy::ExePath));
        }
    }
    None
}

fn is_excluded(bundle_id: Option<&str>, path: Option<&str>) -> bool {
    bundle_id.is_some_and(|b| EXCLUDED_BUNDLE_IDS.iter().any(|p| bundle_matches(b, p)))
        || path.is_some_and(|p| EXCLUDED_PATHS.iter().any(|f| path_matches(p, f)))
}

fn is_webkit_helper(c: &AudioClient) -> bool {
    c.bundle_id
        .as_deref()
        .is_some_and(|b| b.to_ascii_lowercase().starts_with(WEBKIT_BUNDLE_PREFIX))
        || c.exe_path
            .as_deref()
            .is_some_and(|p| path_matches(p, WEBKIT_PATH_MARKER))
}

/// The known-app index and match kind for one client, or `None`.
fn classify_client(c: &AudioClient, own_pid: u32) -> Option<(usize, MatchedBy)> {
    if c.pid == own_pid || c.responsible_pid == Some(own_pid) {
        return None;
    }
    if is_excluded(c.bundle_id.as_deref(), c.exe_path.as_deref())
        || is_excluded(None, c.responsible_path.as_deref())
    {
        return None;
    }
    if is_webkit_helper(c) {
        return match c.responsible_path.as_deref() {
            // Owned by a known app (Safari, or a meeting app's web view).
            Some(rp) => match_known(None, Some(rp)).map(|(i, _)| (i, MatchedBy::ResponsibleApp)),
            // Owner unknown: Safari is by far the most likely.
            None => Some((SAFARI_INDEX, MatchedBy::WebKitHelper)),
        };
    }
    if let Some(m) = match_known(c.bundle_id.as_deref(), c.exe_path.as_deref()) {
        return Some(m);
    }
    if let Some(m) = c.binary.as_deref().and_then(|b| match_known(None, Some(b))) {
        return Some(m);
    }
    c.responsible_path
        .as_deref()
        .and_then(|rp| match_known(None, Some(rp)))
        .map(|(i, _)| (i, MatchedBy::ResponsibleApp))
}

/// All known meeting apps among `clients`, best first (native apps before
/// browsers, then table order), one entry per app. Clients with pid
/// `own_pid` (or owned by it) and known dictation tools are ignored.
pub fn classify_all(clients: &[AudioClient], own_pid: u32) -> Vec<MeetingApp> {
    let mut hits: Vec<(usize, MeetingApp)> = Vec::new();
    for c in clients {
        let Some((i, matched_by)) = classify_client(c, own_pid) else {
            log::trace!(
                "mic client not a meeting app: pid={} bundle={:?} path={:?} responsible={:?}",
                c.pid,
                c.bundle_id,
                c.exe_path,
                c.responsible_path
            );
            continue;
        };
        if hits.iter().any(|(j, _)| *j == i) {
            continue; // keep the first client seen for each app
        }
        let k = &KNOWN_APPS[i];
        hits.push((
            i,
            MeetingApp {
                id: k.id,
                name: k.name,
                kind: k.kind,
                pid: c.pid,
                bundle_id: c.bundle_id.clone(),
                exe_path: c.exe_path.clone(),
                matched_by,
                call: None,
            },
        ));
    }
    // Stable sort: table order is the preference order.
    hits.sort_by_key(|(i, _)| *i);
    hits.into_iter().map(|(_, app)| app).collect()
}

/// The preferred meeting app holding the mic, if any. See [`classify_all`].
pub fn classify(clients: &[AudioClient], own_pid: u32) -> Option<MeetingApp> {
    classify_all(clients, own_pid).into_iter().next()
}

/// `(id, name, kind)` of every known meeting app, in preference order.
pub fn known_apps() -> impl Iterator<Item = (&'static str, &'static str, AppKind)> {
    KNOWN_APPS.iter().map(|k| (k.id, k.name, k.kind))
}

#[cfg(test)]
mod tests {
    use super::*;

    const OWN: u32 = 4242;

    fn c(pid: u32, bid: Option<&str>, path: Option<&str>) -> AudioClient {
        AudioClient::new(pid, bid, path)
    }

    const ZOOM: &str = "/Applications/zoom.us.app/Contents/MacOS/zoom.us";
    const CHROME_HELPER: &str = "/Applications/Google Chrome.app/Contents/Frameworks/Google Chrome Framework.framework/Versions/131.0.6778.86/Helpers/Google Chrome Helper.app/Contents/MacOS/Google Chrome Helper";
    const WEBKIT_GPU: &str = "/System/Volumes/Preboot/Cryptexes/OS/System/Library/Frameworks/WebKit.framework/Versions/A/XPCServices/com.apple.WebKit.GPU.xpc/Contents/MacOS/com.apple.WebKit.GPU";
    const WEBKIT_WEBCONTENT: &str = "/System/Library/Frameworks/WebKit.framework/Versions/A/XPCServices/com.apple.WebKit.WebContent.xpc/Contents/MacOS/com.apple.WebKit.WebContent";
    const SAFARI: &str = "/Applications/Safari.app/Contents/MacOS/Safari";
    const HANDY: &str = "/Applications/Handy.app/Contents/MacOS/handy";
    const SOKKI: &str = "/Applications/Sokki.app/Contents/MacOS/sokki";

    #[test]
    fn zoom_native_by_bundle_id() {
        let app = classify(&[c(100, Some("us.zoom.xos"), Some(ZOOM))], OWN).unwrap();
        assert_eq!(app.id, "zoom");
        assert_eq!(app.name, "Zoom");
        assert_eq!(app.kind, AppKind::Native);
        assert_eq!(app.pid, 100);
        assert_eq!(app.matched_by, MatchedBy::BundleId);
    }

    #[test]
    fn zoom_by_path_when_no_bundle_id() {
        let app = classify(&[c(100, None, Some(ZOOM))], OWN).unwrap();
        assert_eq!(app.id, "zoom");
        assert_eq!(app.matched_by, MatchedBy::ExePath);
    }

    #[test]
    fn chrome_helper_by_path_without_bundle_id() {
        let app = classify(&[c(200, None, Some(CHROME_HELPER))], OWN).unwrap();
        assert_eq!(app.id, "chrome");
        assert_eq!(app.kind, AppKind::Browser);
        assert_eq!(app.matched_by, MatchedBy::ExePath);
    }

    #[test]
    fn chrome_helper_by_bundle_prefix() {
        let app = classify(&[c(200, Some("com.google.Chrome.helper"), None)], OWN).unwrap();
        assert_eq!(app.id, "chrome");
        assert_eq!(app.matched_by, MatchedBy::BundleId);
    }

    #[test]
    fn safari_webcontent_owned_by_safari() {
        let client = c(
            300,
            Some("com.apple.WebKit.WebContent"),
            Some(WEBKIT_WEBCONTENT),
        )
        .with_responsible(301, Some(SAFARI));
        let app = classify(&[client], OWN).unwrap();
        assert_eq!(app.id, "safari");
        assert_eq!(app.matched_by, MatchedBy::ResponsibleApp);
    }

    #[test]
    fn webkit_gpu_with_unknown_owner_assumed_safari() {
        let app = classify(&[c(300, None, Some(WEBKIT_GPU))], OWN).unwrap();
        assert_eq!(app.id, "safari");
        assert_eq!(app.matched_by, MatchedBy::WebKitHelper);
    }

    #[test]
    fn webkit_helper_of_unrelated_app_is_ignored() {
        // e.g. a WKWebView inside some note-taking app
        let client = c(300, Some("com.apple.WebKit.GPU"), Some(WEBKIT_GPU)).with_responsible(
            301,
            Some("/Applications/Notes Thing.app/Contents/MacOS/Notes Thing"),
        );
        assert_eq!(classify(&[client], OWN), None);
    }

    #[test]
    fn webkit_helper_owned_by_own_process_is_ignored() {
        let client = c(300, None, Some(WEBKIT_GPU)).with_responsible(OWN, Some(SOKKI));
        assert_eq!(classify(&[client], OWN), None);
    }

    #[test]
    fn own_pid_excluded_even_if_it_looks_like_a_meeting_app() {
        assert_eq!(
            classify(&[c(OWN, Some("us.zoom.xos"), Some(ZOOM))], OWN),
            None
        );
    }

    #[test]
    fn dictation_tools_excluded() {
        let clients = [
            c(10, Some("com.pais.handy"), Some(HANDY)),
            c(
                11,
                None,
                Some("/Applications/superwhisper.app/Contents/MacOS/superwhisper"),
            ),
            c(12, Some("com.electron.wispr-flow"), None),
            c(13, Some("com.goodsnooze.MacWhisper"), None),
        ];
        assert_eq!(classify(&clients, OWN), None);
    }

    #[test]
    fn helper_of_dictation_tool_excluded_via_responsible() {
        // A Chrome-like helper path would match, but its owner is a
        // dictation tool: ignore.
        let client = c(20, None, Some("/tmp/whatever/helper")).with_responsible(
            21,
            Some("/Applications/VoiceInk.app/Contents/MacOS/VoiceInk"),
        );
        assert_eq!(classify(&[client], OWN), None);
    }

    #[test]
    fn native_app_preferred_over_browser_regardless_of_order() {
        let clients = [
            c(200, None, Some(CHROME_HELPER)),
            c(300, None, Some(WEBKIT_GPU)),
            c(100, Some("us.zoom.xos"), Some(ZOOM)),
        ];
        let all = classify_all(&clients, OWN);
        let ids: Vec<_> = all.iter().map(|a| a.id).collect();
        assert_eq!(ids, ["zoom", "chrome", "safari"]);
        assert_eq!(classify(&clients, OWN).unwrap().id, "zoom");
    }

    #[test]
    fn several_processes_of_one_app_reported_once() {
        let clients = [
            c(200, None, Some(CHROME_HELPER)),
            c(201, Some("com.google.Chrome.helper"), Some(CHROME_HELPER)),
        ];
        let all = classify_all(&clients, OWN);
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].pid, 200);
    }

    #[test]
    fn teams_webview_helper_is_teams_not_edge() {
        let p = "/Applications/Microsoft Teams.app/Contents/Helpers/Microsoft Teams WebView.app/Contents/Frameworks/Microsoft Edge Framework.framework/Helpers/Microsoft Teams WebView Helper.app/Contents/MacOS/Microsoft Teams WebView Helper";
        assert_eq!(classify(&[c(1, None, Some(p))], OWN).unwrap().id, "teams");
        assert_eq!(
            classify(&[c(1, Some("com.microsoft.teams2"), None)], OWN)
                .unwrap()
                .id,
            "teams"
        );
        assert_eq!(
            classify(&[c(1, Some("com.microsoft.teams"), None)], OWN)
                .unwrap()
                .id,
            "teams"
        );
    }

    #[test]
    fn bundle_prefix_respects_component_boundary() {
        // Arc's bundle id must not swallow Dia's or anything else.
        assert_eq!(
            classify(&[c(1, Some("company.thebrowser.dia"), None)], OWN)
                .unwrap()
                .id,
            "dia"
        );
        assert_eq!(classify(&[c(1, Some("us.zoom.xosfake"), None)], OWN), None);
    }

    #[test]
    fn other_meeting_apps_match() {
        let cases = [
            ("Cisco-Systems.Spark", "webex"),
            ("com.tinyspeck.slackmacgap.helper", "slack"),
            ("com.apple.FaceTime", "facetime"),
            ("com.hnc.Discord.helper", "discord"),
            ("net.whatsapp.WhatsApp", "whatsapp"),
            ("org.whispersystems.signal-desktop.helper", "signal"),
            ("ru.keepcoder.Telegram", "telegram"),
            ("org.mozilla.firefox", "firefox"),
            ("com.brave.Browser.helper", "brave"),
            ("com.microsoft.edgemac.helper", "edge"),
            ("com.vivaldi.Vivaldi", "vivaldi"),
            ("com.operasoftware.Opera", "opera"),
            ("company.thebrowser.Browser.helper", "arc"),
        ];
        for (bid, id) in cases {
            assert_eq!(
                classify(&[c(1, Some(bid), None)], OWN).map(|a| a.id),
                Some(id),
                "{bid}"
            );
        }
        let paths = [
            ("/usr/libexec/avconferenced", "facetime"),
            ("/Applications/Firefox.app/Contents/MacOS/plugin-container.app/Contents/MacOS/plugin-container", "firefox"),
            ("/Applications/Arc.app/Contents/Frameworks/ArcCore.framework/Versions/A/Helpers/Browser Helper.app/Contents/MacOS/Browser Helper", "arc"),
            ("/Applications/Slack.app/Contents/Frameworks/Slack Helper.app/Contents/MacOS/Slack Helper", "slack"),
            ("/Applications/Brave Browser.app/Contents/Frameworks/Brave Browser Framework.framework/Helpers/Brave Browser Helper.app/Contents/MacOS/Brave Browser Helper", "brave"),
        ];
        for (p, id) in paths {
            assert_eq!(
                classify(&[c(1, None, Some(p))], OWN).map(|a| a.id),
                Some(id),
                "{p}"
            );
        }
    }

    #[test]
    fn continuity_phone_call_is_offered_as_facetime() {
        // An iPhone call taken on the Mac holds the mic through
        // avconferenced, exactly like FaceTime. It is offered on purpose.
        let client = c(
            1178,
            Some("com.apple.avconference"),
            Some("/usr/libexec/avconferenced"),
        );
        let app = classify(&[client], OWN).unwrap();
        assert_eq!(app.id, "facetime");
        assert_eq!(app.kind, AppKind::Native);
        assert_eq!(app.matched_by, MatchedBy::BundleId);
        // Without a bundle id the daemon's path still matches.
        let app = classify(&[c(1178, None, Some("/usr/libexec/avconferenced"))], OWN).unwrap();
        assert_eq!(app.id, "facetime");
    }

    #[test]
    fn linux_apps_match_by_binary() {
        let cases = [
            ("/opt/zoom/zoom", "zoom"),
            ("/usr/lib/teams-for-linux/teams-for-linux", "teams"),
            ("/opt/Webex/bin/CiscoCollabHost", "webex"),
            ("/usr/lib/slack/slack", "slack"),
            ("/opt/discord/Discord", "discord"),
            ("/usr/share/skypeforlinux/skypeforlinux", "skype"),
            ("/usr/lib/signal-desktop/signal-desktop", "signal"),
            ("/usr/bin/telegram-desktop", "telegram"),
            ("/usr/bin/Telegram", "telegram"),
            ("/opt/Element/element-desktop", "element"),
            ("/opt/google/chrome/chrome", "chrome"),
            ("/usr/lib/chromium/chromium", "chromium"),
            ("/opt/brave-bin/brave", "brave"),
            ("/usr/lib/firefox/firefox", "firefox"),
            ("/opt/vivaldi/vivaldi-bin", "vivaldi"),
            ("/opt/microsoft/msedge/msedge", "edge"),
            ("/usr/lib/opera/opera", "opera"),
            ("/opt/helium/helium", "helium"),
            // /proc/<pid>/exe unreadable: only application.process.binary.
            ("chrome", "chrome"),
            ("zoom", "zoom"),
        ];
        for (p, id) in cases {
            let app = classify(&[c(1, None, Some(p))], OWN);
            assert_eq!(app.as_ref().map(|a| a.id), Some(id), "{p}");
            assert_eq!(app.unwrap().matched_by, MatchedBy::ExePath);
        }
        for p in [
            "/usr/bin/pw-cat",
            "/usr/bin/parecord",
            "/usr/bin/zoomer",
            "/home/me/chrome-tools/run",
        ] {
            assert_eq!(classify(&[c(1, None, Some(p))], OWN), None, "{p}");
        }
    }

    #[test]
    fn linux_native_app_preferred_over_browser() {
        let clients = [
            c(10, None, Some("/opt/google/chrome/chrome")),
            c(20, None, Some("/opt/zoom/zoom")),
        ];
        let ids: Vec<_> = classify_all(&clients, OWN).iter().map(|a| a.id).collect();
        assert_eq!(ids, ["zoom", "chrome"]);
    }

    #[test]
    fn pipewire_binary_is_a_second_chance() {
        // /proc names a translator (Rosetta in an x86 container); PipeWire
        // still says which program it is.
        let mut client = c(42, None, Some("/run/rosetta/rosetta"));
        assert!(classify(&[client.clone()], OWN).is_none());
        client.binary = Some("zoom".into());
        assert_eq!(classify(&[client], OWN).map(|a| a.id), Some("zoom"));
    }

    #[test]
    fn label_prefers_the_call() {
        let mut app = classify(&[c(1, None, Some("/usr/lib/chromium/chromium"))], OWN).unwrap();
        assert_eq!(app.label(), "Chromium");
        app.call = Some("Google Meet".into());
        assert_eq!(app.label(), "Google Meet");
    }

    #[test]
    fn nothing_matching() {
        let clients = [
            c(1, None, Some("/opt/homebrew/bin/ffmpeg")),
            c(
                2,
                Some("com.apple.Terminal"),
                Some("/System/Applications/Utilities/Terminal.app/Contents/MacOS/Terminal"),
            ),
            c(3, Some("com.spotify.client"), None),
            c(4, None, None),
        ];
        assert_eq!(classify(&clients, OWN), None);
        assert!(classify_all(&[], OWN).is_empty());
    }

    #[test]
    fn table_has_unique_ids_and_natives_first() {
        let apps: Vec<_> = known_apps().collect();
        let mut ids: Vec<_> = apps.iter().map(|a| a.0).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), apps.len());
        let first_browser = apps.iter().position(|a| a.2 == AppKind::Browser).unwrap();
        assert!(apps[first_browser..]
            .iter()
            .all(|a| a.2 == AppKind::Browser));
        assert_eq!(KNOWN_APPS[SAFARI_INDEX].id, "safari");
    }
}
