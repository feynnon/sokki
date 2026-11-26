//! asr-bench <model.gguf> <audio.wav> [threads]
//! Transcribes a 16 kHz mono WAV in 20 s chunks and prints load time, speed
//! (real-time factor) and peak memory. Built for Android with cargo-ndk and
//! run over adb, to see whether a phone can transcribe meetings by itself.
use std::time::Instant;

mod via_mac;

fn peak_rss_mb() -> f64 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("VmHWM:"))
                .and_then(|l| l.split_whitespace().nth(1).map(str::to_string))
        })
        .and_then(|kb| kb.parse::<f64>().ok())
        .map(|kb| kb / 1024.0)
        .unwrap_or(f64::NAN)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    // Dictation through the Mac (src/via_mac.rs).
    if args[1] == "--mac-serve" {
        let _ = transcribe_cpp::init_backends_default();
        via_mac::serve(&args[2]);
        return;
    }
    if args[1] == "--via-mac" {
        let r = hound::WavReader::open(&args[3]).expect("wav");
        let samples = r
            .into_samples::<i16>()
            .map(|s| s.unwrap() as f32 / 32768.0)
            .collect();
        via_mac::dictate(&args[2], samples);
        return;
    }
    let (model, wav) = (&args[1], &args[2]);
    let _ = transcribe_cpp::init_backends_default();
    let mut r = hound::WavReader::open(wav).expect("wav");
    let spec = r.spec();
    assert!(
        spec.sample_rate == 16_000 && spec.channels == 1,
        "need 16 kHz mono"
    );
    let samples: Vec<f32> = r
        .samples::<i16>()
        .map(|s| s.unwrap() as f32 / 32768.0)
        .collect();
    let audio_s = samples.len() as f64 / 16_000.0;

    let t = Instant::now();
    let model = transcribe_cpp::Model::load_with(
        std::path::Path::new(model),
        &transcribe_cpp::ModelOptions::default(),
    )
    .expect("load model");
    let mut session = model.session().expect("session");
    let load_s = t.elapsed().as_secs_f64();

    // VB_LANG=de: the language, instead of detecting it.
    let opts = transcribe_cpp::RunOptions {
        language: std::env::var("VB_LANG").ok(),
        ..Default::default()
    };

    // VB_PAGEOUT=1: after the warm-up, push this process's memory out of RAM
    // (model pages dropped, buffers to swap), as Android does to an idle
    // keyboard. Then the first dictation pays for reading it back.
    let pageout = || {
        if std::env::var("VB_PAGEOUT").is_ok() {
            let t = Instant::now();
            let n = page_out();
            std::thread::sleep(std::time::Duration::from_millis(500));
            println!("paged out {n} MB in {} ms", t.elapsed().as_millis());
        }
    };

    // Parakeet on the phone, live: the growing piece is read again every
    // VB_EVERY ms (default 500) for live text; pieces end at pauses
    // (VB_MIN s, VB_MAX s, VB_PAUSE ms; dictation-core) and are read once
    // more when they end, then kept; at the stop only the last piece is
    // left. One engine, one read at a time, in real time (VB_NOWAIT=1: as
    // fast as it goes, for the text). VB_THREADS, VB_SHOW, VB_SCRIPT.
    if args.get(3).map(String::as_str) == Some("--pieces") {
        use dictation_core::{Mode, Params, Segmenter};
        let env_f = |k: &str, d: f32| {
            std::env::var(k)
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(d)
        };
        if let Some(n) = std::env::var("VB_THREADS")
            .ok()
            .and_then(|v| v.parse().ok())
        {
            session = model
                .session_with(&transcribe_cpp::SessionOptions {
                    n_threads: n,
                    ..Default::default()
                })
                .expect("session");
        }
        let params = Params {
            min_s: env_f("VB_MIN", 6.0),
            max_s: env_f("VB_MAX", 15.0),
            pause_ms: env_f("VB_PAUSE", 300.0) as u32,
            ..Default::default()
        };
        let every = (env_f("VB_EVERY", 500.0) * 16.0) as usize;
        let nowait = std::env::var("VB_NOWAIT").is_ok();
        let show = std::env::var("VB_SHOW").is_ok();
        let mut samples = samples.clone();
        samples.extend(
            (0..(0.8 * 16_000.0) as usize).map(|i| 0.0005 * ((i * 7919 % 13) as f32 - 6.0)),
        );
        let len = samples.len();
        let audio_s = len as f64 / 16_000.0;
        let _ = session.run(&vec![0.0f32; 16_000], &opts); // warm-up, as on keyboard open
        let mut read = |a: &[f32]| {
            session
                .run(a, &opts)
                .map(|r| r.text.trim().to_string())
                .unwrap_or_default()
        };
        let mut seg = Segmenter::new(Mode::Pauses(params));
        let mut queue: std::collections::VecDeque<Vec<f32>> = Default::default();
        let (mut fed, mut committed, mut last_len) = (0usize, String::new(), 0usize);
        let (mut first, mut lags, mut reads, mut busy) =
            (None::<f64>, Vec::<f64>::new(), 0usize, 0.0f64);
        let join = |a: &str, b: &str| format!("{a} {b}").trim().to_string();
        let start = Instant::now();
        let now = || start.elapsed().as_secs_f64();
        loop {
            let target = if nowait {
                (fed + 3_200).min(len)
            } else {
                ((now() * 16_000.0) as usize).min(len)
            };
            if target > fed {
                queue.extend(seg.push(&samples[fed..target]));
                fed = target;
            }
            let covered = fed;
            let t0 = now();
            let shown = if let Some(p) = queue.pop_front() {
                let t = read(&p);
                committed = join(&committed, &t);
                last_len = 0;
                committed.clone()
            } else if fed == len {
                break;
            } else {
                let pend = seg.pending();
                if pend.len() < last_len {
                    last_len = 0;
                }
                if pend.len() >= last_len + every && seg.quiet_from() > 0 {
                    last_len = pend.len();
                    let live = read(pend);
                    join(&committed, &live)
                } else {
                    if !nowait {
                        std::thread::sleep(std::time::Duration::from_millis(20));
                    }
                    continue;
                }
            };
            reads += 1;
            busy += now() - t0;
            if !shown.is_empty() {
                let t = now();
                first.get_or_insert(t);
                lags.push(t - covered as f64 / 16_000.0);
                if show {
                    println!("{t:6.2} s  {} words", shown.split_whitespace().count());
                }
            }
        }
        // Stopped (the person stopped at audio_s): the pieces still
        // waiting, then the last one.
        while let Some(p) = queue.pop_front() {
            committed = join(&committed, &read(&p));
        }
        if let Some(p) = seg.finish() {
            let end = seg.speech_end(&p).max(1).min(p.len());
            committed = join(&committed, &read(&p[..end]));
        }
        println!("after stop: {:.2} s", now() - audio_s);
        if show {
            println!("final: {committed}");
        }
        if let Ok(p) = std::env::var("VB_SCRIPT") {
            let script = std::fs::read_to_string(p).unwrap_or_default();
            println!("vs script: {:.1} % off", wer(&script, &committed) * 100.0);
        }
        lags.sort_by(|a, b| a.partial_cmp(b).unwrap());
        println!(
            "first words at {:.2} s; text lags speech by median {:.2} s, worst {:.2} s; {reads} reads, engine busy {:.0} %",
            first.unwrap_or(f64::NAN),
            lags.get(lags.len() / 2).copied().unwrap_or(f64::NAN),
            lags.last().copied().unwrap_or(f64::NAN),
            busy / audio_s * 100.0
        );
        return;
    }

    // Streaming, as the Mac does it with a streaming model: 200 ms blocks
    // in real time into the model's own stream, which keeps its context.
    // Prints when the text changes and how far it lags behind the speech.
    // VB_RIGHT=n: lookahead frames (cache-aware models, 80 ms each);
    // VB_BUF="left_ms:chunk_ms:right_ms": buffered streaming (other
    // Parakeets). VB_THREADS: CPU threads. VB_SHOW=1 prints the text.
    if args.get(3).map(String::as_str) == Some("--stream") {
        use transcribe_cpp::{
            ParakeetBufferedStreamOptions, ParakeetStreamOptions, StreamExtension, StreamOptions,
        };
        let env_i = |k: &str| std::env::var(k).ok().and_then(|v| v.parse::<i32>().ok());
        let family = if let Ok(b) = std::env::var("VB_BUF") {
            let x: Vec<i32> = b.split(':').map(|n| n.parse().unwrap()).collect();
            Some(StreamExtension::ParakeetBuffered(
                ParakeetBufferedStreamOptions {
                    left_ms: Some(x[0]),
                    chunk_ms: Some(x[1]),
                    right_ms: Some(x[2]),
                },
            ))
        } else {
            env_i("VB_RIGHT").map(|r| {
                StreamExtension::ParakeetStream(ParakeetStreamOptions {
                    att_context_right: Some(r),
                })
            })
        };
        let so = StreamOptions {
            family,
            ..Default::default()
        };
        if let Some(n) = env_i("VB_THREADS") {
            session = model
                .session_with(&transcribe_cpp::SessionOptions {
                    n_threads: n,
                    ..Default::default()
                })
                .expect("session");
        }
        // VB_BLOCK_MS: audio per feed (default 200 ms).
        let block = env_i("VB_BLOCK_MS").unwrap_or(200) as usize * 16;
        let show = std::env::var("VB_SHOW").is_ok();
        let tail: f32 = std::env::var("VB_TAIL")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(0.8);
        let mut samples = samples.clone();
        samples.extend(
            (0..(tail * 16_000.0) as usize).map(|i| 0.0005 * ((i * 7919 % 13) as f32 - 6.0)),
        );
        let audio_s = samples.len() as f64 / 16_000.0;
        // Warm-up, as on keyboard open.
        {
            let mut st = session.stream(&opts, &so).expect("stream");
            for b in vec![0.0f32; 16_000].chunks(3_200) {
                st.feed(b).expect("feed");
            }
            st.finalize().expect("finalize");
        }
        pageout();
        // Where speech starts and the words' ends, from the script timing: the
        // lag is measured against the audio fed so far.
        let mut st = session.stream(&opts, &so).expect("stream");
        let start = Instant::now();
        let now = || start.elapsed().as_secs_f64();
        let (mut fed, mut first, mut lags, mut busy) =
            (0usize, None::<f64>, Vec::<f64>::new(), 0.0f64);
        let mut last_words = 0usize;
        let mut feeds = 0usize;
        let (mut prev, mut rewrites) = (String::new(), 0usize);
        let mut slowest = 0.0f64;
        while fed < samples.len() {
            // Real time: wait until the next 200 ms block has been "spoken".
            let next = (fed + block).min(samples.len());
            let due = next as f64 / 16_000.0;
            let t = now();
            // VB_NOWAIT=1: as fast as it goes (text only; timings meaningless).
            if t < due && std::env::var("VB_NOWAIT").is_err() {
                std::thread::sleep(std::time::Duration::from_secs_f64(due - t));
            }
            let t0 = now();
            st.feed(&samples[fed..next]).expect("feed");
            let took = now() - t0;
            busy += took;
            slowest = slowest.max(took);
            feeds += 1;
            fed = next;
            let text = st.text();
            // Words already shown that changed afterwards (the last, growing one aside).
            let shown = text.display();
            let done_words = |t: &str| t.rsplit_once(' ').map_or("", |(a, _)| a).to_string();
            let old_done = done_words(&prev);
            if !shown.starts_with(&old_done) {
                rewrites += 1;
                if show {
                    println!("        rewrite: {old_done:?} -> {shown:?}");
                }
            }
            prev = shown;
            let words = text.display().split_whitespace().count();
            if words != last_words {
                let t = now();
                let lag = t - fed as f64 / 16_000.0;
                if words > last_words {
                    first.get_or_insert(t);
                    lags.push(lag);
                }
                last_words = words;
                println!(
                    "{t:6.2} s  {words:3} words (committed {:3})  lag {lag:4.2} s  {}",
                    text.committed.split_whitespace().count(),
                    if show { text.display() } else { String::new() }
                );
            }
        }
        let t0 = now();
        st.finalize().expect("finalize");
        let text = st.text().display();
        println!(
            "after stop: {:.2} s (finalize {:.2} s)",
            now() - audio_s,
            now() - t0
        );
        if show {
            println!("final: {text}");
        }
        if let Some(p) = std::env::var("VB_SCRIPT").ok() {
            let script = std::fs::read_to_string(p).unwrap_or_default();
            println!("vs script: {:.1} % off", wer(&script, &text) * 100.0);
        }
        lags.sort_by(|a, b| a.partial_cmp(b).unwrap());
        println!(
            "first words at {:.2} s; text lags speech by median {:.2} s, worst {:.2} s; engine busy {:.0} % of the time, slowest feed {:.2} s; {rewrites} rewrites of shown words",
            first.unwrap_or(f64::NAN),
            lags.get(lags.len() / 2).copied().unwrap_or(f64::NAN),
            lags.last().copied().unwrap_or(f64::NAN),
            busy / audio_s * 100.0,
            slowest
        );
        let _ = feeds;
        return;
    }

    // Chunking: how much accuracy do pieces cost? Whole recording vs pieces,
    // word error rate against the whole transcript (and against the script
    // when given). Also: seconds of audio left when the speaker stops.
    if args.get(3).map(String::as_str) == Some("--chunking") {
        use dictation_core::{Mode, Params, Segmenter};
        let mut run = |audio: &[f32]| -> String {
            session
                .run(audio, &opts)
                .expect("run")
                .text
                .trim()
                .to_string()
        };
        // "Whole": 30 s windows (the model's comfortable maximum).
        let whole: Vec<String> = samples.chunks(16_000 * 30).map(|c| run(c)).collect();
        let whole = whole.join(" ");
        let script = args
            .get(4)
            .map(|p| std::fs::read_to_string(p).unwrap_or_default());
        println!(
            "{:<34} {:>8} {:>9} {:>7} {:>9}",
            "mode", "pieces", "avg len", "vs whole", "vs script"
        );
        // VB_MODES="f:5,p:4:240:10" = fixed 5 s, pauses with min 4 s, 240 ms, max 10 s.
        let spec = std::env::var("VB_MODES").unwrap_or("f:5,f:3,p:1.5:240:8,p:1:160:8".into());
        let modes: Vec<(String, Mode)> = spec
            .split(',')
            .map(|m| {
                let v: Vec<&str> = m.split(':').collect();
                if v[0] == "f" {
                    (
                        format!("fixed {} s", v[1]),
                        Mode::Fixed(v[1].parse().unwrap()),
                    )
                } else {
                    let (min_s, pause_ms, max_s) = (
                        v[1].parse().unwrap(),
                        v[2].parse().unwrap(),
                        v[3].parse().unwrap(),
                    );
                    // Optional 5th field: the margin above the background level.
                    let margin = v
                        .get(4)
                        .map(|m| m.parse().unwrap())
                        .unwrap_or(Params::default().margin);
                    (
                        format!("pauses min {min_s} s, {pause_ms} ms, max {max_s} s, +{margin}"),
                        Mode::Pauses(Params {
                            min_s,
                            pause_ms,
                            max_s,
                            margin,
                            ..Params::default()
                        }),
                    )
                }
            })
            .collect();
        let wer_script = |hyp: &str| script.as_ref().map(|s| wer(s, hyp) * 100.0);
        if let Some(w) = wer_script(&whole) {
            println!(
                "{:<34} {:>8} {:>9} {:>7} {:>8.1}%",
                "whole (reference)", "-", "-", "-", w
            );
        }
        for (name, mode) in &modes {
            let mode = *mode;
            let mut seg = Segmenter::new(mode);
            let mut pieces = Vec::new();
            for block in samples.chunks(1600) {
                pieces.extend(seg.push(block));
            }
            let tail = seg.finish();
            let tail_s = tail
                .as_ref()
                .map(|t| t.len() as f64 / 16_000.0)
                .unwrap_or(0.0);
            pieces.extend(tail);
            let n = pieces.len();
            let avg = samples.len() as f64 / 16_000.0 / n as f64;
            // Stopping at a random moment leaves the audio since the last cut:
            // on average sum(L²) / (2·sum(L)), at most the longest piece.
            let lens: Vec<f64> = pieces.iter().map(|p| p.len() as f64 / 16_000.0).collect();
            let left_avg =
                lens.iter().map(|l| l * l).sum::<f64>() / (2.0 * lens.iter().sum::<f64>());
            let left_max = lens.iter().cloned().fold(0.0, f64::max);
            let text: Vec<String> = pieces.iter().map(|p| run(p)).collect();
            let text = text.join(" ");
            let ws = wer_script(&text)
                .map(|w| format!("{w:.1}%"))
                .unwrap_or("-".into());
            println!(
                "{:<34} {:>8} {:>8.1}s {:>6.1}% {:>9}   (last piece {:.1} s; left at a random stop: avg {:.1} s, max {:.1} s)",
                name, n, avg, wer(&whole, &text) * 100.0, ws, tail_s, left_avg, left_max
            );
        }
        return;
    }

    // Live dictation as the Sokki keyboard does it (`src-tauri/src/keyboard_voice.rs`):
    // audio arrives in real time; finished pieces are transcribed first; when
    // idle, the growing piece is transcribed as a preview. Prints when words
    // show up and how far the text lags behind the speech.
    // VB_LIVE="min_s:max_s:first_s:step_s[:first_piece_min_s]" (default 6:10:0.6:0.5);
    // first_s 0 = no previews. VB_SHOW=1 prints text; VB_WER=1 compares the
    // final text with the whole recording transcribed at once.
    if args.get(3).map(String::as_str) == Some("--live") {
        use dictation_core::{Mode, Params, Segmenter};
        let cfg: Vec<f32> = std::env::var("VB_LIVE")
            .unwrap_or("6:10:0.6:0.5".into())
            .split(':')
            .map(|v| v.parse().unwrap())
            .collect();
        let (first, step) = ((cfg[2] * 16_000.0) as usize, (cfg[3] * 16_000.0) as usize);
        let show = std::env::var("VB_SHOW").is_ok();
        // People tap stop a moment after their last word: VB_TAIL seconds of quiet (default 0.8).
        let tail: f32 = std::env::var("VB_TAIL")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(0.8);
        let mut samples = samples.clone();
        samples.extend(
            (0..(tail * 16_000.0) as usize).map(|i| 0.0005 * ((i * 7919 % 13) as f32 - 6.0)),
        );
        let audio_s = samples.len() as f64 / 16_000.0;
        let _ = session.run(&vec![0.0f32; 16_000], &opts); // warm-up, as on keyboard open
        let previews = cfg[2] > 0.0;
        let normal = Mode::Pauses(Params {
            min_s: cfg[0],
            max_s: cfg[1],
            ..Params::default()
        });
        let first_min = cfg.get(4).copied().unwrap_or(cfg[0]);
        // First piece: from first_min at a pause, at the latest 2 s later (quietest spot).
        let first_max = if cfg.len() > 4 {
            first_min + 2.0
        } else {
            cfg[1]
        };
        let mut seg = Segmenter::new(Mode::Pauses(Params {
            min_s: first_min,
            max_s: first_max,
            ..Params::default()
        }));
        let whole = std::env::var("VB_WER").is_ok().then(|| {
            samples
                .chunks(16_000 * 30)
                .map(|c| session.run(c, &opts).expect("run").text.trim().to_string())
                .collect::<Vec<_>>()
                .join(" ")
        });
        let mut finals: Vec<String> = Vec::new();
        // Snippet mode, VB_CHUNK="min_s:max_s": live text from short snippets,
        // each transcribed once (cut at the quietest spot between min and max
        // after the last one), replaced by the final text of each piece.
        let chunk: Option<(usize, usize)> = std::env::var("VB_CHUNK").ok().map(|v| {
            let x: Vec<f32> = v.split(':').map(|n| n.parse().unwrap()).collect();
            ((x[0] * 16_000.0) as usize, (x[1] * 16_000.0) as usize)
        });
        let mut chunked = 0usize; // samples of the growing piece already in snippets
        let mut snippets: Vec<String> = Vec::new();
        let mut final_lags: Vec<f64> = Vec::new();
        fn quietest(audio: &[f32], lo: usize, hi: usize) -> usize {
            let rms = |f: &[f32]| f.iter().map(|s| s * s).sum::<f32>();
            (lo / 320..hi / 320)
                .min_by(|&a, &b| {
                    rms(&audio[a * 320..a * 320 + 320])
                        .partial_cmp(&rms(&audio[b * 320..b * 320 + 320]))
                        .unwrap()
                })
                .map_or(hi, |f| f * 320 + 160)
        }
        // (end in the recording, piece, where its speech ends)
        let mut queue: std::collections::VecDeque<(usize, Vec<f32>, usize)> = Default::default();
        let mut last_preview: Option<(usize, String)> = None;
        let mut stopped = false;
        let (mut reused, mut reran) = (0, 0);
        let (mut fed, mut cut_at, mut cuts, mut done, mut previewed) =
            (0usize, 0usize, 0usize, 0usize, 0usize);
        let mut first_words: Option<f64> = None;
        let mut lags: Vec<f64> = Vec::new();
        // VB_PAR="t1:t2": final pieces on a second session (own thread, t2 CPU
        // threads), snippets on the first (t1); 0 = library default.
        let par: Option<(i32, i32)> = std::env::var("VB_PAR").ok().map(|v| {
            let x: Vec<i32> = v.split(':').map(|n| n.parse().unwrap()).collect();
            (x[0], x[1])
        });
        let (job_tx, res_rx) = match par {
            Some((t1, t2)) => {
                let so = |n| transcribe_cpp::SessionOptions {
                    n_threads: n,
                    ..Default::default()
                };
                session = model.session_with(&so(t1)).expect("session");
                let _ = session.run(&vec![0.0f32; 16_000], &opts);
                let mut s2 = model.session_with(&so(t2)).expect("session");
                let _ = s2.run(
                    &vec![0.0f32; 16_000],
                    &transcribe_cpp::RunOptions::default(),
                );
                let (jt, jr) = std::sync::mpsc::channel::<(usize, Vec<f32>, usize)>();
                let (rt, rr) = std::sync::mpsc::channel::<(usize, usize, String)>();
                std::thread::spawn(move || {
                    let o = transcribe_cpp::RunOptions::default();
                    for (end, piece, speech_end) in jr {
                        let text = if speech_end == 0 {
                            String::new()
                        } else {
                            s2.run(&piece, &o).expect("run").text.trim().to_string()
                        };
                        let _ = rt.send((end, piece.len(), text));
                    }
                });
                (Some(jt), Some(rr))
            }
            None => (None, None),
        };
        let mut results = 0usize;
        pageout();
        let start = Instant::now();
        let now = || start.elapsed().as_secs_f64();
        loop {
            let avail = ((now() * 16_000.0) as usize).min(samples.len());
            for piece in seg.push(&samples[fed..avail]) {
                seg.set_mode(normal);
                chunked = chunked.saturating_sub(piece.len());
                cut_at += piece.len();
                let speech_end = seg.speech_end(&piece);
                queue.push_back((cut_at, piece, speech_end));
                cuts += 1;
            }
            fed = avail;
            let finished = fed == samples.len();
            if finished && !stopped {
                stopped = true;
                // Stopped: the words not in a snippet yet get one right away.
                if chunk.is_some() {
                    let pending = seg.pending();
                    if seg.speech_end(&pending[chunked.min(pending.len())..]) > 0 {
                        let audio = pending[chunked..].to_vec();
                        let text = session
                            .run(&audio, &opts)
                            .expect("run")
                            .text
                            .trim()
                            .to_string();
                        snippets.push(text.clone());
                        chunked = pending.len();
                        println!(
                            "{:6.2} s  snippet {:4.1} s audio (at stop)  {}",
                            now(),
                            audio.len() as f64 / 16_000.0,
                            if show {
                                text
                            } else {
                                format!("{} chars", text.chars().count())
                            }
                        );
                    }
                    println!("all words shown {:.2} s after stop", now() - audio_s);
                }
                if let Some(rest) = seg.finish() {
                    cut_at += rest.len();
                    let speech_end = seg.speech_end(&rest);
                    queue.push_back((cut_at, rest, speech_end));
                    cuts += 1;
                }
            }
            // Parallel: finished pieces go to the second session; collect its results.
            if let (Some(tx), Some(rx)) = (&job_tx, &res_rx) {
                while let Some(job) = queue.pop_front() {
                    let _ = tx.send(job);
                }
                while let Ok((end, len, text)) = rx.try_recv() {
                    let t = now();
                    let lag = t - end as f64 / 16_000.0;
                    final_lags.push(lag);
                    println!(
                        "{t:6.2} s  final   {:4.1} s audio  text lags {lag:4.2} s  {}",
                        len as f64 / 16_000.0,
                        if show {
                            text.clone()
                        } else {
                            format!("{} chars", text.chars().count())
                        }
                    );
                    finals.push(text);
                    results += 1;
                    reran += 1;
                    if stopped && results == cuts {
                        println!("after stop: {:.2} s", t - audio_s);
                    }
                }
                if stopped && results == cuts {
                    break;
                }
            }
            // A snippet first: it is short, and it keeps the live text going.
            if let Some((cmin, cmax)) = chunk {
                let pending = seg.pending();
                // The first snippet comes sooner: 0.6–1 s.
                let (cmin, cmax) = if snippets.is_empty() {
                    (9_600, 16_000)
                } else {
                    (cmin, cmax)
                };
                if !finished && pending.len() >= chunked + cmax {
                    let cut = quietest(pending, chunked + cmin, pending.len());
                    let audio = pending[chunked..cut].to_vec();
                    let abs_end = cut_at + cut;
                    let text = session
                        .run(&audio, &opts)
                        .expect("run")
                        .text
                        .trim()
                        .to_string();
                    let t = now();
                    let lag = t - abs_end as f64 / 16_000.0;
                    chunked = cut;
                    if !text.is_empty() {
                        lags.push(lag);
                        first_words.get_or_insert(t);
                    }
                    println!(
                        "{t:6.2} s  snippet {:4.1} s audio  text lags {lag:4.2} s  {}",
                        audio.len() as f64 / 16_000.0,
                        if show {
                            text.clone()
                        } else {
                            format!("{} chars", text.chars().count())
                        }
                    );
                    snippets.push(text);
                    continue;
                }
            }
            if let Some((end, piece, speech_end)) = queue.pop_front() {
                let reuse = match last_preview.take() {
                    _ if speech_end == 0 => Some(String::new()),
                    Some((len, text)) if len >= speech_end => Some(text),
                    _ => None,
                };
                if reuse.is_some() {
                    reused += 1
                } else {
                    reran += 1
                }
                let text = reuse.unwrap_or_else(|| {
                    session
                        .run(&piece, &opts)
                        .expect("run")
                        .text
                        .trim()
                        .to_string()
                });
                finals.push(text.clone());
                let t = now();
                let lag = t - end as f64 / 16_000.0;
                if chunk.is_some() {
                    final_lags.push(lag)
                } else {
                    lags.push(lag);
                    first_words.get_or_insert(t);
                }
                println!(
                    "{t:6.2} s  final   {:4.1} s audio  text lags {lag:4.2} s  {}",
                    piece.len() as f64 / 16_000.0,
                    if show {
                        text
                    } else {
                        format!("{} chars", text.chars().count())
                    }
                );
                done += 1;
                previewed = 0;
                if finished && queue.is_empty() {
                    println!("after stop: {:.2} s", t - audio_s);
                    break;
                }
                continue;
            }
            if finished && queue.is_empty() && job_tx.is_none() {
                break;
            }
            let pending = seg.pending();
            let speech_end = seg.quiet_from();
            let pausing = pending.len() >= speech_end + 1_600;
            if previews
                && cuts == done
                && speech_end > previewed
                && pending.len() >= first
                && (pausing || pending.len() >= previewed + step)
            {
                let audio = pending.to_vec();
                let end = fed;
                let text = session
                    .run(&audio, &opts)
                    .expect("run")
                    .text
                    .trim()
                    .to_string();
                let t = now();
                let lag = t - end as f64 / 16_000.0;
                previewed = audio.len();
                last_preview = Some((audio.len(), text.clone()));
                if !text.is_empty() {
                    lags.push(lag);
                    first_words.get_or_insert(t);
                }
                println!(
                    "{t:6.2} s  preview {:4.1} s audio  text lags {lag:4.2} s  {}",
                    audio.len() as f64 / 16_000.0,
                    if show {
                        text
                    } else {
                        format!("{} chars", text.chars().count())
                    }
                );
                continue;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        if let Some(whole) = &whole {
            println!(
                "vs whole recording: {:.1} % off",
                wer(whole, &finals.join(" ")) * 100.0
            );
            if chunk.is_some() {
                println!(
                    "live snippets alone vs whole: {:.1} % off",
                    wer(whole, &snippets.join(" ")) * 100.0
                );
            }
        }
        if !final_lags.is_empty() {
            final_lags.sort_by(|a, b| a.partial_cmp(b).unwrap());
            println!(
                "final text after each cut: median {:.2} s, worst {:.2} s",
                final_lags[final_lags.len() / 2],
                final_lags[final_lags.len() - 1]
            );
        }
        lags.sort_by(|a, b| a.partial_cmp(b).unwrap());
        println!(
            "pieces: {reused} from the preview, {reran} transcribed again; first words at {:.2} s; text lags speech by median {:.2} s, worst {:.2} s",
            first_words.unwrap_or(f64::NAN),
            lags[lags.len() / 2],
            lags[lags.len() - 1]
        );
        return;
    }

    // Dictation: how long after you stop speaking is the text there? The model
    // stays loaded, so only the transcription counts. Median of 3 runs.
    if args.get(3).map(String::as_str) == Some("--dictation") {
        // VB_THREADS: CPU threads (default: the engine's choice).
        if let Some(n) = std::env::var("VB_THREADS")
            .ok()
            .and_then(|v| v.parse().ok())
        {
            session = model
                .session_with(&transcribe_cpp::SessionOptions {
                    n_threads: n,
                    ..Default::default()
                })
                .expect("session");
        }
        let _ = session.run(&samples[..16_000 * 2], &opts); // warm-up
        for secs in [1usize, 2, 3, 5, 10, 30] {
            let clip = &samples[..(16_000 * secs).min(samples.len())];
            let mut times: Vec<f64> = (0..3)
                .map(|_| {
                    let t = Instant::now();
                    session.run(clip, &opts).expect("run");
                    t.elapsed().as_secs_f64()
                })
                .collect();
            times.sort_by(|a, b| a.partial_cmp(b).unwrap());
            println!("dictation {secs:>2} s -> {:.2} s", times[1]);
        }
        // While speaking: 5 s pieces as they come in; after you stop, only the
        // last piece is left. Fine if each piece takes less than 5 s.
        let mut worst: f64 = 0.0;
        let mut last = 0.0;
        for piece in samples[..16_000 * 60].chunks(16_000 * 5) {
            let t = Instant::now();
            session.run(piece, &opts).expect("run");
            last = t.elapsed().as_secs_f64();
            worst = worst.max(last);
        }
        println!("pieces of 5 s: slowest {worst:.2} s, last {last:.2} s (keeps up if < 5 s)");
        println!("peak memory {:.0} MB", peak_rss_mb());
        return;
    }

    let t = Instant::now();
    let mut text = Vec::new();
    for chunk in samples.chunks(16_000 * 20) {
        let out = session.run(chunk, &opts).expect("run");
        text.push(out.text.trim().to_string());
    }
    let run_s = t.elapsed().as_secs_f64();
    println!(
        "audio {audio_s:.1} s | load {load_s:.1} s | transcribe {run_s:.1} s | {:.2}x real time | peak memory {:.0} MB",
        audio_s / run_s,
        peak_rss_mb()
    );
    println!("{}", text.join(" "));
}

/// Asks the kernel to page out every private or file mapping of this
/// process (MADV_PAGEOUT, Linux 5.4+): file pages are dropped, anonymous
/// ones go to swap. Returns the MB asked for.
#[cfg(target_os = "linux")]
fn page_out() -> usize {
    page_out_maps()
}
#[cfg(target_os = "android")]
fn page_out() -> usize {
    page_out_maps()
}
#[cfg(not(any(target_os = "linux", target_os = "android")))]
fn page_out() -> usize {
    0
}
#[cfg(any(target_os = "linux", target_os = "android"))]
fn page_out_maps() -> usize {
    unsafe extern "C" {
        fn madvise(addr: *mut std::ffi::c_void, len: usize, advice: i32) -> i32;
    }
    const MADV_PAGEOUT: i32 = 21;
    let maps = std::fs::read_to_string("/proc/self/maps").unwrap_or_default();
    let mut total = 0;
    for line in maps.lines() {
        let mut it = line.split_whitespace();
        let (Some(range), Some(perms)) = (it.next(), it.next()) else {
            continue;
        };
        let name = line.split_whitespace().nth(5).unwrap_or("");
        // The model file, and the heap / anonymous buffers; not the stack or code.
        let wanted = name.ends_with(".gguf")
            || (perms.starts_with("rw") && (name.is_empty() || name.starts_with("[anon")));
        if !wanted {
            continue;
        }
        let Some((a, b)) = range.split_once('-') else {
            continue;
        };
        let (a, b) = (
            usize::from_str_radix(a, 16).unwrap_or(0),
            usize::from_str_radix(b, 16).unwrap_or(0),
        );
        if b > a && unsafe { madvise(a as *mut _, b - a, MADV_PAGEOUT) } == 0 {
            total += b - a;
        }
    }
    total / 1_048_576
}

/// Word error rate of `hyp` against `reference`, lower-cased, punctuation
/// removed.
fn wer(reference: &str, hyp: &str) -> f64 {
    let norm = |t: &str| -> Vec<String> {
        t.to_lowercase()
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || c.is_whitespace() {
                    c
                } else {
                    ' '
                }
            })
            .collect::<String>()
            .split_whitespace()
            .map(str::to_string)
            .collect()
    };
    let (r, h) = (norm(reference), norm(hyp));
    if r.is_empty() {
        return 0.0;
    }
    let mut prev: Vec<usize> = (0..=h.len()).collect();
    for i in 1..=r.len() {
        let mut cur = vec![i; h.len() + 1];
        for j in 1..=h.len() {
            let sub = prev[j - 1] + usize::from(r[i - 1] != h[j - 1]);
            cur[j] = sub.min(prev[j] + 1).min(cur[j - 1] + 1);
        }
        prev = cur;
    }
    prev[h.len()] as f64 / r.len() as f64
}
