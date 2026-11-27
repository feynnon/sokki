//! Debounced meeting start/end events.
//!
//! [`Detector`] is the pure state machine: feed it "these meeting apps hold
//! the mic now" plus a timestamp, get events back. [`Watcher`] runs it on a
//! background thread, in one of two ways:
//!
//! - polling a [`ClientSource`] every `poll_interval` (macOS, Core Audio);
//! - event-driven from an [`EventSource`] that pushes the client list when
//!   it changes (Linux, `pw-dump --monitor`). The detector then runs on a
//!   change and once more at its next deadline (`start_after`,
//!   `end_after`), never in between: idle means blocked, no wakeups.
//!
//! Sources and the [`Clock`] can be injected, for tests.

use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::{classify_all, AudioClient, MeetingApp, Result};

/// What the watcher reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MeetingEvent {
    /// A meeting app has held the mic for at least
    /// [`WatcherConfig::start_after`].
    Started { app: MeetingApp },
    /// The app from the matching `Started` has not held the mic for
    /// [`WatcherConfig::end_after`]. `after` is how long it held the mic:
    /// from the first poll that saw it to the first poll that no longer
    /// did (the grace period is not included).
    Ended { app: MeetingApp, after: Duration },
}

/// Timing knobs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WatcherConfig {
    /// How often Core Audio is polled. Default 2 s. Unused by the
    /// event-driven watcher.
    pub poll_interval: Duration,
    /// How long a meeting app must hold the mic before `Started`. Filters
    /// out blips such as a browser's permission probe. Default 3 s. With a
    /// 2 s poll, `Started` arrives on the third sighting (~4 s in).
    pub start_after: Duration,
    /// How long the app must have released the mic before `Ended`.
    /// Re-acquiring within this window (device switch, reconnect) continues
    /// the same meeting. Default 20 s.
    pub end_after: Duration,
}

impl Default for WatcherConfig {
    fn default() -> Self {
        WatcherConfig {
            poll_interval: Duration::from_secs(2),
            start_after: Duration::from_secs(3),
            end_after: Duration::from_secs(20),
        }
    }
}

/// Where the detector currently stands.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum DetectorState {
    /// No meeting app holds the mic.
    #[default]
    Idle,
    /// `app` holds the mic since `since`, not long enough for `Started` yet.
    Pending { app: MeetingApp, since: Instant },
    /// `Started` was emitted for `app`; it still holds the mic.
    Active { app: MeetingApp, since: Instant },
    /// `app` let go of the mic at `released_at`; `Ended` follows unless it
    /// re-acquires within `end_after`.
    Releasing {
        app: MeetingApp,
        since: Instant,
        released_at: Instant,
    },
}

/// The debounce state machine. Pure: no threads, no clock, no Core Audio.
///
/// Identity is [`MeetingApp::id`]: a different pid of the same app (Chrome
/// restarting its audio helper) is the same meeting. While a meeting is
/// active, other apps grabbing the mic are ignored. If the active app
/// releases the mic and another one holds it, the first meeting ends after
/// `end_after` and the other one then has to pass `start_after` on its own
/// (an app switch yields `Ended` then `Started`, never two overlapping
/// meetings).
#[derive(Debug, Clone)]
pub struct Detector {
    config: WatcherConfig,
    state: DetectorState,
    /// Meeting apps playing audio at the current observation, if known.
    playing: Option<Vec<String>>,
}

fn find<'a>(apps: &'a [MeetingApp], id: &str) -> Option<&'a MeetingApp> {
    apps.iter().find(|a| a.id == id)
}

impl Detector {
    pub fn new(config: WatcherConfig) -> Self {
        Detector {
            config,
            state: DetectorState::Idle,
            playing: None,
        }
    }

    pub fn state(&self) -> &DetectorState {
        &self.state
    }

    /// When [`Detector::observe`] next has something to decide even if
    /// nothing changes: the end of `start_after` while pending, the end of
    /// `end_after` while releasing. `None` when only a change can matter.
    pub fn next_deadline(&self) -> Option<Instant> {
        match &self.state {
            // Held back (a browser that plays nothing): only a change can
            // start it, so no deadline, or the event loop would spin.
            DetectorState::Pending { app, .. } if !self.looks_like_a_call(app) => None,
            DetectorState::Pending { since, .. } => Some(*since + self.config.start_after),
            DetectorState::Releasing { released_at, .. } => {
                Some(*released_at + self.config.end_after)
            }
            DetectorState::Idle | DetectorState::Active { .. } => None,
        }
    }

    /// The app of the current meeting, if `Started` was emitted and `Ended`
    /// not yet (includes the release grace period).
    pub fn active(&self) -> Option<&MeetingApp> {
        match &self.state {
            DetectorState::Active { app, .. } | DetectorState::Releasing { app, .. } => Some(app),
            _ => None,
        }
    }

    /// Feed one observation: the meeting apps holding the mic at `now`,
    /// best first (as returned by [`classify_all`]). `now` must not go
    /// backwards between calls.
    pub fn observe(&mut self, now: Instant, apps: &[MeetingApp]) -> Vec<MeetingEvent> {
        self.observe_playing(now, apps, None)
    }

