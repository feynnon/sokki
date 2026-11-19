//! Print meeting events live, for manual testing with a real call.
//!
//!     cargo run --example watch            # default timings (3 s / 20 s)
//!     cargo run --example watch -- --fast  # 1 s poll, 2 s start, 5 s end
//!
//! It also prints the raw mic clients whenever they change, so you can see
//! which process (and path) a new app uses and extend the app table. On
//! Linux that list comes from its own `pw-dump --monitor`, so nothing
//! polls; on macOS it polls Core Audio.

use std::time::Duration;

use meeting_detect::{classify_all, AudioClient, MeetingEvent, Watcher, WatcherConfig};

fn main() {
    let fast = std::env::args().any(|a| a == "--fast");
    let config = if fast {
        WatcherConfig {
            poll_interval: Duration::from_secs(1),
            start_after: Duration::from_secs(2),
            end_after: Duration::from_secs(5),
        }
    } else {
        WatcherConfig::default()
    };
    let own_pid = std::process::id();
    println!("watching mic clients (own pid {own_pid}), {config:?}; Ctrl-C to quit");

    let _watcher = Watcher::spawn(config, own_pid, |event| match event {
        MeetingEvent::Started { app } => println!(
            ">>> STARTED  {} (app {}, pid {}, matched by {:?}, path {:?})",
            app.label(),
            app.name,
            app.pid,
            app.matched_by,
            app.exe_path
        ),
        MeetingEvent::Ended { app, after } => {
            println!(">>> ENDED    {} after {}s", app.label(), after.as_secs())
        }
    })
    .expect("spawn watcher");

    print_raw_clients(config, own_pid);
}

fn describe(clients: &[AudioClient], own_pid: u32) -> String {
    if clients.is_empty() {
        return "mic clients: none".to_string();
    }
    let mut s = String::from("mic clients:");
    for c in clients {
        s.push_str(&format!(
            "\n  pid {:>6}  {:<28} bundle={:?} path={:?} responsible={:?}",
            c.pid, c.name, c.bundle_id, c.exe_path, c.responsible_path
        ));
    }
    let apps = classify_all(clients, own_pid);
    let names: Vec<_> = apps.iter().map(|a| a.name).collect();
    s.push_str(&format!("\n  -> meeting apps: {names:?}"));
    s
}

#[cfg(target_os = "linux")]
fn print_raw_clients(_config: WatcherConfig, own_pid: u32) {
    use meeting_detect::{ClientSink, EventSource, PwMonitor};
    let _monitor = PwMonitor
        .start(ClientSink::new(move |clients| {
            println!("{}", describe(&clients, own_pid));
            true
        }))
        .expect("start pw-dump monitor");
    loop {
        std::thread::park();
    }
}

#[cfg(not(target_os = "linux"))]
fn print_raw_clients(config: WatcherConfig, own_pid: u32) {
    let mut last = String::new();
    loop {
        let line = match meeting_detect::audio_input_clients() {
            Ok(clients) => describe(&clients, own_pid),
            Err(e) => format!("error: {e}"),
        };
        if line != last {
            println!("{line}");
            last = line;
        }
        std::thread::sleep(config.poll_interval);
    }
}
