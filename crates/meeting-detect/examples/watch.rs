//! Print meeting events live, for manual testing with a real call.
//!
//!     cargo run --example watch            # default timings (3 s / 20 s)
//!     cargo run --example watch -- --fast  # 1 s poll, 2 s start, 5 s end
//!
//! Every poll also prints the raw mic clients, so you can see which
//! process (and path) a new app uses and extend the app table.

use std::time::Duration;

use meeting_detect::{audio_input_clients, classify_all, MeetingEvent, Watcher, WatcherConfig};

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
            ">>> STARTED  {} (pid {}, matched by {:?}, path {:?})",
            app.name, app.pid, app.matched_by, app.exe_path
        ),
        MeetingEvent::Ended { app, after } => {
            println!(">>> ENDED    {} after {}s", app.name, after.as_secs())
        }
    })
    .expect("spawn watcher");

    // Separate raw listing so the user sees what the watcher sees.
    let mut last = String::new();
    loop {
        let line = match audio_input_clients() {
            Ok(clients) if clients.is_empty() => "mic clients: none".to_string(),
            Ok(clients) => {
                let mut s = String::from("mic clients:");
                for c in &clients {
                    s.push_str(&format!(
                        "\n  pid {:>6}  {:<28} bundle={:?} path={:?} responsible={:?}",
                        c.pid, c.name, c.bundle_id, c.exe_path, c.responsible_path
                    ));
                }
                let apps = classify_all(&clients, own_pid);
                let names: Vec<_> = apps.iter().map(|a| a.name).collect();
                s.push_str(&format!("\n  -> meeting apps: {names:?}"));
                s
            }
            Err(e) => format!("error: {e}"),
        };
        if line != last {
            println!("{line}");
            last = line;
        }
        std::thread::sleep(config.poll_interval);
    }
}
