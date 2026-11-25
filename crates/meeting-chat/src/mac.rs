//! Finding and using a meeting's chat field through Accessibility.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use objc2_app_kit::NSRunningApplication;
use objc2_foundation::NSString;

use crate::ax::{self, Element};
use crate::keys;
use crate::{is_stop, normalize, PostError, Target};

/// Most elements one search visits. Meet's whole page is ~2–4k elements.
const BUDGET: usize = 8000;

/// Buttons that end the call. A window with one of these is a meeting
/// window; no button with these words is ever pressed.
const LEAVE_WORDS: &[&str] = &[
    "leave", "verlassen", "hang up", "auflegen", "end call", "end meeting", "anruf beenden",
    "meeting beenden", "besprechung beenden",
];

/// Words in the chat field's name or placeholder.
const FIELD_WORDS: &[&str] = &[
    "message", "nachricht", "chat", "type", "eingeben", "schreib", "mensaje", "envoyer",
    "messaggio", "bericht",
];

/// Words in the name of the button that opens the chat panel.
const CHAT_BUTTON_WORDS: &[&str] = &["chat", "unterhaltung", "conversation"];

fn has_any(label: &str, words: &[&str]) -> bool {
    words.iter().any(|w| label.contains(w))
}

fn app_pid(target: Target) -> Option<i32> {
    for id in target.bundle_ids() {
        let apps = NSRunningApplication::runningApplicationsWithBundleIdentifier(&NSString::from_str(id));
        if let Some(app) = apps.iter().next() {
            return Some(app.processIdentifier());
        }
    }
    None
}

fn is_text_input(e: &Element) -> bool {
    matches!(e.role().as_str(), "AXTextArea" | "AXTextField" | "AXComboBox")
}

fn is_search(e: &Element) -> bool {
    e.string("AXSubrole").as_deref() == Some("AXSearchField") || {
        let l = e.label();
        l.contains("search") || l.contains("such")
    }
}

/// The title of Zoom's call window. Its toolbar, with the leave button,
/// leaves the accessibility tree while it is hidden (mouse elsewhere), so
/// the title has to do. The join preview is "<name>'s Zoom Meeting".
fn is_zoom_call_title(title: &str) -> bool {
    matches!(title.trim(), "zoom meeting" | "zoom-meeting" | "zoom webinar" | "zoom-webinar")
}

/// Whether this window holds a running call: it has a "leave" button.
fn is_meeting_window(win: &Element) -> bool {
    win.find(BUDGET, &mut |e| {
        matches!(e.role().as_str(), "AXButton" | "AXMenuButton" | "AXCheckBox")
            && has_any(&e.label(), LEAVE_WORDS)
    })
    .is_some()
}

/// Windows that may hold the meeting's chat, meeting windows first.
fn meeting_windows(app: &Element, target: Target) -> Vec<Element> {
    let windows = app.elements("AXWindows");
    // Chrome writes "Google\u{a0}Meet" with a no-break space.
    let title = |w: &Element| w.string("AXTitle").unwrap_or_default().to_lowercase().replace('\u{a0}', " ");
    let mut meeting: Vec<Element> = match target {
        // Only a window whose active tab is Meet: the title is the tab's.
        Target::Meet { .. } => windows
            .iter()
            .filter(|w| {
                let t = title(w);
                t.starts_with("meet") || t.contains("google meet") || has_meet_code(&t)
            })
            .filter(|w| is_meeting_window(w))
            .cloned()
            .collect(),
        Target::Zoom => windows
            .iter()
            .filter(|w| is_zoom_call_title(&title(w)) || is_meeting_window(w))
            .cloned()
            .collect(),
        Target::Teams => windows.iter().filter(|w| is_meeting_window(w)).cloned().collect(),
    };
    if !meeting.is_empty() && target == Target::Zoom {
        // Zoom can pop the chat out into a window of its own.
        for w in &windows {
            let t = title(w);
            if t.contains("chat") && !meeting.contains(w) && !t.contains("team chat") {
                meeting.push(w.clone());
            }
        }
    }
    if meeting.is_empty() {
        let titles: Vec<String> = windows.iter().map(|w| format!("{:?}", title(w))).collect();
        log::info!("meeting-chat: no {} meeting window among {}", target.name(), titles.join(", "));
    }
    meeting
}

