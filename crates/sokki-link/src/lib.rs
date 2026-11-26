//! The link between the Sokki phone app and the Mac: direct and end-to-end
//! encrypted, from any network, with nothing stored in between.
//!
//! Each device has a key pair; its public key is its address (iroh
//! "endpoint id"). The phone dials the Mac by that key: iroh finds the Mac
//! through n0's DNS, connects directly where it can (also across networks,
//! by hole punching) and otherwise forwards the encrypted bytes through one
//! of n0's free public relays. If the Mac is off, the call fails, and
//! nothing happens until the user tries again.
//!
//! Pairing: the Mac shows a ticket (QR code) with its key and a pairing
//! code. The phone sends `Pair { code }`; the Mac then remembers the phone's
//! key. Every other request is answered only for remembered phones.
//!
//! Wire format, one QUIC stream per request: 4-byte big-endian length, the
//! request as JSON, then the body bytes (audio for `Upload`) until the
//! stream ends. Answer: `{"ok": true, "data": …}` or `{"ok": false,
//! "error": "…"}`.
//!
//! `Dictate` is the exception, a live stream both ways: after the header
//! the phone sends frames of [u32 big-endian byte count][16-bit PCM, 16 kHz
//! mono, little-endian], a count of 0 ends the dictation; the Mac answers
//! with JSON lines (`DictationText`, or `{"error": "…"}`) until the final one.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use iroh::endpoint::{presets, Connection};
use iroh::protocol::{AcceptError, ProtocolHandler, Router};
use iroh::{Endpoint, EndpointAddr, EndpointId, SecretKey};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub use iroh;

pub const ALPN: &[u8] = b"sokki/link/1";
const MAX_HEADER: usize = 64 * 1024;
pub const MAX_BODY: usize = 1024 * 1024 * 1024;
const MAX_ANSWER: usize = 64 * 1024 * 1024;
/// How long the phone tries to reach the Mac before saying it is offline.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Request {
    Pair {
        code: String,
        device: String,
    },
    Ping,
    /// The body is the audio file (WebM/Opus, M4A, WAV, …).
    Upload {
        started_at: String,
        mime: String,
    },
    Status {
        id: String,
    },
    /// The body is one day of the phone keyboard's journal (JSONL), from the
    /// developer page "Keep what I write"; `file` is `journal-YYYY-MM-DD.jsonl`.
    Journal {
        file: String,
    },
    /// Live dictation: the Mac transcribes the phone's microphone as it
    /// comes and sends the text back (see the wire format above).
    Dictate,
}

/// What the Mac sends back while the phone dictates.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct DictationText {
    /// The whole dictation's text, read again at the stop: the last message.
    #[serde(rename = "final")]
    pub is_final: bool,
    /// Samples of audio this text covers.
    pub heard: u64,
    /// Everything heard so far; earlier words may still change until final.
    pub text: String,
}

/// What the Mac does with requests. Called on a blocking thread.
pub trait Handler: Send + Sync + 'static {
    /// Is this phone paired?
    fn is_paired(&self, peer: &str) -> bool;
    /// Pairs the phone when the code is right.
    fn pair(&self, peer: &str, code: &str, device: &str) -> Result<Value, String>;
    fn handle(&self, peer: &str, request: Request, body: Vec<u8>) -> Result<Value, String>;
    /// Live dictation: `audio` yields 16 kHz mono blocks and closes when the
    /// phone stops; call `text` with each update and last with the final
    /// text. Called on a blocking thread.
    fn dictate(
        &self,
        _peer: &str,
        _audio: std::sync::mpsc::Receiver<Vec<f32>>,
        _text: &dyn Fn(DictationText),
    ) -> Result<(), String> {
        Err("This Mac can't take dictation.".into())
    }
}

// ---------------------------------------------------------------- keys

pub fn new_secret() -> SecretKey {
    SecretKey::generate()
}

