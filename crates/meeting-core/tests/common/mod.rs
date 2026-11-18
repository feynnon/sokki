//! Helpers shared by the integration tests: a tiny scripted HTTP server and a
//! parser for the plain-text transcript fixtures.

#![allow(dead_code)]

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use meeting_core::{Segment, TrackKind, Transcript};
use serde_json::{json, Value};

/// One request the mock server received.
#[derive(Debug, Clone)]
pub struct Recorded {
    pub path: String,
    pub headers: Vec<(String, String)>,
    pub body: Value,
}

impl Recorded {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
    /// Content of the message with `role` (first match).
    pub fn message(&self, role: &str) -> Option<String> {
        self.body["messages"]
            .as_array()?
            .iter()
            .find(|m| m["role"] == role)
            .and_then(|m| m["content"].as_str())
            .map(str::to_string)
    }
    pub fn system(&self) -> String {
        self.message("system").unwrap_or_default()
    }
    pub fn user(&self) -> String {
        self.message("user").unwrap_or_default()
    }
}

pub type Responder = Box<dyn Fn(usize, &Recorded) -> (u16, String) + Send + Sync>;

pub struct MockServer {
    pub base_url: String,
    pub requests: Arc<Mutex<Vec<Recorded>>>,
}

impl MockServer {
    /// Serves responses from `responder(request_index, request)`.
    pub fn start(responder: Responder) -> Self {
        Self::start_with_delay(responder, None)
    }

    /// Replies with the given (status, body) pairs in order; extra requests
    /// get HTTP 500.
    pub fn scripted(replies: Vec<(u16, String)>) -> Self {
        Self::start(Box::new(move |i, _| {
            replies
                .get(i)
                .cloned()
                .unwrap_or((500, "{\"error\":\"unexpected extra request\"}".into()))
        }))
    }

    /// Like `start`, but waits `delay` before answering (for timeout tests).
    pub fn start_with_delay(responder: Responder, delay: Option<Duration>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().unwrap();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let reqs = requests.clone();
        std::thread::spawn(move || {
            for (i, stream) in listener.incoming().enumerate() {
                let Ok(mut stream) = stream else { continue };
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut request_line = String::new();
                if reader.read_line(&mut request_line).is_err() {
                    continue;
                }
                let path = request_line
                    .split_whitespace()
                    .nth(1)
                    .unwrap_or("")
                    .to_string();
                let mut headers = Vec::new();
                let mut content_length = 0usize;
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap_or(0) == 0 {
                        break;
                    }
                    let line = line.trim_end();
                    if line.is_empty() {
                        break;
                    }
                    if let Some((k, v)) = line.split_once(':') {
                        let (k, v) = (k.trim().to_string(), v.trim().to_string());
                        if k.eq_ignore_ascii_case("content-length") {
                            content_length = v.parse().unwrap_or(0);
                        }
                        headers.push((k, v));
                    }
                }
                let mut body = vec![0u8; content_length];
                let _ = reader.read_exact(&mut body);
                let body: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
                let rec = Recorded {
                    path,
                    headers,
                    body,
                };
                reqs.lock().unwrap().push(rec.clone());
                if let Some(d) = delay {
                    std::thread::sleep(d);
                }
                let (status, body) = responder(i, &rec);
                let resp = format!(
                    "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(resp.as_bytes());
                let _ = stream.flush();
            }
        });
        Self {
            base_url: format!("http://{addr}"),
            requests,
        }
    }

    pub fn requests(&self) -> Vec<Recorded> {
        self.requests.lock().unwrap().clone()
    }
}

/// Body of an Ollama `/api/chat` reply carrying `content`.
pub fn ollama_reply(content: &str) -> String {
    json!({
        "model": "test",
        "created_at": "2026-11-17T12:00:00Z",
        "message": { "role": "assistant", "content": content },
        "done": true,
        "done_reason": "stop"
    })
    .to_string()
}

/// Body of an OpenAI chat-completions reply carrying `content`.
pub fn openai_reply(content: &str) -> String {
    json!({
        "id": "x",
        "object": "chat.completion",
        "choices": [{ "index": 0, "message": { "role": "assistant", "content": content }, "finish_reason": "stop" }]
    })
    .to_string()
}

pub fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

/// Parses "[mm:ss] Speaker: text" (or "[h:mm:ss] …") lines into a transcript.
/// A segment ends where the next one starts (last one: +5 s). Speaker "Ich"
/// is put on the mic track, everyone else on the system track. Lines starting
/// with '#' and blank lines are ignored.
pub fn parse_transcript(text: &str) -> Transcript {
    let mut segs: Vec<Segment> = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let rest = line.strip_prefix('[').expect("line must start with [");
        let close = rest.find(']').expect("missing ]");
        let stamp = &rest[..close];
        let parts: Vec<f64> = stamp
            .split(':')
            .map(|p| p.parse().expect("bad stamp"))
            .collect();
        let t = parts.iter().fold(0.0, |acc, p| acc * 60.0 + p);
        let rest = rest[close + 1..].trim();
        let (speaker, text) = rest.split_once(':').expect("missing speaker:");
        segs.push(Segment {
            start_s: t,
            end_s: t + 5.0,
            track: if speaker.trim() == "Ich" {
                TrackKind::Mic
            } else {
                TrackKind::System
            },
            speaker: speaker.trim().to_string(),
            text: text.trim().to_string(),
        });
    }
    for i in 0..segs.len().saturating_sub(1) {
        let next = segs[i + 1].start_s;
        if next > segs[i].start_s {
            segs[i].end_s = next;
        }
    }
    Transcript { segments: segs }
}

pub fn load_fixture(name: &str) -> Transcript {
    let text = std::fs::read_to_string(fixture_path(name)).expect("read fixture");
    parse_transcript(&text)
}

/// A fresh empty directory under the system temp dir.
pub fn temp_dir(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let p = std::env::temp_dir().join(format!("meeting-core-{tag}-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&p).unwrap();
    p
}