/// A Meet code like "abc-defg-hij" in a window title.
fn has_meet_code(title: &str) -> bool {
    title.split(|c: char| !(c.is_ascii_lowercase() || c == '-')).any(|w| {
        let parts: Vec<&str> = w.split('-').collect();
        parts.len() == 3 && parts[0].len() == 3 && parts[1].len() == 4 && parts[2].len() == 3
    })
}

fn find_field(win: &Element) -> Option<Element> {
    // A disabled field: the host switched the chat off.
    let inputs = win.find_all(BUDGET, &mut |e| {
        is_text_input(e) && !is_search(e) && e.bool("AXEnabled") != Some(false)
    });
    if let Some(f) = inputs.iter().find(|e| has_any(&e.label(), FIELD_WORDS)) {
        return Some(f.clone());
    }
    // An unnamed field is used only when it is the window's only one.
    if inputs.len() == 1 {
        log::info!("meeting-chat: using the only text field: {}", inputs[0].describe());
        return inputs.into_iter().next();
    }
    if !inputs.is_empty() {
        let names: Vec<String> = inputs.iter().map(|e| e.describe()).collect();
        log::info!("meeting-chat: no chat field among {}", names.join("; "));
    }
    None
}

fn find_chat_button(win: &Element) -> Option<Element> {
    let buttons = win.find_all(BUDGET, &mut |e| {
        matches!(e.role().as_str(), "AXButton" | "AXCheckBox" | "AXRadioButton" | "AXMenuButton")
            && {
                let l = e.label();
                l.len() < 80 && has_any(&l, CHAT_BUTTON_WORDS) && !has_any(&l, LEAVE_WORDS)
            }
    });
    // "Chat" itself before "Chat with everyone" before anything else.
    buttons
        .iter()
        .min_by_key(|b| {
            let l = b.label();
            if l == "chat" || l.starts_with("chat |") {
                0
            } else if l.starts_with("chat") {
                1
            } else {
                2
            }
        })
        .cloned()
}

/// Lets Chromium build the tree of web content (it does so only for
/// assistive apps that ask). Electron apps take AXManualAccessibility;
/// Chrome, Edge, Brave and Arc take AXEnhancedUserInterface.
fn wake_web_accessibility(app: &Element) {
    let manual = app.set_bool_code("AXManualAccessibility", true);
    if manual == 0 {
        return;
    }
    let enhanced = app.set_bool_code("AXEnhancedUserInterface", true);
    if enhanced != 0 {
        log::info!(
            "meeting-chat: the browser took neither accessibility switch (AXManualAccessibility {manual}, AXEnhancedUserInterface {enhanced})"
        );
    }
}

fn web_tree_empty(win: &Element) -> bool {
    match win.find(BUDGET, &mut |e| e.role() == "AXWebArea") {
        Some(web) => web.children().is_empty(),
        None => true,
    }
}