pub fn secret_to_hex(k: &SecretKey) -> String {
    k.to_bytes().iter().map(|b| format!("{b:02x}")).collect()
}

pub fn secret_from_hex(s: &str) -> Result<SecretKey> {
    let s = s.trim();
    if s.len() != 64 {
        bail!("key must be 64 hex characters");
    }
    let mut bytes = [0u8; 32];
    for (i, b) in bytes.iter_mut().enumerate() {
        *b = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).context("key is not hex")?;
    }
    Ok(SecretKey::from_bytes(&bytes))
}

// ---------------------------------------------------------------- ticket

/// What the Mac shows and the phone scans: "sokki1:<mac key>:<code>", inside
/// a sokki.org link (`PAIR_URL` + ticket) so a phone without Sokki lands on
/// the install page. The ticket sits after `#`, which browsers never send.
#[derive(Debug, Clone, PartialEq)]
pub struct Ticket {
    pub mac: EndpointId,
    pub code: String,
}

pub const PAIR_URL: &str = "https://sokki.org/p#";

impl Ticket {
    pub fn encode(&self) -> String {
        format!("sokki1:{}:{}", self.mac, self.code)
    }

    /// The ticket as a sokki.org link: what the QR code holds.
    pub fn url(&self) -> String {
        format!("{PAIR_URL}{}", self.encode())
    }

    /// Reads a bare ticket or a link with the ticket after `#`.
    pub fn parse(s: &str) -> Result<Self> {
        let s = s.trim();
        let s = match s.split_once('#') {
            Some((_, fragment)) => fragment,
            None => s,
        };
        let mut parts = s.splitn(3, ':');
        let (Some("sokki1"), Some(mac), Some(code)) = (parts.next(), parts.next(), parts.next())
        else {
            bail!("This is not a Sokki pairing code.");
        };
        let mac: EndpointId = mac
            .parse()
            .map_err(|_| anyhow!("The Mac's key in the code is damaged."))?;
        if code.is_empty() {
            bail!("The pairing code is missing.");
        }
        Ok(Ticket {
            mac,
            code: code.to_string(),
        })
    }
}

// ---------------------------------------------------------------- endpoint

/// An endpoint with the given key, reachable through n0's discovery and
/// relays.
pub async fn bind(secret: SecretKey) -> Result<Endpoint> {
    Endpoint::builder(presets::N0)
        .secret_key(secret)
        .alpns(vec![ALPN.to_vec()])
        .bind()
        .await
        .map_err(|e| anyhow!("could not start the link: {e}"))
}

// ---------------------------------------------------------------- Mac side

#[derive(Clone)]
struct Accept(Arc<dyn Handler>);

impl std::fmt::Debug for Accept {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Accept")
    }
}

impl ProtocolHandler for Accept {
    async fn accept(&self, connection: Connection) -> Result<(), AcceptError> {
        let peer = connection.remote_id().to_string();
        // One request per stream; serve streams until the phone hangs up.
        while let Ok((mut send, mut recv)) = connection.accept_bi().await {
            let handler = self.0.clone();
            let peer = peer.clone();
            let request = match read_header(&mut recv).await {
                Ok(r) => r,
                Err(e) => {
                    let answer = json!({ "ok": false, "error": format!("{e:#}") });
                    let _ = send.write_all(answer.to_string().as_bytes()).await;
                    let _ = send.finish();
                    continue;
                }
            };
            if request == Request::Dictate {
                // Its own task: the phone may dictate again while a final is out.
                tokio::spawn(dictate_stream(handler, peer, send, recv));
                continue;
            }
            let answer = match read_body(&mut recv).await.map(|body| (request, body)) {
                Ok((request, body)) => tokio::task::spawn_blocking(move || {
                    let result = match &request {
                        Request::Pair { code, device } => handler.pair(&peer, code, device),
                        _ if !handler.is_paired(&peer) => {
                            Err("This phone isn't paired with this Mac. Pair it again.".into())
                        }
                        _ => handler.handle(&peer, request, body),
                    };
                    match result {
                        Ok(data) => json!({ "ok": true, "data": data }),
                        Err(error) => json!({ "ok": false, "error": error }),
                    }
                })
                .await
                .unwrap_or_else(|e| json!({ "ok": false, "error": e.to_string() })),
                Err(e) => json!({ "ok": false, "error": format!("{e:#}") }),
            };
            send.write_all(answer.to_string().as_bytes())
                .await
                .map_err(AcceptError::from_err)?;
            send.finish()?;
        }
        Ok(())
    }
}

