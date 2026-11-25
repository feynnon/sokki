//! Try the chat from a terminal (needs a process with Accessibility):
//!
//!   cargo run --example chat -- dump zoom          # the app's tree
//!   cargo run --example chat -- post chrome "Hi"   # post one line
//!   cargo run --example chat -- watch zoom "Hi"    # post, then wait for "stop"

use meeting_chat::{dump, post, Target};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (cmd, app) = (args.first().map(String::as_str), args.get(1).map(String::as_str));
    let Some(target) = app.and_then(Target::for_app) else {
        eprintln!("usage: chat dump|post|watch zoom|teams|chrome|arc|edge|brave [text]");
        std::process::exit(2);
    };
    let text = args.get(2).cloned().unwrap_or_else(|| "Test from Sokki".into());
    match cmd {
        Some("dump") => match dump(target, 20_000) {
            Ok(s) => print!("{s}"),
            Err(e) => eprintln!("{e}"),
        },
        Some("post") | Some("watch") => match post(target, &text) {
            Ok(mut chat) => {
                println!("posted");
                if cmd == Some("watch") {
                    loop {
                        match chat.stop_requested() {
                            Some(true) => {
                                println!("stop seen");
                                let _ = chat.reply("OK, stopped.");
                                break;
                            }
                            Some(false) => {}
                            None => {
                                println!("chat gone");
                                break;
                            }
                        }
                        std::thread::sleep(std::time::Duration::from_secs(2));
                    }
                }
            }
            Err(e) => eprintln!("not posted: {e}"),
        },
        _ => eprintln!("unknown command"),
    }
}