    /// [`Detector::observe`], also told which meeting apps play audio now
    /// (their [`MeetingApp::id`]s; `None`: not known, nothing is held back).
    ///
    /// A browser holding the mic is not yet a call: a tab can listen for
    /// dictation, a voice search or a test page, and an agent's test
    /// browser can open the mic while the user just types (2026-11-27: 11
    /// prompts in a day, none a call). A call also plays the other side, so
    /// a browser starts a meeting only once it plays audio too, or once its
    /// tab is a known call service ([`MeetingApp::call`]). Native call apps
    /// (Zoom, Slack, Teams) hold the mic only in a call and start as before.
    pub fn observe_playing(
        &mut self,
        now: Instant,
        apps: &[MeetingApp],
        playing: Option<&[&str]>,
    ) -> Vec<MeetingEvent> {
        self.playing = playing.map(|p| p.iter().map(|s| s.to_string()).collect());
        let mut events = Vec::new();
        let state = std::mem::take(&mut self.state);
        self.state = match state {
            DetectorState::Idle => self.start_fresh(now, apps, &mut events),
            DetectorState::Pending { app, since } => match find(apps, app.id) {
                Some(cur) => self.pending(cur.clone(), since, now, &mut events),
                // Released before start_after: a blip. Something else may
                // hold the mic now; it starts its own pending period.
                None => self.start_fresh(now, apps, &mut events),
            },
            DetectorState::Active { app, since } => match find(apps, app.id) {
                Some(cur) => DetectorState::Active {
                    app: cur.clone(),
                    since,
                },
                None => self.releasing(app, since, now, now, apps, &mut events),
            },
            DetectorState::Releasing {
                app,
                since,
                released_at,
            } => match find(apps, app.id) {
                // Back within the grace window: same meeting, no event.
                Some(cur) => DetectorState::Active {
                    app: cur.clone(),
                    since,
                },
                None => self.releasing(app, since, released_at, now, apps, &mut events),
            },
        };
        events
    }

    fn start_fresh(
        &self,
        now: Instant,
        apps: &[MeetingApp],
        events: &mut Vec<MeetingEvent>,
    ) -> DetectorState {
        match apps.first() {
            Some(top) => self.pending(top.clone(), now, now, events),
            None => DetectorState::Idle,
        }
    }

    fn pending(
        &self,
        app: MeetingApp,
        since: Instant,
        now: Instant,
        events: &mut Vec<MeetingEvent>,
    ) -> DetectorState {
        if now.saturating_duration_since(since) >= self.config.start_after
            && self.looks_like_a_call(&app)
        {
            events.push(MeetingEvent::Started { app: app.clone() });
            DetectorState::Active { app, since }
        } else {
            DetectorState::Pending { app, since }
        }
    }

    /// Whether `app` holding the mic is a call (see
    /// [`Detector::observe_playing`]).
    fn looks_like_a_call(&self, app: &MeetingApp) -> bool {
        if app.kind != crate::AppKind::Browser || app.call.is_some() {
            return true;
        }
        match &self.playing {
            None => true,
            Some(playing) => playing.iter().any(|id| id == app.id),
        }
    }

    fn releasing(
        &self,
        app: MeetingApp,
        since: Instant,
        released_at: Instant,
        now: Instant,
        apps: &[MeetingApp],
        events: &mut Vec<MeetingEvent>,
    ) -> DetectorState {
        if now.saturating_duration_since(released_at) >= self.config.end_after {
            let after = released_at.saturating_duration_since(since);
            events.push(MeetingEvent::Ended { app, after });
            // Whatever holds the mic now starts fresh.
            self.start_fresh(now, apps, events)
        } else {
            DetectorState::Releasing {
                app,
                since,
                released_at,
            }
        }
    }
}

/// Supplies the current audio-input clients. Implemented for closures
/// `FnMut() -> Result<Vec<AudioClient>>`; [`SystemSource`] is the real one.
pub trait ClientSource: Send + 'static {
    fn poll(&mut self) -> Result<Vec<AudioClient>>;

    /// The processes playing audio now, or `None` when the source can't
    /// tell (then nothing is held back; see [`Detector::observe_playing`]).
    fn poll_playing(&mut self) -> Option<Vec<AudioClient>> {
        None
    }
}

impl<F> ClientSource for F
where
    F: FnMut() -> Result<Vec<AudioClient>> + Send + 'static,
{
    fn poll(&mut self) -> Result<Vec<AudioClient>> {
        self()
    }
}

/// [`crate::audio_input_clients`].
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemSource;

impl ClientSource for SystemSource {
    fn poll(&mut self) -> Result<Vec<AudioClient>> {
        crate::audio_input_clients()
    }

    #[cfg(target_os = "macos")]
    fn poll_playing(&mut self) -> Option<Vec<AudioClient>> {
        crate::audio_output_clients().ok()
    }
}

