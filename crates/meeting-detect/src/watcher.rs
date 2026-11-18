//! Debounced meeting start/end events.
//!
//! [`Detector`] is the pure state machine: feed it "these meeting apps hold
//! the mic now" plus a timestamp, get events back. [`Watcher`] runs it on a
//! background thread against Core Audio (or an injected [`ClientSource`] and
//! [`Clock`], for tests).

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
    /// How often Core Audio is polled. Default 2 s.
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
}

fn find<'a>(apps: &'a [MeetingApp], id: &str) -> Option<&'a MeetingApp> {
    apps.iter().find(|a| a.id == id)
}

impl Detector {
    pub fn new(config: WatcherConfig) -> Self {
        Detector {
            config,
            state: DetectorState::Idle,
        }
    }

    pub fn state(&self) -> &DetectorState {
        &self.state
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
        if now.saturating_duration_since(since) >= self.config.start_after {
            events.push(MeetingEvent::Started { app: app.clone() });
            DetectorState::Active { app, since }
        } else {
            DetectorState::Pending { app, since }
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

/// Polls on its own thread and calls back with [`MeetingEvent`]s.
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
    stop_tx: Option<Sender<()>>,
    handle: Option<JoinHandle<()>>,
}

impl Watcher {
    /// Watch Core Audio, ignoring `own_pid` (pass `std::process::id()`).
    pub fn spawn<F>(config: WatcherConfig, own_pid: u32, on_event: F) -> std::io::Result<Self>
    where
        F: FnMut(MeetingEvent) + Send + 'static,
    {
        Self::spawn_with(config, own_pid, SystemSource, SystemClock, on_event)
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

    /// Fully injectable variant for tests: custom client source and clock.
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
        let (stop_tx, stop_rx) = mpsc::channel::<()>();
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
                            let apps = classify_all(&clients, own_pid);
                            for event in detector.observe(clock.now(), &apps) {
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
                        Ok(()) | Err(RecvTimeoutError::Disconnected) => break,
                    }
                }
                log::info!("meeting watcher stopped");
            })?;
        Ok(Watcher {
            stop_tx: Some(stop_tx),
            handle: Some(handle),
        })
    }

    /// Stop polling and join the thread.
    pub fn stop(mut self) {
        self.shutdown();
    }

    fn shutdown(&mut self) {
        if let Some(tx) = self.stop_tx.take() {
            let _ = tx.send(());
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
        }
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
