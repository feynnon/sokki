//! Tests against the real Core Audio on this machine.
#![cfg(target_os = "macos")]

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use meeting_detect::audio_input_clients;

/// Listing clients reads Core Audio's process list; it opens no input and
/// needs no microphone permission. The list may be empty.
#[test]
fn audio_input_clients_works_without_mic_permission() {
    let clients = audio_input_clients().expect("Core Audio process list");
    for c in &clients {
        assert!(c.pid > 0);
        assert!(!c.name.is_empty());
    }
    eprintln!("{} mic client(s): {clients:#?}", clients.len());
}

/// Records from the default mic with ffmpeg for a few seconds and checks
/// ffmpeg shows up as an input client. Needs `ffmpeg` on PATH and mic
/// permission for the terminal running the test, hence ignored:
///
///     cargo test -p meeting-detect -- --ignored --nocapture
#[test]
#[ignore = "needs ffmpeg and microphone permission"]
fn ffmpeg_recording_shows_up_as_input_client() {
    let mut child = Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "avfoundation",
            "-i",
            ":0",
            "-t",
            "6",
            "-f",
            "null",
            "-",
        ])
        .stdin(Stdio::null())
        .spawn()
        .expect("spawn ffmpeg");
    let ffmpeg_pid = child.id();

    let deadline = Instant::now() + Duration::from_secs(5);
    let mut found = None;
    while Instant::now() < deadline && found.is_none() {
        std::thread::sleep(Duration::from_millis(300));
        let clients = audio_input_clients().expect("list clients");
        found = clients.into_iter().find(|c| {
            c.pid == ffmpeg_pid
                || c.exe_path
                    .as_deref()
                    .is_some_and(|p| p.ends_with("/ffmpeg"))
                || c.responsible_pid == Some(ffmpeg_pid)
        });
    }
    let _ = child.kill();
    let _ = child.wait();
    let client = found.expect("ffmpeg (or its responsible terminal) listed as a mic client");
    eprintln!("found: {client:#?}");
    assert_eq!(
        meeting_detect::classify(&[client], std::process::id()),
        None,
        "ffmpeg is not a meeting app"
    );
}