/// Where an [`EventSource`] delivers the client list.
pub struct ClientSink(Box<dyn Fn(Vec<AudioClient>) -> bool + Send>);

impl ClientSink {
    /// `deliver` gets every new list and returns `false` once nobody
    /// listens any more.
    pub fn new(deliver: impl Fn(Vec<AudioClient>) -> bool + Send + 'static) -> Self {
        ClientSink(Box::new(deliver))
    }

    /// Hand over the full current list of mic clients. Returns `false`
    /// when the receiver is gone; the source should stop then.
    pub fn send(&self, clients: Vec<AudioClient>) -> bool {
        (self.0)(clients)
    }
}

/// Pushes the list of mic clients whenever it changes, from its own
/// thread, instead of being polled. Implemented for closures
/// `FnOnce(ClientSink) -> io::Result<G>`; `PwMonitor` is the real one on
/// Linux.
pub trait EventSource: Send + 'static {
    /// Stops the source when dropped.
    type Guard: Send + 'static;
    /// Start delivering to `sink`: the current list first, then each
    /// change.
    fn start(self, sink: ClientSink) -> std::io::Result<Self::Guard>;
}

impl<F, G> EventSource for F
where
    F: FnOnce(ClientSink) -> std::io::Result<G> + Send + 'static,
    G: Send + 'static,
{
    type Guard = G;
    fn start(self, sink: ClientSink) -> std::io::Result<G> {
        self(sink)
    }
}

/// Supplies "now". Implemented for closures `Fn() -> Instant`;
/// [`SystemClock`] is the real one.
pub trait Clock: Send + 'static {
    fn now(&self) -> Instant;
}

impl<F> Clock for F
where
    F: Fn() -> Instant + Send + 'static,
{
    fn now(&self) -> Instant {
        self()
    }
}

/// [`Instant::now`].
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Instant {
        Instant::now()
    }
}

/// What wakes the watcher thread.
enum Msg {
    Clients(Vec<AudioClient>),
    Stop,
}

/// What the event loop waits for: a new client list, the detector's
/// deadline, or the end.
enum Wake {
    Clients(Vec<AudioClient>),
    Deadline,
    Stop,
}

/// The event-driven loop, apart from threads and channels so tests can
/// drive it with a scripted `wait`. `wait(deadline)` blocks until a new
/// list arrives or `deadline` passes (`None`: no deadline).
fn run_events(
    config: WatcherConfig,
    own_pid: u32,
    mut wait: impl FnMut(Option<Instant>) -> Wake,
    clock: &impl Clock,
    name_calls: &mut impl FnMut(&mut [MeetingApp]),
    on_event: &mut impl FnMut(MeetingEvent),
) {
    let mut detector = Detector::new(config);
    let mut apps = Vec::new();
    loop {
        match wait(detector.next_deadline()) {
            Wake::Clients(clients) => apps = classify_all(&clients, own_pid),
            Wake::Deadline => {}
            Wake::Stop => break,
        }
        // Again at a deadline too: a tab title can change while pending.
        name_calls(&mut apps);
        for event in detector.observe(clock.now(), &apps) {
            log::info!("meeting event: {event:?}");
            on_event(event);
        }
    }
}

/// Watches on its own thread and calls back with [`MeetingEvent`]s.
///
/// The callback runs on the watcher thread; keep it short (hand work off to
/// another thread or a channel). `stop()` or dropping the watcher stops the
/// thread and joins it; no `Ended` is emitted for a meeting still active at
/// that point.
///
/// A poll that fails (e.g. [`crate::Error::Unsupported`] before macOS 14)
/// is skipped: the state is left as it was and a warning is logged once
/// per run of failures.
pub struct Watcher {
    stop_tx: Option<Sender<Msg>>,
    handle: Option<JoinHandle<()>>,
}

impl Watcher {
    /// Watch the system, ignoring `own_pid` (pass `std::process::id()`).
    /// macOS polls Core Audio every `poll_interval`.
    #[cfg(not(target_os = "linux"))]
    pub fn spawn<F>(config: WatcherConfig, own_pid: u32, on_event: F) -> std::io::Result<Self>
    where
        F: FnMut(MeetingEvent) + Send + 'static,
    {
        Self::spawn_with(config, own_pid, SystemSource, SystemClock, on_event)
    }

    /// Watch the system, ignoring `own_pid` (pass `std::process::id()`).
    /// Linux follows `pw-dump --monitor` and names browser calls from
    /// Hyprland's windows; nothing polls.
    #[cfg(target_os = "linux")]
    pub fn spawn<F>(config: WatcherConfig, own_pid: u32, on_event: F) -> std::io::Result<Self>
    where
        F: FnMut(MeetingEvent) + Send + 'static,
    {
        Self::spawn_events_with(
            config,
            own_pid,
            crate::PwMonitor,
            crate::linux::name_calls,
            SystemClock,
            on_event,
        )
    }