/// The Mac's side of a `Dictate` stream.
async fn dictate_stream(
    handler: Arc<dyn Handler>,
    peer: String,
    mut send: iroh::endpoint::SendStream,
    mut recv: iroh::endpoint::RecvStream,
) {
    let (audio_tx, audio_rx) = std::sync::mpsc::channel::<Vec<f32>>();
    let (text_tx, mut text_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let line = |v: Value| format!("{v}\n");
    if !handler.is_paired(&peer) {
        let err = json!({ "error": "This phone isn't paired with this Mac. Pair it again." });
        let _ = send.write_all(line(err).as_bytes()).await;
        let _ = send.finish();
        return;
    }
    let worker = {
        let text_tx = text_tx.clone();
        tokio::task::spawn_blocking(move || {
            let send = |t: DictationText| {
                let _ = text_tx.send(line(serde_json::to_value(t).unwrap_or_default()));
            };
            if let Err(e) = handler.dictate(&peer, audio_rx, &send) {
                let _ = text_tx.send(line(json!({ "error": e })));
            }
        })
    };
    drop(text_tx);
    let reader = tokio::spawn(async move {
        loop {
            let mut len = [0u8; 4];
            if recv.read_exact(&mut len).await.is_err() {
                break;
            }
            let n = u32::from_be_bytes(len) as usize;
            if n == 0 || n > MAX_FRAME {
                break;
            }
            let mut buf = vec![0u8; n];
            if recv.read_exact(&mut buf).await.is_err() {
                break;
            }
            let pcm = buf
                .chunks_exact(2)
                .map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0)
                .collect();
            if audio_tx.send(pcm).is_err() {
                break;
            }
        }
        // Dropping the sender ends the audio: the worker reads the rest.
    });
    while let Some(l) = text_rx.recv().await {
        if send.write_all(l.as_bytes()).await.is_err() {
            break;
        }
    }
    let _ = send.finish();
    reader.abort();
    let _ = worker.await;
}

/// The largest audio frame accepted: 10 s.
const MAX_FRAME: usize = 10 * 16_000 * 2;

async fn read_header(recv: &mut iroh::endpoint::RecvStream) -> Result<Request> {
    let mut len = [0u8; 4];
    recv.read_exact(&mut len)
        .await
        .map_err(|e| anyhow!("{e}"))?;
    let len = u32::from_be_bytes(len) as usize;
    if len > MAX_HEADER {
        bail!("request too large");
    }
    let mut header = vec![0u8; len];
    recv.read_exact(&mut header)
        .await
        .map_err(|e| anyhow!("{e}"))?;
    serde_json::from_slice(&header).context("unknown request")
}

async fn read_body(recv: &mut iroh::endpoint::RecvStream) -> Result<Vec<u8>> {
    let body = recv
        .read_to_end(MAX_BODY)
        .await
        .map_err(|e| anyhow!("receiving failed: {e}"))?;
    Ok(body)
}

/// Serves requests until the returned router is shut down.
pub fn serve(endpoint: Endpoint, handler: Arc<dyn Handler>) -> Router {
    Router::builder(endpoint)
        .accept(ALPN, Accept(handler))
        .spawn()
}

// ---------------------------------------------------------------- phone side

