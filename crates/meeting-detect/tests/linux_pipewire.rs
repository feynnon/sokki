//! Tests against a real PipeWire. They need `pw-dump`, `parecord`, a
//! running PipeWire with pipewire-pulse and a capture source, hence
//! ignored; one at a time, because they count processes and threads:
//!
//!     cargo test --test linux_pipewire -- --ignored --test-threads=1
//!
//! A "meeting app" here is a copy of `pacat` (what `parecord` links to)
//! named like the app, so `/proc/<pid>/exe` says `zoom` or `chrome`. A
//! symlink would not do: the kernel resolves it.
#![cfg(target_os = "linux")]

use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use meeting_detect::{
    audio_input_clients, classify, AudioClient, ClientSink, EventSource, MeetingEvent, PwMonitor,
    Watcher, WatcherConfig,
};

fn fake_app(binary: &str, name: &str) -> Child {
    let dir = std::env::temp_dir().join(format!("meeting-detect-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let exe: PathBuf = dir.join(binary);
    if !exe.exists() {
        std::fs::copy("/usr/bin/pacat", &exe).expect("copy pacat");
    }
    // Named zoom, pacat no longer knows it should record.
    Command::new(&exe)
        .args(["--record", "--file-format=wav"])
        .arg(format!("--client-name={name}"))
        .arg(dir.join(format!("{binary}.wav")))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("start fake app")
}

fn stop(mut child: Child) {
    let _ = child.kill();
    let _ = child.wait();
}

/// Waits for a list satisfying `want`; panics after 5 s.
fn wait_for(rx: &mpsc::Receiver<Vec<AudioClient>>, want: impl Fn(&[AudioClient]) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        let list = rx
            .recv_timeout(left)
            .expect("expected client list did not arrive");
        if want(&list) {
            return;
        }
    }
}

fn has(list: &[AudioClient], pid: u32) -> bool {
    list.iter().any(|c| c.pid == pid)
}

fn monitors_running() -> usize {
    let out = Command::new("pgrep")
        .args(["-f", "pw-dump --monitor"])
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).lines().count()
}

#[test]
#[ignore = "needs a running PipeWire"]
fn one_shot_dump_lists_a_recording_app() {
    let zoom = fake_app("zoom", "ZOOM VoiceEngine");
    let pid = zoom.id();
    let deadline = Instant::now() + Duration::from_secs(5);
    let client = loop {
        let clients = audio_input_clients().expect("pw-dump");
        if let Some(c) = clients.into_iter().find(|c| c.pid == pid) {
            break c;
        }
        assert!(Instant::now() < deadline, "fake zoom never showed up");
        std::thread::sleep(Duration::from_millis(100));
    };
    stop(zoom);
    eprintln!("{client:#?}");
    assert_eq!(client.name, "ZOOM VoiceEngine");
    assert!(client.exe_path.as_deref().unwrap().ends_with("/zoom"));
    assert_eq!(classify(&[client], std::process::id()).unwrap().id, "zoom");
}

#[test]
#[ignore = "needs a running PipeWire"]
fn monitor_follows_streams_skips_sink_capture_and_restarts() {
    let (tx, rx) = mpsc::channel();
    let guard = PwMonitor
        .start(ClientSink::new(move |c| tx.send(c).is_ok()))
        .unwrap();
    rx.recv_timeout(Duration::from_secs(5))
        .expect("the current list first");

    let chrome = fake_app("chrome", "Google Chrome");
    let chrome_pid = chrome.id();
    wait_for(&rx, |l| has(l, chrome_pid));

    // Our recorder's kind of stream: records the speaker, never listed.
    let mut capture = Command::new("pw-record")
        .args(["-P", "{ stream.capture.sink=true }", "/dev/null"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    std::thread::sleep(Duration::from_millis(500));
    while let Ok(list) = rx.try_recv() {
        assert!(!has(&list, capture.id()), "capture-sink stream listed");
    }
    let _ = capture.kill();
    let _ = capture.wait();

    stop(chrome);
    wait_for(&rx, |l| !has(l, chrome_pid));

    // pw-dump dies (as when PipeWire restarts): the monitor starts it again.
    assert_eq!(monitors_running(), 1);
    Command::new("pkill")
        .args(["-f", "pw-dump --monitor"])
        .status()
        .unwrap();
    std::thread::sleep(Duration::from_millis(1500));
    let zoom = fake_app("zoom", "ZOOM VoiceEngine");
    let zoom_pid = zoom.id();
    wait_for(&rx, |l| has(l, zoom_pid));
    stop(zoom);

    drop(guard);
    assert_eq!(monitors_running(), 0, "pw-dump killed with the guard");
}

/// Voluntary context switches of this process's threads with `name`.
fn switches(name: &str) -> Vec<(String, u64)> {
    let mut out = Vec::new();
    for task in std::fs::read_dir("/proc/self/task").unwrap() {
        let dir = task.unwrap().path();
        let comm = std::fs::read_to_string(dir.join("comm")).unwrap_or_default();
        if comm.trim() != name {
            continue;
        }
        let status = std::fs::read_to_string(dir.join("status")).unwrap();
        let n = status
            .lines()
            .find_map(|l| l.strip_prefix("voluntary_ctxt_switches:"))
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        out.push((dir.display().to_string(), n));
    }
    out
}

#[test]
#[ignore = "needs a running PipeWire"]
fn watcher_reports_a_call_and_sleeps_when_idle() {
    let config = WatcherConfig {
        start_after: Duration::from_secs(1),
        end_after: Duration::from_secs(1),
        ..WatcherConfig::default()
    };
    let (w, rx) = Watcher::spawn_channel(config, std::process::id()).unwrap();

    // Idle: neither thread wakes up. Settle first: right after start
    // PipeWire still reports changes (pw-dump's own client, WirePlumber
    // suspending nodes 5 s after the last stream), and each wakes the
    // reader once.
    std::thread::sleep(Duration::from_secs(6));
    let before = (switches("meeting-detect"), switches("pw-dump-monitor"));
    assert_eq!(before.0.len(), 1);
    assert_eq!(before.1.len(), 1);
    std::thread::sleep(Duration::from_secs(5));
    let after = (switches("meeting-detect"), switches("pw-dump-monitor"));
    assert_eq!(before, after, "threads woke up while idle");

    let zoom = fake_app("zoom", "ZOOM VoiceEngine");
    match rx.recv_timeout(Duration::from_secs(5)).unwrap() {
        MeetingEvent::Started { app } => {
            assert_eq!(app.id, "zoom");
            assert_eq!(app.pid, zoom.id());
            assert_eq!(app.label(), "Zoom");
        }
        e => panic!("{e:?}"),
    }
    stop(zoom);
    match rx.recv_timeout(Duration::from_secs(5)).unwrap() {
        MeetingEvent::Ended { app, after } => {
            assert_eq!(app.id, "zoom");
            assert!(after < Duration::from_secs(3), "{after:?}");
        }
        e => panic!("{e:?}"),
    }
    w.stop();
    assert_eq!(monitors_running(), 0);
}