fn wait_until(timeout: Duration, mut done: impl FnMut() -> bool) -> bool {
    let start = Instant::now();
    loop {
        if done() {
            return true;
        }
        if start.elapsed() >= timeout {
            return false;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Brings `pid`'s window to the front. Returns the app that was in front.
fn bring_to_front(app: &Element, win: &Element, pid: i32) -> Option<i32> {
    keys::wait_for_typing_pause(Duration::from_millis(1500), Duration::from_secs(15));
    let before = Element::frontmost_pid();
    win.set_bool("AXMain", true);
    win.perform("AXRaise");
    app.set_bool("AXFrontmost", true);
    if !wait_until(Duration::from_millis(1500), || Element::frontmost_pid() == Some(pid)) {
        log::info!("meeting-chat: the meeting app did not come to the front");
    }
    before
}

fn give_front_back(before: Option<i32>, pid: i32) {
    if let Some(prev) = before.filter(|p| *p != pid) {
        Element::app(prev).set_bool("AXFrontmost", true);
    }
}

fn field_text(field: &Element) -> String {
    field.string("AXValue").unwrap_or_default()
}

/// Puts `text` into the focused field and presses Return. `Ok` once the
/// field has emptied (the app took the message).
fn send(field: &Element, pid: i32, text: &str, web: bool) -> Result<(), PostError> {
    field.set_bool("AXFocused", true);
    std::thread::sleep(Duration::from_millis(150));
    let wanted = normalize(text);
    let cleared = || normalize(&field_text(field)) != wanted;

    // 1. As the field's value: instant, and nothing typed. Not in a web
    // page: Meet shows such a value but its own state never sees it, and
    // the next typing lands behind it (the line went out twice).
    if !web
        && field.settable("AXValue")
        && field.set_string("AXValue", text)
        && normalize(&field_text(field)) == wanted
    {
        std::thread::sleep(Duration::from_millis(120));
        keys::press_return(pid);
        if wait_until(Duration::from_millis(1500), cleared) {
            return Ok(());
        }
        // A web app may not notice a value it didn't see typed.
        log::info!("meeting-chat: Return after setting the value did not send; typing instead");
        field.set_string("AXValue", "");
    }
    // 2. Typed, key by key, into the meeting app only, into an empty field.
    field.set_bool("AXFocused", true);
    if !field_text(field).trim().is_empty() {
        keys::shortcut(pid, keys::KEY_A, keys::FLAG_COMMAND);
        keys::shortcut(pid, keys::KEY_DELETE, 0);
        if !wait_until(Duration::from_millis(800), || field_text(field).trim().is_empty()) {
            log::info!("meeting-chat: the field would not empty; not sending");
            return Err(PostError::NotSent);
        }
    }
    keys::type_text(pid, text);
    if !wait_until(Duration::from_millis(1500), || normalize(&field_text(field)) == wanted) {
        log::info!("meeting-chat: the field shows {:?} after typing", field_text(field));
    }
    std::thread::sleep(Duration::from_millis(120));
    keys::press_return(pid);
    if wait_until(Duration::from_millis(2000), cleared) {
        Ok(())
    } else {
        Err(PostError::NotSent)
    }
}

/// The part of the window that shows the sent line: the closest ancestor
/// of the field that contains it.
fn chat_region(field: &Element, win: &Element, text: &str) -> Element {
    let needle: String = normalize(text).chars().take(28).collect();
    let shows_line = |root: &Element| {
        root.find(3000, &mut |e| {
            !is_text_input(e) && e.text().is_some_and(|t| normalize(&t).contains(&needle))
        })
        .is_some()
    };
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        let mut node = field.element("AXParent");
        let mut depth = 0;
        while let Some(n) = node {
            if &n == win || depth > 14 {
                break;
            }
            if shows_line(&n) {
                log::info!("meeting-chat: watching the chat at {} (level {depth})", n.describe());
                return n;
            }
            node = n.element("AXParent");
            depth += 1;
        }
        std::thread::sleep(Duration::from_millis(300));
    }
    log::info!("meeting-chat: sent line not found in the chat; watching the whole window");
    win.clone()
}

fn count_stops(region: &Element) -> usize {
    region
        .find_all(BUDGET, &mut |e| !is_text_input(e) && e.text().is_some_and(|t| is_stop(&t)))
        .len()
}

/// One meeting chat Sokki has posted into.
pub struct Chat {
    target: Target,
    pid: i32,
    window: Element,
    region: Element,
    stops_seen: usize,
}

struct Found {
    pid: i32,
    app: Element,
    windows: Vec<Element>,
    /// The meeting window with its chat field, when the chat is open.
    chat: Option<(Element, Element)>,
    /// The app that was in front before Sokki brought the meeting app to
    /// this desktop, to give the front back to.
    before: Option<i32>,
}

/// Bringing the call app to this desktop takes the person's screen there:
/// at most once a minute, not on every retry.
fn may_raise() -> bool {
    static LAST: Mutex<Option<Instant>> = Mutex::new(None);
    let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
    if last.is_some_and(|t| t.elapsed() < Duration::from_secs(60)) {
        return false;
    }
    *last = Some(Instant::now());
    true
}

fn locate(target: Target) -> Result<Found, PostError> {
    if !ax::trusted() {
        return Err(PostError::NotTrusted);
    }
    let pid = app_pid(target).ok_or(PostError::NotRunning)?;
    let app = Element::app(pid);
    if matches!(target, Target::Meet { .. }) {
        wake_web_accessibility(&app);
    }
    let mut windows = meeting_windows(&app, target);
    if windows.is_empty() && matches!(target, Target::Meet { .. }) {
        // A browser that had no assistive app yet needs a moment.
        std::thread::sleep(Duration::from_millis(800));
        windows = meeting_windows(&app, target);
        if windows.is_empty() {
            let all = app.elements("AXWindows");
            if all.iter().any(web_tree_empty) {
                log::info!("meeting-chat: the browser shows no web content to Accessibility");
            }
        }
    }
    let mut before = None;
    // Only a native call app: a browser has many windows, and the one in
    // front need not be the call's.
    let native = matches!(target, Target::Zoom | Target::Teams);
    if native && windows.is_empty() && app.elements("AXWindows").is_empty() && may_raise() {
        // Accessibility lists only windows on the current desktop (Space).
        // The call runs on another one: bring the app here.
        log::info!("meeting-chat: {} has no window on this desktop; bringing it here", target.name());
        before = Element::frontmost_pid();
        app.set_bool("AXFrontmost", true);
        wait_until(Duration::from_millis(1500), || !app.elements("AXWindows").is_empty());
        windows = meeting_windows(&app, target);
        if windows.is_empty() {
            give_front_back(before, pid);
        }
    }
    if windows.is_empty() {
        return Err(PostError::NoMeeting);
    }
    let chat = windows.iter().find_map(|w| find_field(w).map(|f| (w.clone(), f)));
    Ok(Found { pid, app, windows, chat, before })
}

/// Opens the chat panel (its button, else the app's shortcut) and finds the
/// field. The app must already be in front for the shortcut.
fn open_chat(found_app: &Element, pid: i32, target: Target, windows: &[Element]) -> Option<(Element, Element)> {
    let wait_for_field = |win: &Element| {
        let mut field = None;
        wait_until(Duration::from_secs(3), || {
            field = find_field(win);
            field.is_some()
        });
        field
    };
    for w in windows {
        if let Some(button) = find_chat_button(w) {
            log::info!("meeting-chat: opening the chat with {}", button.describe());
            button.perform("AXPress");
            if let Some(f) = wait_for_field(w) {
                return Some((w.clone(), f));
            }
        }
    }
    let shortcut = match target {
        Target::Zoom => Some((keys::KEY_H, keys::FLAG_COMMAND | keys::FLAG_SHIFT)),
        Target::Meet { .. } => Some((keys::KEY_C, keys::FLAG_COMMAND | keys::FLAG_CONTROL)),
        Target::Teams => None,
    };
    if let Some((key, flags)) = shortcut {
        log::info!("meeting-chat: opening the chat with its shortcut");
        keys::shortcut(pid, key, flags);
        for w in meeting_windows(found_app, target) {
            if let Some(f) = wait_for_field(&w) {
                return Some((w, f));
            }
        }
    }
    None
}

pub fn post(target: Target, text: &str) -> Result<Chat, PostError> {
    let started = Instant::now();
    let found = locate(target)?;
    let pid = found.pid;
    let (window, field, before) = match found.chat {
        Some((window, field)) => {
            if !field_text(&field).trim().is_empty() {
                give_front_back(found.before, pid);
                return Err(PostError::DraftInField);
            }
            let front = bring_to_front(&found.app, &window, pid);
            (window, field, found.before.or(front))
        }
        None => {
            let first = &found.windows[0];
            let before = found.before.or(bring_to_front(&found.app, first, pid));
            match open_chat(&found.app, pid, target, &found.windows) {
                Some((w, f)) => (w, f, before),
                None => {
                    give_front_back(before, pid);
                    return Err(PostError::NoChatField);
                }
            }
        }
    };
    let result = send(&field, pid, text, matches!(target, Target::Meet { .. }));
    give_front_back(before, pid);
    result?;
    log::info!(
        "meeting-chat: posted into the {} chat in {} ms",
        target.name(),
        started.elapsed().as_millis()
    );
    let region = chat_region(&field, &window, text);
    let stops_seen = count_stops(&region);
    Ok(Chat { target, pid, window, region, stops_seen })
}

impl Chat {
    pub fn stop_requested(&mut self) -> Option<bool> {
        if !self.region.alive() {
            return None;
        }
        let n = count_stops(&self.region);
        if n > self.stops_seen {
            self.stops_seen = n;
            return Some(true);
        }
        // A message can scroll out of the list; never count down below it.
        self.stops_seen = self.stops_seen.max(n);
        Some(false)
    }

    pub fn reply(&mut self, text: &str) -> Result<(), PostError> {
        let app = Element::app(self.pid);
        let field = find_field(&self.window).ok_or(PostError::NoChatField)?;
        if !field_text(&field).trim().is_empty() {
            return Err(PostError::DraftInField);
        }
        let before = bring_to_front(&app, &self.window, self.pid);
        let result = send(&field, self.pid, text, matches!(self.target, Target::Meet { .. }));
        give_front_back(before, self.pid);
        log::info!("meeting-chat: reply in the {} chat: {result:?}", self.target.name());
        result
    }
}

pub fn dump(target: Target, max: usize) -> Result<String, PostError> {
    if !ax::trusted() {
        return Err(PostError::NotTrusted);
    }
    let pid = app_pid(target).ok_or(PostError::NotRunning)?;
    let app = Element::app(pid);
    if matches!(target, Target::Meet { .. }) {
        wake_web_accessibility(&app);
        std::thread::sleep(Duration::from_millis(800));
    }
    let mut out = String::new();
    let mut stack: Vec<(Element, usize)> =
        app.elements("AXWindows").into_iter().rev().map(|w| (w, 0)).collect();
    let mut n = 0;
    while let Some((e, depth)) = stack.pop() {
        n += 1;
        if n > max {
            out.push_str("…\n");
            break;
        }
        out.push_str(&format!("{}{}\n", "  ".repeat(depth), e.describe()));
        for k in e.children().into_iter().rev() {
            stack.push((k, depth + 1));
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::has_meet_code;

    #[test]
    fn meet_codes() {
        assert!(has_meet_code("meet – abc-defg-hij"));
        assert!(has_meet_code("abc-defg-hij - google chrome"));
        assert!(!has_meet_code("zoom meeting"));
        assert!(!has_meet_code("re-plan-now"));
    }

    #[test]
    fn zoom_call_titles() {
        use super::is_zoom_call_title;
        assert!(is_zoom_call_title("zoom meeting"));
        assert!(is_zoom_call_title("zoom webinar"));
        assert!(!is_zoom_call_title("robin's zoom meeting"));
        assert!(!is_zoom_call_title("zoom workplace"));
    }
}