    /// Event-driven: `source` pushes client lists, `name_calls` may fill in
    /// [`MeetingApp::call`] before each observation. The thread blocks
    /// until a list arrives or the detector's next deadline.
    pub fn spawn_events_with<S, N, C, F>(
        config: WatcherConfig,
        own_pid: u32,
        source: S,
        mut name_calls: N,
        clock: C,
        mut on_event: F,
    ) -> std::io::Result<Self>
    where
        S: EventSource,
        N: FnMut(&mut [MeetingApp]) + Send + 'static,
        C: Clock,
        F: FnMut(MeetingEvent) + Send + 'static,
    {
        let (tx, rx) = mpsc::channel::<Msg>();
        let sink_tx = tx.clone();
        let guard = source.start(ClientSink::new(move |clients| {
            sink_tx.send(Msg::Clients(clients)).is_ok()
        }))?;
        let handle = std::thread::Builder::new()
            .name("meeting-detect".into())
            .spawn(move || {
                let _guard = guard; // stops the source when the loop ends
                log::info!("meeting watcher started");
                let wait = |deadline: Option<Instant>| {
                    let msg = match deadline {
                        None => rx.recv().map_err(|_| RecvTimeoutError::Disconnected),
                        Some(d) => rx.recv_timeout(d.saturating_duration_since(clock.now())),
                    };
                    match msg {
                        Ok(Msg::Clients(c)) => Wake::Clients(c),
                        Err(RecvTimeoutError::Timeout) => Wake::Deadline,
                        Ok(Msg::Stop) | Err(RecvTimeoutError::Disconnected) => Wake::Stop,
                    }
                };
                run_events(
                    config,
                    own_pid,
                    wait,
                    &clock,
                    &mut name_calls,
                    &mut on_event,
                );
                log::info!("meeting watcher stopped");
            })?;
        Ok(Watcher {
            stop_tx: Some(tx),
            handle: Some(handle),
        })
    }

    /// Like [`Watcher::spawn`] but delivering events on a channel.
    pub fn spawn_channel(
        config: WatcherConfig,
        own_pid: u32,
    ) -> std::io::Result<(Self, Receiver<MeetingEvent>)> {
        let (tx, rx) = mpsc::channel();
        let w = Self::spawn(config, own_pid, move |e| {
            let _ = tx.send(e);
        })?;
        Ok((w, rx))
    }

    /// Polling, fully injectable: custom client source and clock.
    pub fn spawn_with<S, C, F>(
        config: WatcherConfig,
        own_pid: u32,
        mut source: S,
        clock: C,
        mut on_event: F,
    ) -> std::io::Result<Self>
    where
        S: ClientSource,
        C: Clock,
        F: FnMut(MeetingEvent) + Send + 'static,
    {
        let (stop_tx, stop_rx) = mpsc::channel::<Msg>();
        let handle = std::thread::Builder::new()
            .name("meeting-detect".into())
            .spawn(move || {
                let mut detector = Detector::new(config);
                let mut failing = false;
                log::info!("meeting watcher started");
                loop {
                    match source.poll() {
                        Ok(clients) => {
                            failing = false;
                            #[allow(unused_mut)]
                            let mut apps = classify_all(&clients, own_pid);
                            #[cfg(target_os = "macos")]
                            crate::macos::name_calls(&mut apps);
                            let playing: Option<Vec<&str>> = source.poll_playing().map(|out| {
                                classify_all(&out, own_pid).iter().map(|a| a.id).collect()
                            });
                            let was_pending = matches!(detector.state(), DetectorState::Pending { .. });
                            let events =
                                detector.observe_playing(clock.now(), &apps, playing.as_deref());
                            if let DetectorState::Pending { app, since } = detector.state() {
                                let held = clock.now().saturating_duration_since(*since);
                                if was_pending
                                    && held >= config.start_after
                                    && held < config.start_after + config.poll_interval
                                {
                                    log::info!(
                                        "meeting watcher: {} holds the mic but plays nothing; not a call yet",
                                        app.label()
                                    );
                                }
                            }
                            for event in events {
                                log::info!("meeting event: {event:?}");
                                on_event(event);
                            }
                        }
                        Err(e) => {
                            if !failing {
                                log::warn!("meeting watcher: listing mic clients failed: {e}");
                            }
                            failing = true;
                        }
                    }
                    match stop_rx.recv_timeout(config.poll_interval) {
                        Err(RecvTimeoutError::Timeout) => continue,
                        Ok(_) | Err(RecvTimeoutError::Disconnected) => break,
                    }
                }
                log::info!("meeting watcher stopped");
            })?;
        Ok(Watcher {
            stop_tx: Some(stop_tx),
            handle: Some(handle),
        })
    }

    /// Stop watching and join the thread.
    pub fn stop(mut self) {
        self.shutdown();
    }

    fn shutdown(&mut self) {
        if let Some(tx) = self.stop_tx.take() {
            let _ = tx.send(Msg::Stop);
        }
        if let Some(h) = self.handle.take() {
            // Don't deadlock if the callback itself dropped the watcher.
            if h.thread().id() != std::thread::current().id() {
                let _ = h.join();
            }
        }
    }
}

