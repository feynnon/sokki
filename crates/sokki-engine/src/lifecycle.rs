//! The meeting lifecycle as a pure state machine: idle → asking →
//! recording → idle. No threads, no clock, no audio; the host feeds it
//! inputs with the current time and carries out the effects it returns.
//!
//! The rule it exists for: **recording starts only from a yes.** A call
//! starting leads to a question; only `Yes`, `Start` or `Toggle` (a person
//! acting) leads to `StartRecording`. An unanswered question counts as no.

use std::time::{Duration, Instant};

/// A call app as detection reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallApp {
    /// Stable id: two reports with the same id are the same call.
    pub id: String,
    /// What the person sees ("Google Meet", "Zoom").
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Phase {
    Idle,
    Asking {
        app: CallApp,
        since: Instant,
    },
    Recording {
        /// The call being recorded; `None` for a manual recording.
        app: Option<CallApp>,
        since: Instant,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Input {
    /// Detection: a call app has held the mic long enough.
    CallStarted(CallApp),
    /// Detection: that app let go of the mic (after its grace period).
    CallEnded { id: String },
    /// The person said yes to the question.
    Yes,
    /// The person said no.
    No,
    /// Manual start (shortcut, menu, CLI).
    Start,
    /// Manual stop.
    Stop,
    /// Start if idle, yes if asking, stop if recording.
    Toggle,
    /// Time passed; lets the question expire.
    Tick,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// Show the question for this call.
    Ask(CallApp),
    /// Take the question away.
    CloseAsk,
    StartRecording { app: Option<CallApp> },
    /// Stop and hand the recording to processing.
    StopRecording,
}

#[derive(Debug, Clone)]
pub struct Lifecycle {
    phase: Phase,
    /// The call the person declined (or stopped recording): not asked
    /// again until it ends.
    declined: Option<String>,
    ask_timeout: Duration,
}

impl Lifecycle {
    /// `ask_timeout`: how long the question waits before it counts as no.
    pub fn new(ask_timeout: Duration) -> Self {
        Lifecycle {
            phase: Phase::Idle,
            declined: None,
            ask_timeout,
        }
    }

    pub fn phase(&self) -> &Phase {
        &self.phase
    }

    pub fn ask_timeout(&self) -> Duration {
        self.ask_timeout
    }

    /// When the host must send `Tick` next (the question's expiry), if ever.
    pub fn next_deadline(&self) -> Option<Instant> {
        match &self.phase {
            Phase::Asking { since, .. } => Some(*since + self.ask_timeout),
            _ => None,
        }
    }

    pub fn handle(&mut self, now: Instant, input: Input) -> Vec<Effect> {
        use Effect::*;
        let phase = std::mem::replace(&mut self.phase, Phase::Idle);
        let (next, effects) = match (phase, input) {
            // A call starts: ask, unless the person already said no to it.
            (Phase::Idle, Input::CallStarted(app)) => {
                if self.declined.as_deref() == Some(app.id.as_str()) {
                    (Phase::Idle, vec![])
                } else {
                    (
                        Phase::Asking {
                            app: app.clone(),
                            since: now,
                        },
                        vec![Ask(app)],
                    )
                }
            }
            // Yes (or a manual start or toggle while asking): record this call.
            (Phase::Asking { app, .. }, Input::Yes | Input::Start | Input::Toggle) => (
                Phase::Recording {
                    app: Some(app.clone()),
                    since: now,
                },
                vec![CloseAsk, StartRecording { app: Some(app) }],
            ),
            (Phase::Asking { app, .. }, Input::No) => {
                self.declined = Some(app.id);
                (Phase::Idle, vec![CloseAsk])
            }
            (Phase::Asking { app, since }, Input::Tick) => {
                if now >= since + self.ask_timeout {
                    self.declined = Some(app.id);
                    (Phase::Idle, vec![CloseAsk])
                } else {
                    (Phase::Asking { app, since }, vec![])
                }
            }
            (Phase::Asking { app, since }, Input::CallEnded { id }) => {
                if app.id == id {
                    (Phase::Idle, vec![CloseAsk])
                } else {
                    (Phase::Asking { app, since }, vec![])
                }
            }
            // Manual recording.
            (Phase::Idle, Input::Start | Input::Toggle) => (
                Phase::Recording {
                    app: None,
                    since: now,
                },
                vec![StartRecording { app: None }],
            ),
            (Phase::Recording { app, .. }, Input::Stop | Input::Toggle) => {
                // Stopping by hand during a call: don't ask about it again.
                if let Some(app) = app {
                    self.declined = Some(app.id);
                }
                (Phase::Idle, vec![StopRecording])
            }
            // The recorded call ended: stop by itself.
            (Phase::Recording { app: Some(app), since }, Input::CallEnded { id }) => {
                if app.id == id {
                    (Phase::Idle, vec![StopRecording])
                } else {
                    (
                        Phase::Recording {
                            app: Some(app),
                            since,
                        },
                        vec![],
                    )
                }
            }
            // Everything else changes nothing, except that a declined call
            // that ends is forgotten.
            (phase, input) => {
                if let Input::CallEnded { id } = &input {
                    if self.declined.as_deref() == Some(id.as_str()) {
                        self.declined = None;
                    }
                }
                (phase, vec![])
            }
        };
        self.phase = next;
        effects
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meet() -> CallApp {
        CallApp {
            id: "chrome".into(),
            label: "Google Meet".into(),
        }
    }

    fn lc() -> (Lifecycle, Instant) {
        (Lifecycle::new(Duration::from_secs(60)), Instant::now())
    }

    fn records(effects: &[Effect]) -> bool {
        effects
            .iter()
            .any(|e| matches!(e, Effect::StartRecording { .. }))
    }

    #[test]
    fn a_call_asks_and_never_records_by_itself() {
        let (mut l, t) = lc();
        let e = l.handle(t, Input::CallStarted(meet()));
        assert_eq!(e, vec![Effect::Ask(meet())]);
        // Time passes, the call ends: never a recording.
        assert!(!records(&l.handle(t + Duration::from_secs(30), Input::Tick)));
        let e = l.handle(t + Duration::from_secs(61), Input::Tick);
        assert_eq!(e, vec![Effect::CloseAsk]);
        assert_eq!(l.phase(), &Phase::Idle);
        assert!(!records(&l.handle(
            t + Duration::from_secs(90),
            Input::CallEnded { id: "chrome".into() }
        )));
    }

    #[test]
    fn yes_records_and_the_call_ending_stops() {
        let (mut l, t) = lc();
        l.handle(t, Input::CallStarted(meet()));
        let e = l.handle(t, Input::Yes);
        assert_eq!(
            e,
            vec![
                Effect::CloseAsk,
                Effect::StartRecording { app: Some(meet()) }
            ]
        );
        assert!(l.next_deadline().is_none());
        let e = l.handle(t, Input::CallEnded { id: "chrome".into() });
        assert_eq!(e, vec![Effect::StopRecording]);
        assert_eq!(l.phase(), &Phase::Idle);
    }

    #[test]
    fn no_is_remembered_until_the_call_ends() {
        let (mut l, t) = lc();
        l.handle(t, Input::CallStarted(meet()));
        assert_eq!(l.handle(t, Input::No), vec![Effect::CloseAsk]);
        // The same call reported again (a helper restarted): no new question.
        assert!(l.handle(t, Input::CallStarted(meet())).is_empty());
        l.handle(t, Input::CallEnded { id: "chrome".into() });
        // The next call is asked about again.
        assert_eq!(
            l.handle(t, Input::CallStarted(meet())),
            vec![Effect::Ask(meet())]
        );
    }

    #[test]
    fn manual_recording_ignores_calls_and_stops_by_hand() {
        let (mut l, t) = lc();
        assert_eq!(
            l.handle(t, Input::Toggle),
            vec![Effect::StartRecording { app: None }]
        );
        assert!(l.handle(t, Input::CallStarted(meet())).is_empty());
        assert!(l
            .handle(t, Input::CallEnded { id: "chrome".into() })
            .is_empty());
        assert_eq!(l.handle(t, Input::Toggle), vec![Effect::StopRecording]);
    }

    #[test]
    fn stopping_a_call_by_hand_does_not_ask_again() {
        let (mut l, t) = lc();
        l.handle(t, Input::CallStarted(meet()));
        l.handle(t, Input::Yes);
        assert_eq!(l.handle(t, Input::Stop), vec![Effect::StopRecording]);
        assert!(l.handle(t, Input::CallStarted(meet())).is_empty());
    }

    #[test]
    fn another_app_ending_changes_nothing() {
        let (mut l, t) = lc();
        l.handle(t, Input::CallStarted(meet()));
        assert!(l.handle(t, Input::CallEnded { id: "zoom".into() }).is_empty());
        assert!(matches!(l.phase(), Phase::Asking { .. }));
    }
}
