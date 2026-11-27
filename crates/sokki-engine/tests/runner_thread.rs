//! The runner as a host starts it (`hooks::start`, its own thread): an
//! event in, the hook run, a recording holding it back, own notes waiting
//! for quiet, listeners without content. One process, one runner, so this
//! file has one test.

use sokki_engine::hooks::event::{MEETING_NOTE_READY, OWN_NOTES_SAVED};
use sokki_engine::hooks::{self, tools, Gate, HookEvent, Host, Meta, Paths, Report};
use std::path::Path;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

fn wait_for(what: &str, limit: Duration, ok: impl Fn() -> bool) {
    let begun = Instant::now();
    while !ok() {
        assert!(begun.elapsed() < limit, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn events_reach_hooks_through_the_runner_thread() {
    let root = std::env::temp_dir().join(format!("sokki-runner-thread-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let paths = Paths {
        config_dir: root.join("config"),
        state_dir: root.join("state"),
    };
    std::fs::create_dir_all(&paths.config_dir).unwrap();
    std::fs::write(
        paths.config_file(),
        "[[hook]]\nname = \"note\"\non = \"meeting.note_ready\"\nrun = \"cat > note-$SOKKI_ID.json\"\nscope = [\"tasks\"]\n\n\
         [[hook]]\nname = \"own\"\non = \"own_notes.saved\"\nrun = \"cat >> own.jsonl\"\nscope = [\"own_notes\"]\n",
    )
    .unwrap();

    let gate = Arc::new(AtomicU8::new(1)); // a meeting records
    let reports: Arc<Mutex<Vec<Report>>> = Arc::default();
    let heard: Arc<Mutex<Vec<HookEvent>>> = Arc::default();
    let (g, r, h) = (gate.clone(), reports.clone(), heard.clone());
    hooks::subscribe(move |e| h.lock().unwrap().push(e.clone()));
    hooks::start(Host {
        paths: paths.clone(),
        gate: Arc::new(move || match g.load(Ordering::SeqCst) {
            0 => Gate::Free,
            _ => Gate::Recording,
        }),
        report: Arc::new(move |x| r.lock().unwrap().push(x)),
        env: Arc::new(Vec::new),
    })
    .unwrap();
    assert!(
        hooks::start(Host {
            paths: paths.clone(),
            gate: Arc::new(|| Gate::Free),
            report: Arc::new(|_| {}),
            env: Arc::new(Vec::new),
        })
        .is_err(),
        "one runner per process"
    );

    // While the meeting records, the hook waits.
    let fixtures = root.join("fixtures");
    let event = tools::fixture_event(MEETING_NOTE_READY, &fixtures).unwrap();
    hooks::emit(event);
    let out = paths
        .config_dir
        .join(format!("note-{}.json", tools::FIXTURE_ID));
    std::thread::sleep(Duration::from_millis(1500));
    assert!(!out.exists(), "a hook ran during a recording");
    gate.store(0, Ordering::SeqCst);
    wait_for(
        "the hook after the recording",
        Duration::from_secs(10),
        || out.exists(),
    );
    wait_for("its report", Duration::from_secs(5), || {
        reports.lock().unwrap().contains(&Report::Succeeded {
            hook: "note".into(),
        })
    });
    let seen: HookEvent = serde_json::from_str(&std::fs::read_to_string(&out).unwrap()).unwrap();
    assert_eq!(seen.content.tasks.as_ref().map(Vec::len), Some(3));

    // Own notes: three saves in a row, one run once they rest.
    let notes = root.join("notes.md");
    for text in ["- eins", "- eins\n- zwei", "- eins\n- zwei\n- drei"] {
        std::fs::write(&notes, text).unwrap();
        hooks::emit_after_quiet(
            "own_notes:m1",
            Duration::from_millis(400),
            HookEvent::own_notes("m1", Path::new(&notes), Meta::default()),
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    let own = paths.config_dir.join("own.jsonl");
    wait_for("the own-notes hook", Duration::from_secs(10), || {
        own.exists()
    });
    std::thread::sleep(Duration::from_millis(800));
    let lines: Vec<HookEvent> = std::fs::read_to_string(&own)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(lines.len(), 1, "saves in a row run the hook once");
    assert_eq!(lines[0].event, OWN_NOTES_SAVED);
    assert_eq!(
        lines[0].content.text.as_deref(),
        Some("- eins\n- zwei\n- drei")
    );

    // At quit, a resting event goes into the queue at once instead of
    // waiting out its minute (and being lost with the process).
    std::fs::write(&notes, "- vier").unwrap();
    hooks::emit_after_quiet(
        "own_notes:m2",
        Duration::from_secs(60),
        HookEvent::own_notes("m2", Path::new(&notes), Meta::default()),
    );
    hooks::flush();
    wait_for(
        "the flushed own-notes event",
        Duration::from_secs(10),
        || {
            std::fs::read_to_string(&own)
                .unwrap_or_default()
                .contains("\"id\":\"m2\"")
        },
    );

    // Listeners (`sokki events`) heard both, without content.
    let heard = heard.lock().unwrap();
    assert!(heard.iter().any(|e| e.event == MEETING_NOTE_READY));
    assert_eq!(
        heard.iter().filter(|e| e.event == OWN_NOTES_SAVED).count(),
        2,
        "once per rest, once at the flush"
    );
    assert!(heard.iter().all(|e| e.content.is_empty()));
    let _ = std::fs::remove_dir_all(&root);
}
