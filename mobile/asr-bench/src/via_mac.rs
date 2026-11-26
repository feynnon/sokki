//! Dictation through the Mac: the phone sends the microphone audio over the
//! encrypted link (iroh, as `crates/sokki-link`), the Mac reads it with
//! Parakeet on its GPU and sends the text back. Measures, on the phone, how
//! long the text takes, the same way as `--stream`.
//!
//! Mac:   asr-bench --mac-serve <parakeet.gguf>     (prints its address)
//! Phone: asr-bench --via-mac <address> <clip.wav>  (VB_RELAY=1: relay only)
//!
//! The Mac does what Mac Sokki does with Parakeet Ultra ("Preview"): every
//! 0.5 s it re-reads the piece still growing (pieces end at 15 s here,
//! Sokki cuts at pauses) and sends the text so far; at the stop it reads the
//! whole dictation once, so the final text is the model's best.
//!
//! Wire, one QUIC stream: phone → Mac frames of [u32 big-endian byte count]
//! [16-bit PCM, 16 kHz mono], count 0 ends the dictation. Mac → phone JSON
//! lines {"final": bool, "heard": samples read, "text": …}.

use std::sync::mpsc;
use std::time::{Duration, Instant};

use iroh::endpoint::presets;
use iroh::{Endpoint, EndpointAddr, SecretKey};
use tokio::io::AsyncBufReadExt;

const ALPN: &[u8] = b"sokki/dictate-test/1";
const RATE: usize = 16_000;
const PREVIEW_EVERY: Duration = Duration::from_millis(500);
const PIECE: usize = 15 * RATE;

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("runtime")
}