/// Sends one request to the Mac and returns its `data`, or the Mac's error.
pub async fn call(
    endpoint: &Endpoint,
    mac: EndpointId,
    request: &Request,
    body: &[u8],
) -> Result<Value> {
    call_addr(endpoint, mac.into(), request, body).await
}

pub async fn call_addr(
    endpoint: &Endpoint,
    mac: EndpointAddr,
    request: &Request,
    body: &[u8],
) -> Result<Value> {
    let conn = tokio::time::timeout(CONNECT_TIMEOUT, endpoint.connect(mac, ALPN))
        .await
        .map_err(|_| anyhow!(OFFLINE))?
        .map_err(|e| anyhow!("{OFFLINE} ({e})"))?;
    let result = exchange(&conn, request, body).await;
    conn.close(0u32.into(), b"done");
    result
}

/// A connection to the Mac, to keep open for dictation.
pub async fn connect(endpoint: &Endpoint, mac: EndpointAddr) -> Result<Connection> {
    tokio::time::timeout(CONNECT_TIMEOUT, endpoint.connect(mac, ALPN))
        .await
        .map_err(|_| anyhow!(OFFLINE))?
        .map_err(|e| anyhow!("{OFFLINE} ({e})"))
}

/// One request over an open connection (the keyboard's link): `Ping` on
/// keyboard open also has the Mac load its speech model.
pub async fn call_on(conn: &Connection, request: &Request, body: &[u8]) -> Result<Value> {
    exchange(conn, request, body).await
}

/// Dictates through the Mac: sends `audio` (16 kHz mono blocks) as it
/// comes; the channel closing is the stop. Calls `on_text` with every
/// update; returns once the final text is in, or the Mac's error.
pub async fn dictate(
    conn: &Connection,
    mut audio: tokio::sync::mpsc::UnboundedReceiver<Vec<i16>>,
    mut on_text: impl FnMut(DictationText),
) -> Result<()> {
    use tokio::io::AsyncBufReadExt;
    let (mut send, recv) = conn.open_bi().await.map_err(|e| anyhow!("{e}"))?;
    let header = serde_json::to_vec(&Request::Dictate)?;
    send.write_all(&(header.len() as u32).to_be_bytes())
        .await
        .map_err(|e| anyhow!("{e}"))?;
    send.write_all(&header).await.map_err(|e| anyhow!("{e}"))?;
    let writer = tokio::spawn(async move {
        while let Some(block) = audio.recv().await {
            let bytes: Vec<u8> = block.iter().flat_map(|s| s.to_le_bytes()).collect();
            if send
                .write_all(&(bytes.len() as u32).to_be_bytes())
                .await
                .is_err()
                || send.write_all(&bytes).await.is_err()
            {
                return;
            }
        }
        let _ = send.write_all(&0u32.to_be_bytes()).await;
        let _ = send.finish();
    });
    let mut lines = tokio::io::BufReader::new(recv).lines();
    let result = loop {
        match lines.next_line().await {
            Ok(Some(l)) => {
                let v: Value = serde_json::from_str(&l).context("the Mac's answer is damaged")?;
                if let Some(e) = v.get("error").and_then(Value::as_str) {
                    break Err(anyhow!("{e}"));
                }
                let t: DictationText = serde_json::from_value(v)?;
                let last = t.is_final;
                on_text(t);
                if last {
                    break Ok(());
                }
            }
            Ok(None) => break Err(anyhow!("The Mac stopped answering.")),
            Err(e) => break Err(anyhow!("{e}")),
        }
    };
    writer.abort();
    result
}

pub const OFFLINE: &str = "Can't reach the Mac. Is it on and is Sokki running?";

