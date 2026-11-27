//! The runner's thread: takes events, queues a job per matching hook,
//! fires schedules, and runs one job at a time whenever no meeting needs
//! the computer.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, PoisonError};
use std::time::{Duration, Instant};

use super::config::{self, Config};
use super::content;
use super::event::{HookEvent, ScheduleInfo, DICTATION_DONE, SCHEDULE};
use super::exec;
use super::store::{now_ms, Job, Store, KEEP_RUNS};
use super::{Gate, Host, Report, Shared};

/// How often schedules are checked, and the longest sleep.
const TICK: Duration = Duration::from_secs(20);
/// While a meeting needs the computer.
const BUSY_WAIT: Duration = Duration::from_secs(5);

struct Runner {
    host: Host,
    store: Store,
    /// Reported once per hook and hash, not at every event.
    told_not_allowed: HashSet<(String, String)>,
    told_config: Option<String>,
    last_schedule_check: Option<Instant>,
    runs_since_prune: usize,
}

/// One job per event and matching hook.
pub(super) fn queue_into(store: &Store, cfg: &Config, events: Vec<HookEvent>) {
    for mut event in events {
        content::complete_meta(&mut event);
        let whole = event.to_json();
        for hook in cfg.for_event(&event.event, &event.meta) {
            // A dictation's text is kept in the queue only for hooks that
            // may read it. Meeting content is read when the hook runs, so
            // it is never stored here.
            let scoped;
            let payload = if event.event == DICTATION_DONE {
                scoped = content::for_scope(&event, &hook.scope).to_json();
                &scoped
            } else {
                &whole
            };
            match store.enqueue(&hook.name, &event.event, &event.id, payload) {
                Ok(job) => log::info!(
                    "Hooks: queued {} for {} {} (job {job})",
                    hook.name,
                    event.event,
                    event.id
                ),
                Err(e) => log::warn!("Hooks: queueing {} failed: {e:#}", hook.name),
            }
        }
    }
}

pub(super) fn run(host: Host, store: Store, shared: Arc<Shared>) {
    let mut r = Runner {
        host,
        store,
        told_not_allowed: HashSet::new(),
        told_config: None,
        last_schedule_check: None,
        runs_since_prune: 0,
    };
    loop {
        let mut events: Vec<HookEvent> = shared
            .incoming
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .drain(..)
            .collect();
        {
            let mut waiting = shared
                .debounced
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            let now = Instant::now();
            let due: Vec<String> = waiting
                .iter()
                .filter(|(_, (at, _))| *at <= now)
                .map(|(k, _)| k.clone())
                .collect();
            for k in due {
                if let Some((_, e)) = waiting.remove(&k) {
                    super::tell_listeners(&e);
                    events.push(e);
                }
            }
        }
        if !events.is_empty() {
            r.queue(events);
        }
        if r.last_schedule_check.is_none_or(|t| t.elapsed() >= TICK) {
            r.last_schedule_check = Some(Instant::now());
            r.schedules();
        }
        if (r.host.gate)() != Gate::Free {
            shared.sleep(BUSY_WAIT);
            continue;
        }
        match r.store.next_due(now_ms()) {
            Ok(Some(job)) => {
                r.run_job(job);
                continue;
            }
            Ok(None) => {}
            Err(e) => log::warn!("Hooks: reading the queue failed: {e:#}"),
        }
        let mut sleep = TICK;
        if let Ok(Some(next)) = r.store.next_wake() {
            let ms = (next - now_ms()).max(0) as u64;
            sleep = sleep.min(Duration::from_millis(ms.max(100)));
        }
        if let Some(first) = shared
            .debounced
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .values()
            .map(|(at, _)| *at)
            .min()
        {
            sleep = sleep.min(
                first
                    .saturating_duration_since(Instant::now())
                    .max(Duration::from_millis(100)),
            );
        }
        shared.sleep(sleep);
    }
}

impl Runner {
    fn config(&mut self) -> Option<Config> {
        match config::load(&self.host.paths.config_dir) {
            Ok(c) => {
                self.told_config = None;
                Some(c)
            }
            Err(e) => {
                let error = format!("{e:#}");
                if self.told_config.as_deref() != Some(&error) {
                    log::warn!("Hooks: {error}");
                    (self.host.report)(Report::ConfigInvalid {
                        error: error.clone(),
                    });
                    self.told_config = Some(error);
                }
                None
            }
        }
    }