pub fn serve(model_path: &str) {
    let t = Instant::now();
    let model = transcribe_cpp::Model::load_with(
        std::path::Path::new(model_path),
        &transcribe_cpp::ModelOptions::default(),
    )
    .expect("load model");
    let mut session = model.session().expect("session");
    let opts = transcribe_cpp::RunOptions::default();
    session.run(&vec![0.0f32; RATE], &opts).ok();
    eprintln!("model ready in {} ms", t.elapsed().as_millis());

    let rt = rt();
    let ep = rt.block_on(async {
        let ep = Endpoint::builder(presets::N0)
            .secret_key(SecretKey::generate())
            .alpns(vec![ALPN.to_vec()])
            .bind()
            .await
            .expect("bind");
        let _ = tokio::time::timeout(Duration::from_secs(10), ep.online()).await;
        ep
    });
    println!("ADDR {}", serde_json::to_string(&ep.addr()).unwrap());

    // One dictation at a time, read on this thread (the model isn't Send).
    let (jobs_tx, jobs_rx) = mpsc::channel::<(
        mpsc::Receiver<Option<Vec<f32>>>,
        tokio::sync::mpsc::UnboundedSender<String>,
    )>();
    rt.spawn(async move {
        while let Some(incoming) = ep.accept().await {
            let jobs_tx = jobs_tx.clone();
            tokio::spawn(async move {
                let Ok(conn) = incoming.await else { return };
                while let Ok((mut send, mut recv)) = conn.accept_bi().await {
                    let (audio_tx, audio_rx) = mpsc::channel();
                    let (text_tx, mut text_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
                    jobs_tx.send((audio_rx, text_tx)).ok();
                    let writer = tokio::spawn(async move {
                        while let Some(line) = text_rx.recv().await {
                            if send.write_all(line.as_bytes()).await.is_err() {
                                break;
                            }
                        }
                        let _ = send.finish();
                    });
                    loop {
                        let mut len = [0u8; 4];
                        if recv.read_exact(&mut len).await.is_err() {
                            break;
                        }
                        let n = u32::from_be_bytes(len) as usize;
                        if n == 0 {
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
                        audio_tx.send(Some(pcm)).ok();
                    }
                    audio_tx.send(None).ok();
                    let _ = writer.await;
                }
            });
        }
    });

    for (audio_rx, text_tx) in jobs_rx {
        let mut audio: Vec<f32> = Vec::new();
        let (mut piece_start, mut done_text) = (0usize, String::new());
        let mut last_read = 0usize;
        let mut ended = false;
        let send = |final_: bool, heard: usize, text: &str| {
            let line = serde_json::json!({"final": final_, "heard": heard, "text": text});
            text_tx.send(format!("{line}\n")).ok();
        };
        let started = Instant::now();
        while !ended {
            // Everything that has arrived; wait for more at most one interval.
            match audio_rx.recv_timeout(PREVIEW_EVERY) {
                Ok(Some(a)) => audio.extend(a),
                Ok(None) | Err(mpsc::RecvTimeoutError::Disconnected) => ended = true,
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
            while let Ok(m) = audio_rx.try_recv() {
                match m {
                    Some(a) => audio.extend(a),
                    None => ended = true,
                }
            }
            if ended || audio.len() < last_read + PREVIEW_EVERY.as_millis() as usize * 16 {
                continue;
            }
            last_read = audio.len();
            let piece = &audio[piece_start..];
            let text = session
                .run(piece, &opts)
                .map(|r| r.text.trim().to_string())
                .unwrap_or_default();
            let shown = format!("{done_text} {text}").trim().to_string();
            send(false, audio.len(), &shown);
            if piece.len() >= PIECE {
                done_text = shown;
                piece_start = audio.len();
            }
        }
        let t = Instant::now();
        let text = session
            .run(&audio, &opts)
            .map(|r| r.text.trim().to_string())
            .unwrap_or_default();
        send(true, audio.len(), &text);
        eprintln!(
            "dictation: {:.1} s heard, final read {} ms, {} s in all",
            audio.len() as f64 / RATE as f64,
            t.elapsed().as_millis(),
            started.elapsed().as_secs()
        );
    }
}

pub fn dictate(addr: &str, samples: Vec<f32>) {
    let addr: EndpointAddr = serde_json::from_str(addr).expect("address (JSON from --mac-serve)");
    let relay_only = std::env::var("VB_RELAY").is_ok();
    let show = std::env::var("VB_SHOW").is_ok();
    let mut samples = samples;
    // The same quiet tail as `--stream`: the person stops 0.8 s after speaking.
    samples
        .extend((0..(0.8 * RATE as f32) as usize).map(|i| 0.0005 * ((i * 7919 % 13) as f32 - 6.0)));
    let audio_s = samples.len() as f64 / RATE as f64;
    rt().block_on(async move {
        let mut b = Endpoint::builder(presets::N0).secret_key(SecretKey::generate());
        let addr = if relay_only {
            b = b.clear_ip_transports();
            EndpointAddr::from_parts(addr.id, addr.addrs.iter().filter(|a| a.is_relay()).cloned())
        } else {
            addr
        };
        let ep = b.bind().await.expect("bind");
        let t = Instant::now();
        let conn = ep.connect(addr, ALPN).await.expect("connect");
        println!("connected in {} ms", t.elapsed().as_millis());
        // The keyboard would connect when it opens; give hole punching its
        // moment, as the person would take to start speaking.
        tokio::time::sleep(Duration::from_secs(2)).await;
        for p in conn.paths().iter() {
            if p.is_selected() {
                println!(
                    "path: {} (rtt {} ms)",
                    if p.is_ip() { "direct" } else { "relay" },
                    p.rtt().as_millis()
                );
            }
        }
        let (mut send, recv) = conn.open_bi().await.expect("stream");
        let start = Instant::now();
        let now = move || start.elapsed().as_secs_f64();
        let sender = tokio::spawn(async move {
            let mut fed = 0usize;
            while fed < samples.len() {
                let next = (fed + RATE / 5).min(samples.len());
                let due = next as f64 / RATE as f64;
                let t = now();
                if t < due {
                    tokio::time::sleep(Duration::from_secs_f64(due - t)).await;
                }
                let bytes: Vec<u8> = samples[fed..next]
                    .iter()
                    .flat_map(|s| ((s * 32767.0) as i16).to_le_bytes())
                    .collect();
                send.write_all(&(bytes.len() as u32).to_be_bytes())
                    .await
                    .expect("send");
                send.write_all(&bytes).await.expect("send");
                fed = next;
            }
            send.write_all(&0u32.to_be_bytes()).await.expect("send");
            let _ = send.finish();
        });
        let mut lines = tokio::io::BufReader::new(recv).lines();
        let (mut first, mut lags, mut last_words) = (None::<f64>, Vec::<f64>::new(), 0usize);
        let mut final_text = String::new();
        while let Ok(Some(line)) = lines.next_line().await {
            let v: serde_json::Value = serde_json::from_str(&line).expect("json");
            let text = v["text"].as_str().unwrap_or_default().to_string();
            let t = now();
            if v["final"].as_bool() == Some(true) {
                println!("after stop: {:.2} s", t - audio_s);
                final_text = text;
                break;
            }
            let words = text.split_whitespace().count();
            // Behind the speech: now vs when the last audio this text read was spoken.
            let lag = t - v["heard"].as_u64().unwrap_or(0) as f64 / RATE as f64;
            if words > 0 {
                first.get_or_insert(t);
                lags.push(lag);
            }
            if words != last_words {
                last_words = words;
                println!(
                    "{t:6.2} s  {words:3} words  lag {lag:4.2} s  {}",
                    if show { text.as_str() } else { "" }
                );
            }
        }
        let _ = sender.await;
        if show {
            println!("final: {final_text}");
        }
        if let Ok(p) = std::env::var("VB_SCRIPT") {
            let script = std::fs::read_to_string(p).unwrap_or_default();
            println!(
                "vs script: {:.1} % off",
                crate::wer(&script, &final_text) * 100.0
            );
        }
        lags.sort_by(|a, b| a.partial_cmp(b).unwrap());
        println!(
            "first words at {:.2} s; text lags speech by median {:.2} s, worst {:.2} s",
            first.unwrap_or(f64::NAN),
            lags.get(lags.len() / 2).copied().unwrap_or(f64::NAN),
            lags.last().copied().unwrap_or(f64::NAN),
        );
        conn.close(0u32.into(), b"done");
    });
}
