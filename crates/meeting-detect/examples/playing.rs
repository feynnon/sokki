//! Prints every process playing audio now, as the auto-meeting listener
//! sees it (`cargo run -p meeting-detect --example playing`).

fn main() {
    match meeting_detect::audio_output_clients() {
        Ok(clients) if clients.is_empty() => println!("nothing plays"),
        Ok(clients) => {
            for c in clients {
                println!("{} (pid {}, {:?})", c.name, c.pid, c.bundle_id);
            }
        }
        Err(e) => println!("error: {e}"),
    }
}