async fn exchange(conn: &Connection, request: &Request, body: &[u8]) -> Result<Value> {
    let (mut send, mut recv) = conn.open_bi().await.map_err(|e| anyhow!("{e}"))?;
    let header = serde_json::to_vec(request)?;
    send.write_all(&(header.len() as u32).to_be_bytes())
        .await
        .map_err(|e| anyhow!("{e}"))?;
    send.write_all(&header).await.map_err(|e| anyhow!("{e}"))?;
    send.write_all(body)
        .await
        .map_err(|e| anyhow!("sending failed: {e}"))?;
    send.finish().map_err(|e| anyhow!("{e}"))?;
    let answer = recv
        .read_to_end(MAX_ANSWER)
        .await
        .map_err(|e| anyhow!("{e}"))?;
    let v: Value = serde_json::from_slice(&answer).context("the Mac's answer is damaged")?;
    if v["ok"].as_bool() == Some(true) {
        Ok(v["data"].clone())
    } else {
        bail!("{}", v["error"].as_str().unwrap_or("The Mac refused."))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[test]
    fn tickets_round_trip_and_reject_junk() {
        let mac = new_secret().public();
        let t = Ticket {
            mac,
            code: "ABCD-EFGH-JKLM".into(),
        };
        assert_eq!(Ticket::parse(&t.encode()).unwrap(), t);
        assert_eq!(Ticket::parse(&t.url()).unwrap(), t);
        assert_eq!(Ticket::parse(&format!(" {} ", t.url())).unwrap(), t);
        assert!(Ticket::parse("hello").is_err());
        assert!(Ticket::parse("sokki1:nothex:CODE").is_err());
        assert!(Ticket::parse(&format!("sokki1:{mac}:")).is_err());
    }

    #[test]
    fn keys_round_trip() {
        let k = new_secret();
        assert_eq!(
            secret_from_hex(&secret_to_hex(&k)).unwrap().public(),
            k.public()
        );
        assert!(secret_from_hex("abc").is_err());
    }

    struct Mac {
        paired: Mutex<Vec<String>>,
    }

    impl Handler for Mac {
        fn is_paired(&self, peer: &str) -> bool {
            self.paired.lock().unwrap().iter().any(|p| p == peer)
        }
        fn pair(&self, peer: &str, code: &str, _device: &str) -> Result<Value, String> {
            if code != "RIGHT" {
                return Err("Wrong pairing code.".into());
            }
            self.paired.lock().unwrap().push(peer.to_string());
            Ok(json!({ "name": "Test Mac" }))
        }
        fn handle(&self, _peer: &str, request: Request, body: Vec<u8>) -> Result<Value, String> {
            match request {
                Request::Upload { .. } => Ok(json!({ "id": "m1", "bytes": body.len() })),
                Request::Status { id } => Ok(json!({ "id": id, "status": "done" })),
                _ => Ok(json!({ "pong": true })),
            }
        }
        fn dictate(
            &self,
            _peer: &str,
            audio: std::sync::mpsc::Receiver<Vec<f32>>,
            text: &dyn Fn(DictationText),
        ) -> Result<(), String> {
            let mut heard = 0u64;
            for block in audio {
                heard += block.len() as u64;
                text(DictationText {
                    is_final: false,
                    heard,
                    text: format!("{heard}"),
                });
            }
            text(DictationText {
                is_final: true,
                heard,
                text: format!("all {heard}"),
            });
            Ok(())
        }
    }

    /// Local endpoints without relays or discovery: the phone dials the
    /// Mac's direct addresses.
    async fn local() -> Endpoint {
        Endpoint::builder(presets::Minimal)
            .relay_mode(iroh::RelayMode::Disabled)
            .alpns(vec![ALPN.to_vec()])
            .bind()
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn pairing_then_upload_and_status() {
        let mac_ep = local().await;
        let addr = mac_ep.addr();
        let router = serve(
            mac_ep,
            Arc::new(Mac {
                paired: Mutex::new(vec![]),
            }),
        );
        let phone = local().await;

        // Not paired yet: refused.
        let err = call_addr(&phone, addr.clone(), &Request::Ping, &[])
            .await
            .unwrap_err();
        assert!(err.to_string().contains("isn't paired"), "{err}");
        // Wrong code: refused.
        let pair = |code: &str| Request::Pair {
            code: code.into(),
            device: "Pixel".into(),
        };
        let err = call_addr(&phone, addr.clone(), &pair("WRONG"), &[])
            .await
            .unwrap_err();
        assert!(err.to_string().contains("Wrong pairing code"), "{err}");
        // Right code, then everything works.
        let v = call_addr(&phone, addr.clone(), &pair("RIGHT"), &[])
            .await
            .unwrap();
        assert_eq!(v["name"], "Test Mac");
        let audio = vec![7u8; 3 * 1024 * 1024];
        let v = call_addr(
            &phone,
            addr.clone(),
            &Request::Upload {
                started_at: "2026-11-18T10:00:00Z".into(),
                mime: "audio/webm".into(),
            },
            &audio,
        )
        .await
        .unwrap();
        assert_eq!(v["bytes"], audio.len());
        let v = call_addr(&phone, addr, &Request::Status { id: "m1".into() }, &[])
            .await
            .unwrap();
        assert_eq!(v["status"], "done");
        router.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn dictation_streams_both_ways() {
        let mac_ep = local().await;
        let addr = mac_ep.addr();
        let router = serve(
            mac_ep,
            Arc::new(Mac {
                paired: Mutex::new(vec![]),
            }),
        );
        let phone = local().await;
        let pair = Request::Pair {
            code: "RIGHT".into(),
            device: "Pixel".into(),
        };
        call_addr(&phone, addr.clone(), &pair, &[]).await.unwrap();
        let conn = connect(&phone, addr).await.unwrap();
        // Twice on one connection, as the keyboard does.
        for _ in 0..2 {
            let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
            for _ in 0..3 {
                tx.send(vec![1i16; 3200]).unwrap();
            }
            drop(tx);
            let mut got = vec![];
            dictate(&conn, rx, |t| got.push(t)).await.unwrap();
            let last = got.last().unwrap();
            assert!(last.is_final);
            assert_eq!(last.text, "all 9600");
            assert_eq!(got.len(), 4);
        }
        router.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn dictation_refused_when_not_paired() {
        let mac_ep = local().await;
        let addr = mac_ep.addr();
        let router = serve(
            mac_ep,
            Arc::new(Mac {
                paired: Mutex::new(vec![]),
            }),
        );
        let phone = local().await;
        let conn = connect(&phone, addr).await.unwrap();
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        drop(tx);
        let err = dictate(&conn, rx, |_| {}).await.unwrap_err();
        assert!(err.to_string().contains("isn't paired"), "{err}");
        router.shutdown().await.unwrap();
    }

    /// Over the internet: the phone knows only the Mac's key; n0's DNS and
    /// relays do the rest. `cargo test -- --ignored over_n0`
    #[tokio::test]
    #[ignore]
    async fn over_n0_by_key_only() {
        let mac_ep = bind(new_secret()).await.unwrap();
        mac_ep.online().await;
        let mac = mac_ep.id();
        let router = serve(
            mac_ep,
            Arc::new(Mac {
                paired: Mutex::new(vec![]),
            }),
        );
        let phone = bind(new_secret()).await.unwrap();
        let t = std::time::Instant::now();
        let pair = Request::Pair {
            code: "RIGHT".into(),
            device: "Pixel".into(),
        };
        let v = call(&phone, mac, &pair, &[]).await.unwrap();
        assert_eq!(v["name"], "Test Mac");
        println!("paired in {:?}", t.elapsed());
        let t = std::time::Instant::now();
        let audio = vec![7u8; 10 * 1024 * 1024];
        let req = Request::Upload {
            started_at: "x".into(),
            mime: "audio/webm".into(),
        };
        let v = call(&phone, mac, &req, &audio).await.unwrap();
        assert_eq!(v["bytes"], audio.len());
        println!("10 MB in {:?}", t.elapsed());
        router.shutdown().await.unwrap();
    }
}