impl Drop for Watcher {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AppKind, MatchedBy};
    use std::sync::{Arc, Mutex};

    const S: Duration = Duration::from_secs(1);

    fn app(id: &'static str) -> MeetingApp {
        MeetingApp {
            id,
            name: id,
            kind: AppKind::Native,
            pid: 1,
            bundle_id: None,
            exe_path: None,
            matched_by: MatchedBy::BundleId,
            call: None,
        }
    }

    fn browser(call: Option<&str>) -> MeetingApp {
        MeetingApp {
            kind: AppKind::Browser,
            call: call.map(String::from),
            ..app("chrome")
        }
    }

    /// Every 2 s for `secs`, the given app holds the mic; `playing(t)` says
    /// which apps play audio at t. Returns the times of `Started`.
    fn starts(app: MeetingApp, secs: u64, playing: impl Fn(u64) -> Vec<&'static str>) -> Vec<u64> {
        let t0 = Instant::now();
        let mut d = Detector::new(WatcherConfig::default());
        let mut out = Vec::new();
        for t in (0..=secs).step_by(2) {
            let p = playing(t);
            for e in d.observe_playing(t0 + S * t as u32, std::slice::from_ref(&app), Some(&p)) {
                if matches!(e, MeetingEvent::Started { .. }) {
                    out.push(t);
                }
            }
        }
        out
    }

    #[test]
    fn browser_holding_the_mic_while_nothing_plays_is_no_call() {
        // The 2026-11-27 case: Chrome holds the mic for a minute while the
        // user types in other apps; Chrome plays nothing. No prompt.
        assert_eq!(starts(browser(None), 600, |_| vec![]), Vec::<u64>::new());
        // Something else playing doesn't make Chrome's mic a call.
        assert_eq!(
            starts(browser(None), 60, |_| vec!["slack"]),
            Vec::<u64>::new()
        );
    }

    #[test]
    fn browser_call_starts_once_the_other_side_plays() {
        // Joining a call in the browser: the mic first, the call audio a
        // few seconds later. Starts on the first poll that has both.
        assert_eq!(
            starts(browser(None), 30, |t| if t >= 10 {
                vec!["chrome"]
            } else {
                vec![]
            }),
            vec![10]
        );
        // A call that plays from the start starts as before (3 s, 4th s poll).
        assert_eq!(starts(browser(None), 30, |_| vec!["chrome"]), vec![4]);
    }

    #[test]
    fn named_browser_call_needs_no_sound() {
        assert_eq!(
            starts(browser(Some("Google Meet")), 30, |_| vec![]),
            vec![4]
        );
    }

    #[test]
    fn native_call_app_needs_no_sound() {
        assert_eq!(starts(app("zoom"), 30, |_| vec![]), vec![4]);
    }

    #[test]
    fn held_back_browser_sets_no_deadline() {
        // The event-driven loop waits for the deadline: a held-back browser
        // must not leave one in the past, or it would spin.
        let t0 = Instant::now();
        let mut d = Detector::new(WatcherConfig::default());
        let apps = [browser(None)];
        d.observe_playing(t0, &apps, Some(&[]));
        d.observe_playing(t0 + S * 10, &apps, Some(&[]));
        assert!(matches!(d.state(), DetectorState::Pending { .. }));
        assert_eq!(d.next_deadline(), None);
    }

    #[test]
    fn unknown_playing_holds_nothing_back() {
        let t0 = Instant::now();
        let mut d = Detector::new(WatcherConfig::default());
        let apps = [browser(None)];
        d.observe(t0, &apps);
        let events = d.observe(t0 + S * 4, &apps);
        assert!(matches!(events.as_slice(), [MeetingEvent::Started { .. }]));
    }

    /// Run a script of (seconds since start, apps holding the mic) through
    /// a default-config detector; return (time, event) pairs.
    fn run(script: &[(u64, &[&'static str])]) -> Vec<(u64, MeetingEvent)> {
        let t0 = Instant::now();
        let mut d = Detector::new(WatcherConfig::default());
        let mut out = Vec::new();
        for (t, ids) in script {
            let apps: Vec<_> = ids.iter().map(|id| app(id)).collect();
            for e in d.observe(t0 + S * *t as u32, &apps) {
                out.push((*t, e));
            }
        }
        out
    }

    /// Poll every 2 s from `from` to `to` inclusive with the same apps.
    fn every2(
        from: u64,
        to: u64,
        ids: &'static [&'static str],
    ) -> Vec<(u64, &'static [&'static str])> {
        (from..=to).step_by(2).map(|t| (t, ids)).collect()
    }

    fn started(id: &'static str) -> MeetingEvent {
        MeetingEvent::Started { app: app(id) }
    }

    fn ended(id: &'static str, secs: u64) -> MeetingEvent {
        MeetingEvent::Ended {
            app: app(id),
            after: Duration::from_secs(secs),
        }
    }

    #[test]
    fn blip_shorter_than_start_after_emits_nothing() {
        // Chrome probes the mic for ~2 s (seen at t=2, gone at t=4).
        let mut script = every2(0, 0, &[]);
        script.extend(every2(2, 2, &["chrome"]));
        script.extend(every2(4, 60, &[]));
        assert!(run(&script).is_empty());
    }

    #[test]
    fn starts_once_held_for_start_after() {
        let script = every2(0, 10, &["zoom"]);
        // seen at 0 (0 s), 2 (2 s < 3 s), 4 (4 s >= 3 s) -> Started at t=4.
        assert_eq!(run(&script), vec![(4, started("zoom"))]);
    }

    #[test]
    fn short_release_then_reacquire_is_one_meeting() {
        let mut script = every2(0, 100, &["zoom"]);
        script.extend(every2(102, 114, &[])); // 12 s gap < 20 s
        script.extend(every2(116, 200, &["zoom"]));
        script.extend(every2(202, 260, &[]));
        let events = run(&script);
        assert_eq!(
            events,
            vec![(4, started("zoom")), (222, ended("zoom", 202))],
            "one Started, one Ended; the gap does not split the meeting"
        );
    }

    #[test]
    fn long_release_ends_exactly_once() {
        let mut script = every2(0, 60, &["zoom"]);
        script.extend(every2(62, 300, &[]));
        let events = run(&script);
        // Released at t=62 (first poll without zoom); 20 s later -> t=82.
        assert_eq!(events, vec![(4, started("zoom")), (82, ended("zoom", 62))]);
    }

    #[test]
    fn app_switch_ends_first_then_starts_second() {
        let mut script = every2(0, 60, &["zoom"]);
        script.extend(every2(62, 200, &["chrome"]));
        let events = run(&script);
        assert_eq!(
            events,
            vec![
                (4, started("zoom")),
                // Zoom's grace runs out at 82; Chrome starts pending then
                // and passes start_after at 86.
                (82, ended("zoom", 62)),
                (86, started("chrome")),
            ]
        );
    }

    #[test]
    fn other_app_during_meeting_is_ignored() {
        let mut script = every2(0, 30, &["zoom"]);
        script.extend(every2(32, 60, &["zoom", "chrome"]));
        script.extend(every2(62, 80, &["zoom"]));
        assert_eq!(run(&script), vec![(4, started("zoom"))]);
    }

    #[test]
    fn pending_app_disappearing_hands_over_to_the_next() {
        // Chrome blips, Zoom appears as Chrome goes: Zoom gets its own
        // full start_after.
        let script: Vec<(u64, &[&str])> = vec![
            (0, &["chrome"]),
            (2, &["zoom"]),
            (4, &["zoom"]),
            (6, &["zoom"]),
        ];
        assert_eq!(run(&script), vec![(6, started("zoom"))]);
    }

    #[test]
    fn zero_delays_fire_immediately() {
        let mut d = Detector::new(WatcherConfig {
            poll_interval: S,
            start_after: Duration::ZERO,
            end_after: Duration::ZERO,
        });
        let t0 = Instant::now();
        assert_eq!(d.observe(t0, &[app("zoom")]), vec![started("zoom")]);
        assert_eq!(d.observe(t0 + S, &[]), vec![ended("zoom", 1)]);
        assert_eq!(d.state(), &DetectorState::Idle);
    }

    #[test]
    fn active_reports_through_grace_period() {
        let mut d = Detector::new(WatcherConfig::default());
        let t0 = Instant::now();
        d.observe(t0, &[app("zoom")]);
        assert!(d.active().is_none(), "pending is not active yet");
        d.observe(t0 + S * 4, &[app("zoom")]);
        assert_eq!(d.active().map(|a| a.id), Some("zoom"));
        d.observe(t0 + S * 6, &[]);
        assert_eq!(d.active().map(|a| a.id), Some("zoom"));
        d.observe(t0 + S * 26, &[]);
        assert!(d.active().is_none());
    }

    // ---- the threaded Watcher with a fake source and a fake clock ----

    fn client(id: &str) -> AudioClient {
        match id {
            "zoom" => AudioClient::new(100, Some("us.zoom.xos"), None),
            "chrome" => AudioClient::new(200, Some("com.google.Chrome.helper"), None),
            "own" => AudioClient::new(999, Some("us.zoom.xos"), None),
            _ => unreachable!(),
        }
    }

    /// Each poll advances the fake clock by 2 s and returns the next scripted
    /// observation. Real sleeping is 1 ms per poll.
    fn run_watcher(script: Vec<Vec<&'static str>>) -> Vec<(u64, MeetingEvent)> {
        let t0 = Instant::now();
        let now = Arc::new(Mutex::new(t0));
        let (done_tx, done_rx) = mpsc::channel();
        let mut steps = script.into_iter();
        let mut first = true;
        let source = {
            let now = now.clone();
            move || -> Result<Vec<AudioClient>> {
                if !first {
                    *now.lock().unwrap() += S * 2;
                }
                first = false;
                match steps.next() {
                    Some(ids) => Ok(ids.into_iter().map(client).collect()),
                    None => {
                        let _ = done_tx.send(());
                        Err(crate::Error::Unsupported) // skipped tick
                    }
                }
            }
        };
        let clock = {
            let now = now.clone();
            move || *now.lock().unwrap()
        };
        let events = Arc::new(Mutex::new(Vec::new()));
        let sink = events.clone();
        let clock_for_events = now.clone();
        let config = WatcherConfig {
            poll_interval: Duration::from_millis(1),
            ..WatcherConfig::default()
        };
        let w = Watcher::spawn_with(config, 999, source, clock, move |e| {
            let t = clock_for_events
                .lock()
                .unwrap()
                .duration_since(t0)
                .as_secs();
            sink.lock().unwrap().push((t, e));
        })
        .unwrap();
        done_rx
            .recv_timeout(Duration::from_secs(10))
            .expect("script finished");
        w.stop();
        let out = events.lock().unwrap().clone();
        out
    }

    fn ids(events: &[(u64, MeetingEvent)]) -> Vec<(u64, &'static str, &'static str)> {
        events
            .iter()
            .map(|(t, e)| match e {
                MeetingEvent::Started { app } => (*t, "started", app.id),
                MeetingEvent::Ended { app, .. } => (*t, "ended", app.id),
            })
            .collect()
    }

    #[test]
    fn watcher_thread_debounces_with_fake_clock() {
        // t=0..20 zoom, t=22..30 nothing (short gap), t=32..40 zoom,
        // t=42..80 nothing.
        let mut script = Vec::new();
        script.extend((0..=20).step_by(2).map(|_| vec!["zoom"]));
        script.extend((22..=30).step_by(2).map(|_| vec![]));
        script.extend((32..=40).step_by(2).map(|_| vec!["zoom"]));
        script.extend((42..=80).step_by(2).map(|_| vec![]));
        let events = run_watcher(script);
        assert_eq!(
            ids(&events),
            vec![(4, "started", "zoom"), (62, "ended", "zoom")]
        );
        match &events[1].1 {
            MeetingEvent::Ended { after, .. } => assert_eq!(*after, Duration::from_secs(42)),
            e => panic!("{e:?}"),
        }
    }

    #[test]
    fn watcher_ignores_own_pid_and_prefers_native() {
        let mut script = Vec::new();
        script.extend((0..=10).step_by(2).map(|_| vec!["own"]));
        script.extend((12..=20).step_by(2).map(|_| vec!["chrome", "zoom"]));
        let events = run_watcher(script);
        assert_eq!(ids(&events), vec![(16, "started", "zoom")]);
    }

    #[test]
    fn next_deadline_follows_the_state() {
        let mut d = Detector::new(WatcherConfig::default());
        let t0 = Instant::now();
        assert_eq!(d.next_deadline(), None);
        d.observe(t0, &[app("zoom")]);
        assert_eq!(d.next_deadline(), Some(t0 + S * 3));
        d.observe(t0 + S * 3, &[app("zoom")]);
        assert_eq!(d.next_deadline(), None, "active: only a change matters");
        d.observe(t0 + S * 10, &[]);
        assert_eq!(d.next_deadline(), Some(t0 + S * 30));
    }

    // ---- the event-driven loop with a scripted source and a fake clock ----

    fn linux_client(id: &str) -> AudioClient {
        match id {
            "zoom" => AudioClient::new(100, None, Some("/opt/zoom/zoom")),
            "chrome" => AudioClient::new(200, None, Some("/opt/google/chrome/chrome")),
            "own" => AudioClient::new(999, None, Some("/opt/zoom/zoom")),
            _ => unreachable!(),
        }
    }

    type Wakes = Vec<(u64, &'static str)>;

    /// Runs the event loop on a script of (second, apps holding the mic),
    /// one entry per change the source pushes. Waiting jumps the fake clock
    /// to the next change or to the detector's deadline, whichever comes
    /// first, as `recv_timeout` would. Once nothing is left and there is no
    /// deadline, the loop would block for good; the script ends there.
    /// Returns the events and every wakeup.
    fn run_script(
        script: &[(u64, &[&'static str])],
        mut name_calls: impl FnMut(&mut [MeetingApp]),
    ) -> (Vec<(u64, MeetingEvent)>, Wakes) {
        let t0 = Instant::now();
        let now = Arc::new(Mutex::new(t0));
        let secs = |t: Instant| t.duration_since(t0).as_secs();
        let mut changes = script.iter().peekable();
        let mut wakes = Vec::new();
        let wait = |deadline: Option<Instant>| {
            assert!(wakes.len() < 100, "loop does not settle");
            let mut now = now.lock().unwrap();
            let next = changes.peek().map(|(t, _)| t0 + S * *t as u32);
            match (next, deadline) {
                (Some(n), Some(d)) if d <= n => {}
                (Some(n), _) => {
                    *now = n;
                    wakes.push((secs(n), "change"));
                    let (_, ids) = changes.next().unwrap();
                    return Wake::Clients(ids.iter().map(|id| linux_client(id)).collect());
                }
                (None, Some(_)) => {}
                (None, None) => return Wake::Stop,
            }
            let d = deadline.unwrap().max(*now);
            *now = d;
            wakes.push((secs(d), "deadline"));
            Wake::Deadline
        };
        let clock = {
            let now = now.clone();
            move || *now.lock().unwrap()
        };
        let mut events = Vec::new();
        let mut on_event = |e| events.push((secs(clock()), e));
        let config = WatcherConfig::default();
        run_events(config, 999, wait, &clock, &mut name_calls, &mut on_event);
        (events, wakes)
    }

    fn no_names(_: &mut [MeetingApp]) {}

    #[test]
    fn events_fire_at_deadlines_without_polling() {
        let (events, wakes) = run_script(&[(0, &["zoom"]), (100, &[])], no_names);
        assert_eq!(
            ids(&events),
            vec![(3, "started", "zoom"), (120, "ended", "zoom")]
        );
        match &events[1].1 {
            MeetingEvent::Ended { after, .. } => assert_eq!(*after, S * 100),
            e => panic!("{e:?}"),
        }
        // Two changes, two deadlines, nothing else in 120 s.
        assert_eq!(
            wakes,
            [
                (0, "change"),
                (3, "deadline"),
                (100, "change"),
                (120, "deadline")
            ]
        );
    }

    #[test]
    fn event_blip_needs_no_deadline_wakeup() {
        let (events, wakes) = run_script(&[(0, &["chrome"]), (2, &[])], no_names);
        assert!(events.is_empty());
        assert_eq!(wakes, [(0, "change"), (2, "change")]);
    }

    #[test]
    fn event_reacquire_within_grace_is_one_meeting() {
        let script: &[(u64, &[&str])] = &[(0, &["zoom"]), (50, &[]), (60, &["zoom"]), (200, &[])];
        let (events, wakes) = run_script(script, no_names);
        assert_eq!(
            ids(&events),
            vec![(3, "started", "zoom"), (220, "ended", "zoom")]
        );
        assert_eq!(wakes.len(), 6, "{wakes:?}");
    }

    #[test]
    fn event_app_switch_ends_then_starts() {
        let (events, _) = run_script(&[(0, &["zoom"]), (60, &["chrome"])], no_names);
        assert_eq!(
            ids(&events),
            vec![
                (3, "started", "zoom"),
                (80, "ended", "zoom"),
                (83, "started", "chrome"),
            ]
        );
    }

    #[test]
    fn event_loop_ignores_own_pid_and_names_calls() {
        let mut namings = 0;
        let namer = |apps: &mut [MeetingApp]| {
            namings += 1;
            for a in apps.iter_mut().filter(|a| a.kind == AppKind::Browser) {
                a.call = Some("Google Meet".into());
            }
        };
        let (events, _) = run_script(&[(0, &["own"]), (10, &["chrome"])], namer);
        assert_eq!(ids(&events), vec![(13, "started", "chrome")]);
        match &events[0].1 {
            MeetingEvent::Started { app } => assert_eq!(app.label(), "Google Meet"),
            e => panic!("{e:?}"),
        }
        assert_eq!(namings, 3, "each change and the deadline");
    }

    #[test]
    fn event_watcher_thread_delivers_and_stops_its_source() {
        struct Guard(Arc<Mutex<bool>>);
        impl Drop for Guard {
            fn drop(&mut self) {
                *self.0.lock().unwrap() = true;
            }
        }
        let stopped = Arc::new(Mutex::new(false));
        let flag = stopped.clone();
        let source = move |sink: ClientSink| {
            assert!(sink.send(vec![linux_client("zoom")]));
            assert!(sink.send(vec![]));
            Ok(Guard(flag))
        };
        let config = WatcherConfig {
            start_after: Duration::ZERO,
            end_after: Duration::ZERO,
            ..WatcherConfig::default()
        };
        let (tx, rx) = mpsc::channel();
        let w = Watcher::spawn_events_with(config, 999, source, no_names, SystemClock, move |e| {
            let _ = tx.send(e);
        })
        .unwrap();
        let first = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let second = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(matches!(first, MeetingEvent::Started { ref app } if app.id == "zoom"));
        assert!(matches!(second, MeetingEvent::Ended { ref app, .. } if app.id == "zoom"));
        assert!(!*stopped.lock().unwrap());
        w.stop();
        assert!(*stopped.lock().unwrap(), "source guard dropped on stop");
    }

    #[test]
    fn dropping_watcher_joins_thread() {
        let polls = Arc::new(Mutex::new(0u32));
        let p = polls.clone();
        let w = Watcher::spawn_with(
            WatcherConfig {
                poll_interval: Duration::from_millis(1),
                ..WatcherConfig::default()
            },
            0,
            move || -> Result<Vec<AudioClient>> {
                *p.lock().unwrap() += 1;
                Ok(vec![])
            },
            SystemClock,
            |_| {},
        )
        .unwrap();
        std::thread::sleep(Duration::from_millis(20));
        drop(w);
        let after_drop = *polls.lock().unwrap();
        std::thread::sleep(Duration::from_millis(20));
        assert_eq!(*polls.lock().unwrap(), after_drop, "no polls after drop");
        assert!(after_drop > 0);
    }
}