    fn queue(&mut self, events: Vec<HookEvent>) {
        let Some(cfg) = self.config() else { return };
        queue_into(&self.store, &cfg, events);
    }

    /// Queues each schedule that came due since it last did. A missed one
    /// (the computer was asleep, Sokki was closed) runs once when it can.
    /// A new hook starts from now: it does not catch up on its past.
    fn schedules(&mut self) {
        let Some(cfg) = self.config() else { return };
        let now = chrono::Local::now();
        for hook in cfg.hooks.iter().filter(|h| h.enabled && h.on == SCHEDULE) {
            let Some(cron) = &hook.cron else { continue };
            let Some(due) = cron.latest_due(now) else {
                continue;
            };
            let due_text = due.to_rfc3339_opts(chrono::SecondsFormat::Secs, false);
            let last = match self.store.last_due(&hook.name) {
                Ok(l) => l,
                Err(e) => {
                    log::warn!("Hooks: {e:#}");
                    continue;
                }
            };
            let fire = match &last {
                None => false,
                Some(prev) => chrono::DateTime::parse_from_rfc3339(prev)
                    .map(|p| p < due)
                    .unwrap_or(true),
            };
            if last.is_none() || fire {
                let _ = self.store.set_last_due(&hook.name, &due_text);
            }
            if !fire {
                continue;
            }
            let event = HookEvent {
                schedule: Some(ScheduleInfo {
                    cron: cron.text.clone(),
                    due: due_text.clone(),
                    previous: last,
                }),
                ..HookEvent::new(SCHEDULE, format!("schedule:{}", hook.name))
            };
            super::tell_listeners(&event);
            match self
                .store
                .enqueue(&hook.name, SCHEDULE, &event.id, &event.to_json())
            {
                Ok(job) => log::info!("Hooks: {} is due ({due_text}), job {job}", hook.name),
                Err(e) => log::warn!("Hooks: queueing {} failed: {e:#}", hook.name),
            }
        }
    }

    fn log_path(&self, job: &Job) -> PathBuf {
        let stamp = chrono::Local::now().format("%Y-%m-%d_%H-%M-%S");
        self.host.paths.logs_dir().join(format!(
            "{stamp}_{}_{}-{}.log",
            job.hook,
            job.id,
            job.attempts + 1
        ))
    }

    fn run_job(&mut self, job: Job) {
        let Some(cfg) = self.config() else {
            // Nothing runs until hooks.toml is fixed; the job waits.
            let _ = self.store.postpone(&job, Duration::from_secs(60));
            return;
        };
        let Some(hook) = cfg.get(&job.hook).filter(|h| h.enabled).cloned() else {
            log::info!("Hooks: {} is gone or off; job {} dropped", job.hook, job.id);
            let _ = self.store.skip(&job);
            return;
        };
        let fingerprint = hook.fingerprint(&cfg.dir);
        let allowed = self.store.allowed_hash(&hook.name).ok().flatten();
        if allowed.as_deref() != Some(fingerprint.as_str()) {
            log::warn!(
                "Hooks: {} is {} and needs `sokki hooks allow {}`; job {} skipped",
                hook.name,
                if allowed.is_some() { "changed" } else { "new" },
                hook.name,
                job.id
            );
            if let Ok(run) = self.store.begin_run(
                Some(job.id),
                &job.hook,
                &job.event,
                &job.subject,
                job.attempts + 1,
                "event",
                None,
            ) {
                let _ =
                    self.store
                        .end_run(run, "not_allowed", None, Some("needs sokki hooks allow"));
            }
            let _ = self.store.skip(&job);
            if self
                .told_not_allowed
                .insert((hook.name.clone(), fingerprint))
            {
                (self.host.report)(Report::NotAllowed { hook: hook.name });
            }
            return;
        }
        let event: HookEvent = match serde_json::from_str(&job.payload) {
            Ok(e) => e,
            Err(e) => {
                log::warn!("Hooks: job {} has an unreadable event: {e}", job.id);
                let _ = self.store.skip(&job);
                return;
            }
        };
        let payload = content::for_scope(&event, &hook.scope);
        let log_path = self.log_path(&job);
        let run = match self.store.begin(&job, &log_path) {
            Ok(r) => r,
            Err(e) => {
                log::warn!("Hooks: {e:#}");
                return;
            }
        };
        let begun = Instant::now();
        let env = (self.host.env)();
        let gate = self.host.gate.clone();
        let outcome = exec::run(&hook, &cfg.dir, &payload, &env, &*gate, &log_path);
        let what = outcome.describe(hook.timeout);
        let error = (!outcome.is_ok()).then_some(what.as_str());
        let _ = self
            .store
            .end_run(run, outcome.status(), outcome.exit_code(), error);
        let over = self.store.after_run(&job, outcome.is_ok()).unwrap_or(true);
        log::info!(
            "Hooks: {} on {} {}: {what} in {} ms{}",
            hook.name,
            job.event,
            job.subject,
            begun.elapsed().as_millis(),
            if !outcome.is_ok() && !over {
                "; tried again in a minute"
            } else {
                ""
            }
        );
        if outcome.is_ok() {
            (self.host.report)(Report::Succeeded { hook: hook.name });
        } else if over {
            (self.host.report)(Report::Failed {
                hook: hook.name,
                what,
                log: log_path,
            });
        }
        self.runs_since_prune += 1;
        if self.runs_since_prune >= 50 {
            self.runs_since_prune = 0;
            if let Ok(files) = self.store.prune(KEEP_RUNS) {
                for f in files {
                    let _ = std::fs::remove_file(f);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hooks::event::{Meta, MEETING_NOTE_READY};
    use crate::hooks::{tools, Paths};
    use std::sync::Mutex;

    fn runner(name: &str, toml: &str) -> (Runner, Arc<Mutex<Vec<Report>>>, Paths) {
        let root =
            std::env::temp_dir().join(format!("sokki-hooks-runner-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let paths = Paths {
            config_dir: root.join("config"),
            state_dir: root.join("state"),
        };
        std::fs::create_dir_all(&paths.config_dir).unwrap();
        std::fs::write(paths.config_file(), toml).unwrap();
        let reports = Arc::new(Mutex::new(Vec::new()));
        let r2 = reports.clone();
        let host = Host {
            paths: paths.clone(),
            gate: Arc::new(|| Gate::Free),
            report: Arc::new(move |r| r2.lock().unwrap().push(r)),
            env: Arc::new(|| vec![("SOKKI_NOTES_DIR".into(), "/notes".into())]),
        };
        let store = Store::open(&paths.state_dir).unwrap();
        let r = Runner {
            host,
            store,
            told_not_allowed: HashSet::new(),
            told_config: None,
            last_schedule_check: None,
            runs_since_prune: 0,
        };
        (r, reports, paths)
    }

    fn note_ready(paths: &Paths, app: &str) -> HookEvent {
        let mut e = tools::fixture_event(MEETING_NOTE_READY, &paths.state_dir.join("fx")).unwrap();
        e.meta = Meta {
            app: Some(app.into()),
            ..Meta::default()
        };
        e
    }

    const TOML: &str = "[[hook]]\nname = \"zoom-only\"\non = \"meeting.note_ready\"\nrun = \"cat > out-$SOKKI_HOOK.json; echo $SOKKI_NOTES_DIR\"\nmatch = { app = \"Zoom\", person = \"Anna Beispiel\" }\nscope = [\"summary\"]\n\n[[hook]]\nname = \"fails\"\non = \"meeting.note_ready\"\nrun = \"exit 4\"\n";

    #[test]
    fn queue_match_approval_retry_and_reports() {
        let (mut r, reports, paths) = runner("flow", TOML);
        // Not allowed yet: skipped and reported once.
        r.queue(vec![note_ready(&paths, "Zoom")]);
        while let Some(job) = r.store.next_due(now_ms()).unwrap() {
            r.run_job(job);
        }
        assert!(!paths.config_dir.join("out-zoom-only.json").exists());
        assert_eq!(
            *reports.lock().unwrap(),
            vec![
                Report::NotAllowed {
                    hook: "zoom-only".into()
                },
                Report::NotAllowed {
                    hook: "fails".into()
                }
            ]
        );
        reports.lock().unwrap().clear();
        tools::allow(&paths, "zoom-only").unwrap();
        tools::allow(&paths, "fails").unwrap();
        // Teams does not match zoom-only; "fails" has no filter.
        r.queue(vec![
            note_ready(&paths, "Teams"),
            note_ready(&paths, "Zoom"),
        ]);
        let mut ran = 0;
        let later = now_ms() + 10 * 60 * 1000;
        while let Some(job) = r.store.next_due(later).unwrap() {
            r.run_job(job);
            ran += 1;
        }
        // zoom-only once (Zoom only). "fails" has one job for both events
        // (same hook and meeting while pending): it runs and retries once.
        assert_eq!(ran, 3, "zoom-only, fails, its retry");
        let seen: HookEvent = serde_json::from_str(
            &std::fs::read_to_string(paths.config_dir.join("out-zoom-only.json")).unwrap(),
        )
        .unwrap();
        assert!(seen.content.summary.is_some());
        assert!(seen.content.tasks.is_none(), "tasks are not in its scope");
        let got = reports.lock().unwrap().clone();
        assert!(got.contains(&Report::Succeeded {
            hook: "zoom-only".into()
        }));
        let failed: Vec<_> = got
            .iter()
            .filter(|r| matches!(r, Report::Failed { .. }))
            .collect();
        assert_eq!(
            failed.len(),
            1,
            "one notice after the retry, not two: {got:?}"
        );
        let runs = r.store.runs(Some("zoom-only"), 10).unwrap();
        let log = std::fs::read_to_string(runs[0].log_path.as_ref().unwrap()).unwrap();
        assert!(
            log.contains("/notes"),
            "the host's env reaches the hook: {log}"
        );
        // The script changed: back to not allowed.
        std::fs::write(paths.config_file(), TOML.replace("exit 4", "exit 5")).unwrap();
        reports.lock().unwrap().clear();
        r.queue(vec![note_ready(&paths, "Zoom")]);
        while let Some(job) = r.store.next_due(later).unwrap() {
            r.run_job(job);
        }
        assert!(reports.lock().unwrap().contains(&Report::NotAllowed {
            hook: "fails".into()
        }));
    }

    #[test]
    fn dictation_text_is_queued_only_for_hooks_that_may_read_it() {
        let toml = "[[hook]]\nname = \"reads\"\non = \"dictation.done\"\nrun = \"x\"\nscope = [\"dictations\"]\n\n[[hook]]\nname = \"blind\"\non = \"dictation.done\"\nrun = \"x\"\n";
        let (mut r, _reports, _paths) = runner("dictation", toml);
        r.queue(vec![HookEvent::dictation(5, "Geheim", Meta::default())]);
        let mut stored = Vec::new();
        while let Some(job) = r.store.next_due(now_ms()).unwrap() {
            stored.push((job.hook.clone(), job.payload.contains("Geheim")));
            r.store.skip(&job).unwrap();
        }
        assert_eq!(
            stored,
            vec![("reads".to_string(), true), ("blind".to_string(), false)]
        );
    }

    #[test]
    fn a_broken_config_is_reported_once_and_jobs_wait() {
        let (mut r, reports, paths) = runner("broken", TOML);
        tools::allow(&paths, "fails").unwrap();
        r.queue(vec![note_ready(&paths, "Zoom")]);
        std::fs::write(paths.config_file(), "[[hook]]\nname = 1\n").unwrap();
        let job = r.store.next_due(now_ms()).unwrap().unwrap();
        r.run_job(job);
        r.queue(vec![note_ready(&paths, "Zoom")]);
        let got = reports.lock().unwrap().clone();
        assert_eq!(got.len(), 1, "{got:?}");
        assert!(matches!(got[0], Report::ConfigInvalid { .. }));
        assert!(r.store.next_due(now_ms()).unwrap().is_some() || r.store.pending().unwrap() > 0);
    }

    #[test]
    fn schedules_start_from_now_and_fire_once_when_due() {
        let toml = "[[hook]]\nname = \"every-minute\"\non = \"schedule\"\ncron = \"* * * * *\"\nrun = \"cat > s.json\"\n";
        let (mut r, _reports, paths) = runner("schedule", toml);
        r.schedules();
        assert_eq!(
            r.store.pending().unwrap(),
            0,
            "a new schedule does not catch up"
        );
        // Pretend it last came due two minutes ago.
        let two_ago = (chrono::Local::now() - chrono::Duration::minutes(2))
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, false);
        r.store.set_last_due("every-minute", &two_ago).unwrap();
        r.schedules();
        r.schedules();
        assert_eq!(r.store.pending().unwrap(), 1, "missed runs fire once");
        tools::allow(&paths, "every-minute").unwrap();
        let job = r.store.next_due(now_ms()).unwrap().unwrap();
        r.run_job(job);
        let seen: HookEvent = serde_json::from_str(
            &std::fs::read_to_string(paths.config_dir.join("s.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(seen.id, "schedule:every-minute");
        assert_eq!(
            seen.schedule.as_ref().unwrap().previous.as_deref(),
            Some(two_ago.as_str())
        );
    }
}
