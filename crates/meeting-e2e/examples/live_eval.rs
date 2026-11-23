//! Transcript during the call vs after it, on one real recording: what is
//! left after the stop, and how far the words differ from today's
//! after-the-fact transcript. Prints numbers only, no meeting content;
//! transcripts go to `out_dir` (keep it outside the repository).
//!
//!   cargo run --release --example live_eval -- <meeting dir> <out dir> [min_s max_s]
//!
//! The meeting dir holds `mic.flac`, `system.flac` (optional) and
//! `timing.json`, as the app records them. Needs ffmpeg and
//! `MEETING_ASR_MODEL` (the app's model file).
use meeting_core::live::{place_words, CutConfig, Piece, PieceCutter};
use meeting_core::pipeline::{self, Hooks, PipelineConfig};
use meeting_core::{Diarizer, SpeakerTurn, Track, TrackKind, Transcriber, Transcript};
use meeting_e2e::*;
use std::path::Path;
use std::process::Command;
use std::time::Instant;

fn decode(path: &Path) -> anyhow::Result<Vec<f32>> {
    let out = Command::new("ffmpeg")
        .args(["-v", "error", "-i"])
        .arg(path)
        .args(["-f", "f32le", "-ac", "1", "-ar", "16000", "-"])
        .output()?;
    anyhow::ensure!(out.status.success(), "ffmpeg failed on {}", path.display());
    Ok(out
        .stdout
        .chunks_exact(4)
        .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .collect())
}

/// `sokki_engine::tracks::align_tracks`, which this crate can't depend on.
fn align(tracks: &mut [Track], mic_us: u64, sys_us: u64) {
    if mic_us == 0 || sys_us == 0 || tracks.len() < 2 {
        return;
    }
    let off = mic_us as i64 - sys_us as i64;
    // System audio may start any time later (the tap waits for sound).
    if off > 5_000_000 {
        return;
    }
    let pad = (off.unsigned_abs() * 16_000 / 1_000_000) as usize;
    let later = if off > 0 {
        TrackKind::Mic
    } else {
        TrackKind::System
    };
    if let Some(t) = tracks.iter_mut().find(|t| t.kind == later) {
        let mut p = vec![0.0f32; pad];
        p.extend_from_slice(&t.samples);
        t.samples = p;
    }
}

/// Returns turns diarized earlier, per track in order.
struct Cached(std::sync::Mutex<std::collections::VecDeque<Vec<SpeakerTurn>>>);
impl Diarizer for Cached {
    fn diarize(&self, _s: &[f32]) -> anyhow::Result<Vec<SpeakerTurn>> {
        Ok(self.0.lock().unwrap().pop_front().unwrap_or_default())
    }
}

fn track_text(t: &Transcript, kind: TrackKind) -> String {
    t.segments
        .iter()
        .filter(|s| s.track == kind)
        .map(|s| s.text.as_str())
        .collect::<Vec<_>>()
        .join(" ")
}

fn env_f64(name: &str, default: f64) -> f64 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn slice(x: &[f32], a: f64, b: f64) -> &[f32] {
    let i = ((a * 16_000.0) as usize).min(x.len());
    let j = ((b * 16_000.0) as usize).min(x.len());
    &x[i..j.max(i)]
}

fn count_speakers(t: &[SpeakerTurn]) -> usize {
    let mut s: Vec<&str> = t.iter().map(|t| t.speaker.as_str()).collect();
    s.sort();
    s.dedup();
    s.len()
}

/// Frame by frame (100 ms): the share of frames where both agree on speech
/// or silence, and among frames both call speech, the share with the same
/// speaker once each of `b`'s speakers is mapped to the `a` speaker it
/// shares most time with (one to one).
fn agreement(a: &[SpeakerTurn], b: &[SpeakerTurn], dur_s: f64) -> (f64, f64) {
    let n = (dur_s * 10.0) as usize;
    let label = |turns: &[SpeakerTurn]| -> Vec<Option<String>> {
        let mut v = vec![None; n];
        for t in turns {
            let (i, j) = ((t.start_s * 10.0) as usize, ((t.end_s * 10.0) as usize).min(n));
            for x in v.iter_mut().take(j).skip(i) {
                *x = Some(t.speaker.clone());
            }
        }
        v
    };
    let (la, lb) = (label(a), label(b));
    let mut pairs: std::collections::HashMap<(String, String), usize> = Default::default();
    let (mut same_speech, mut both) = (0usize, 0usize);
    for (x, y) in la.iter().zip(&lb) {
        if x.is_some() == y.is_some() {
            same_speech += 1;
        }
        if let (Some(x), Some(y)) = (x, y) {
            both += 1;
            *pairs.entry((x.clone(), y.clone())).or_default() += 1;
        }
    }
    let mut v: Vec<((String, String), usize)> = pairs.into_iter().collect();
    v.sort_by(|p, q| q.1.cmp(&p.1));
    let (mut used_a, mut used_b, mut hit) = (Vec::new(), Vec::new(), 0usize);
    for ((x, y), c) in v {
        if used_a.contains(&x) || used_b.contains(&y) {
            continue;
        }
        used_a.push(x);
        used_b.push(y);
        hit += c;
    }
    (
        same_speech as f64 / n.max(1) as f64,
        hit as f64 / both.max(1) as f64,
    )
}

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let dir = Path::new(&args[0]);
    let out = Path::new(&args[1]);
    std::fs::create_dir_all(out)?;
    let cut = CutConfig {
        min_s: args.get(2).map_or(8.0, |a| a.parse().unwrap()),
        max_s: args.get(3).map_or(25.0, |a| a.parse().unwrap()),
    };

    let t0 = Instant::now();
    let mut tracks = vec![Track {
        kind: TrackKind::Mic,
        samples: decode(&dir.join("mic.flac"))?,
    }];
    let sys = dir.join("system.flac");
    if sys.exists() {
        let s = decode(&sys)?;
        if !s.is_empty() {
            tracks.push(Track {
                kind: TrackKind::System,
                samples: s,
            });
        }
    }
    let decode_s = t0.elapsed().as_secs_f64();
    if let Ok(t) = std::fs::read_to_string(dir.join("timing.json")) {
        let v: serde_json::Value = serde_json::from_str(&t)?;
        align(
            &mut tracks,
            v["mic_first_us"].as_u64().unwrap_or(0),
            v["system_first_us"].as_u64().unwrap_or(0),
        );
    }
    let has_system = tracks.len() > 1;
    let duration_s = tracks.iter().map(|t| t.duration_s()).fold(0.0, f64::max);

    // Today, after the stop: echo cancellation on the whole track.
    let raw_mic = tracks[0].samples.clone();
    let t0 = Instant::now();
    let aec_ran = meeting_core::aec::cancel_echo_in_tracks(&mut tracks, 0);
    let aec_s = t0.elapsed().as_secs_f64();

    // Speakers (cached across runs: they are the same either way).
    let turns_file = out.join("turns.json");
    let mut diarize_s = f64::NAN;
    let turns: Vec<Vec<SpeakerTurn>> = match std::fs::read_to_string(&turns_file) {
        Ok(t) => serde_json::from_str(&t)?,
        Err(_) => {
            let models = Path::new(&std::env::var("HOME")?)
                .join("Library/Caches/sokki/diarize-models");
            let d = diarize::SortformerDiarizer::new(diarize::DiarizeConfig::new(&models))?;
            let t0 = Instant::now();
            let mut all = Vec::new();
            for t in &tracks {
                all.push(d.diarize(&t.samples)?);
            }
            diarize_s = t0.elapsed().as_secs_f64();
            std::fs::write(&turns_file, serde_json::to_string(&all)?)?;
            all
        }
    };
    let cached = || Cached(std::sync::Mutex::new(turns.clone().into()));

    let asr = EngineTranscriber::from_env()?.expect("speech model (MEETING_ASR_MODEL)");
    let cfg = PipelineConfig {
        user_name: "Ich".into(),
        ..Default::default()
    };
    // Warm the model up so neither path pays for loading it.
    let _ = asr.transcribe(&vec![0.0; 16_000]);

    // Today's transcript and its time (cached across runs).
    let today_file = out.join("today.json");
    let (today_transcript, today_jobs, today_s): (Transcript, usize, f64) =
        match std::fs::read_to_string(&today_file) {
            Ok(t) => serde_json::from_str(&t)?,
            Err(_) => {
                let t0 = Instant::now();
                let r = pipeline::run(&tracks, &cached(), &asr, &cfg)?;
                let v = (r.transcript, r.jobs, t0.elapsed().as_secs_f64());
                std::fs::write(&today_file, serde_json::to_string(&v)?)?;
                v
            }
        };

    // During the call: the mic is echo-cancelled whenever system audio is
    // recorded (live can't know yet whether it stays silent).
    let mut live_mic = raw_mic;
    if has_system {
        meeting_core::aec::cancel_echo(&mut live_mic, &tracks[1].samples);
    }
    let pieces_file = out.join(format!("pieces_{}_{}.json", cut.min_s, cut.max_s));
    type Cache = (
        Vec<(TrackKind, f64, f64, Vec<(f64, f64, String)>)>,
        f64,
        f64,
        f64,
    );
    let mut pieces: Vec<Piece> = Vec::new();
    let (mut live_s, mut tail_s, mut live_audio_s) = (0.0, 0.0, 0.0);
    if let Ok(t) = std::fs::read_to_string(&pieces_file) {
        let (ps, a, b, c): Cache = serde_json::from_str(&t)?;
        (live_s, tail_s, live_audio_s) = (a, b, c);
        pieces = ps
            .into_iter()
            .map(|(track, start_s, end_s, words)| Piece {
                track,
                start_s,
                end_s,
                words: words
                    .into_iter()
                    .map(|(start_s, end_s, text)| meeting_core::TimedWord {
                        start_s,
                        end_s,
                        text,
                    })
                    .collect(),
            })
            .collect();
    }
    let cached_pieces = !pieces.is_empty();
    for t in tracks.iter().filter(|_| !cached_pieces) {
        let samples = if t.kind == TrackKind::Mic {
            &live_mic
        } else {
            &t.samples
        };
        let mut c = PieceCutter::new(cut);
        let mut ps = c.push(samples);
        let last = c.finish();
        let n_live = ps.len();
        ps.extend(last);
        for (k, (at, audio)) in ps.into_iter().enumerate() {
            let start_s = at as f64 / 16_000.0;
            let t0 = Instant::now();
            let words = asr.transcribe_words(&audio)?.unwrap_or_default();
            let took = t0.elapsed().as_secs_f64();
            if k < n_live {
                live_s += took;
                live_audio_s += audio.len() as f64 / 16_000.0;
            } else {
                tail_s += took;
            }
            pieces.push(Piece {
                track: t.kind,
                start_s,
                end_s: start_s + audio.len() as f64 / 16_000.0,
                words: place_words(words, start_s),
            });
        }
    }

    let cache: Cache = (
        pieces
            .iter()
            .map(|p| {
                let w = p
                    .words
                    .iter()
                    .map(|w| (w.start_s, w.end_s, w.text.clone()))
                    .collect();
                (p.track, p.start_s, p.end_s, w)
            })
            .collect(),
        live_s,
        tail_s,
        live_audio_s,
    );
    std::fs::write(&pieces_file, serde_json::to_string(&cache)?)?;
    let n_pieces = pieces.len();

    let mut report = serde_json::json!({
        "meeting": dir.file_name().unwrap().to_string_lossy(),
        "duration_min": duration_s / 60.0,
        "has_system": has_system,
        "aec_ran_after_stop": aec_ran,
        "cut": [cut.min_s, cut.max_s],
        "after_stop_today_s": {
            "decode": decode_s, "aec": aec_s, "diarize": diarize_s, "transcribe": today_s,
        },
        "jobs": today_jobs,
        "pieces": n_pieces,
        "during_call_transcribe_s": live_s,
        "during_call_audio_s": live_audio_s,
        "tail_transcribe_s": tail_s,
    });
    for any in [false, true] {
        let cfg = PipelineConfig {
            pieces_any_speaker: any,
            ..cfg.clone()
        };
        let t0 = Instant::now();
        let new = pipeline::run_with(
            &tracks,
            &cached(),
            &asr,
            &cfg,
            Hooks {
                pieces: Some(&pieces),
                ..Default::default()
            },
        )?;
        let new_s = t0.elapsed().as_secs_f64();
        let name = if any { "any_speaker" } else { "one_speaker" };
        std::fs::write(
            out.join(format!("new_{name}.json")),
            serde_json::to_string_pretty(&new.transcript)?,
        )?;
        let mut w = serde_json::Map::new();
        for kind in [TrackKind::Mic, TrackKind::System] {
            let a = track_text(&today_transcript, kind);
            let b = track_text(&new.transcript, kind);
            let n = normalize_words(&a).len();
            if n > 0 {
                w.insert(
                    format!("{kind:?}"),
                    serde_json::json!({"words_today": n, "word_diff": wer(&a, &b)}),
                );
            }
        }
        report[name] = serde_json::json!({
            "jobs_from_pieces": new.jobs_from_pieces,
            "transcribe_after_stop_s": new_s + tail_s,
            "word_diff_vs_today": w,
            "speakers_today": today_transcript.speakers().len(),
            "speakers_new": new.transcript.speakers().len(),
        });
    }
    // Speakers during the call too (LIVE_SPEAKERS=1): windows joined by
    // `meeting_core::stitch`, against today's speakers of the whole track.
    if std::env::var_os("LIVE_SPEAKERS").is_some() {
        use meeting_core::stitch::{StitchConfig, Stitcher};
        let sc = StitchConfig {
            step_s: env_f64("STITCH_STEP", 300.0),
            overlap_s: env_f64("STITCH_OVERLAP", 60.0),
            guard_s: env_f64("STITCH_GUARD", 30.0),
            same_voice: env_f64("STITCH_SAME_VOICE", 0.6) as f32,
        };
        let models = Path::new(&std::env::var("HOME")?)
            .join("Library/Caches/sokki/diarize-models");
        let d = diarize::SortformerDiarizer::new(diarize::DiarizeConfig::new(&models))?;
        let mut stitched: Vec<(TrackKind, Vec<SpeakerTurn>)> = Vec::new();
        let mut per_track = serde_json::Map::new();
        for (ti, t) in tracks.iter().enumerate() {
            let len_s = t.duration_s();
            let mut st = Stitcher::new(sc);
            let (mut during, mut windows) = (0.0, 0);
            let mut have = 0.0;
            while have < len_s {
                have = (have + 1.0).min(len_s);
                if let Some((a, b)) = sc.next_window(st.last_end_s(), have) {
                    let t0 = Instant::now();
                    let w = d.diarize(slice(&t.samples, a, b))?;
                    during += t0.elapsed().as_secs_f64();
                    windows += 1;
                    st.add_window(a, b, w, false);
                }
            }
            let a = st.last_end_s().map_or(0.0, |e| (e - sc.overlap_s).max(0.0));
            let t0 = Instant::now();
            let w = d.diarize(slice(&t.samples, a, len_s))?;
            let after = t0.elapsed().as_secs_f64();
            st.add_window(a, len_s, w, true);
            let got = st.turns();
            let (speech, acc) = agreement(&turns[ti], &got, len_s);
            per_track.insert(
                format!("{:?}", t.kind),
                serde_json::json!({
                    "windows": windows + 1,
                    "during_call_s": during,
                    "after_stop_s": after,
                    "speakers_today": count_speakers(&turns[ti]),
                    "speakers_live": count_speakers(&got),
                    "speech_agrees": speech,
                    "same_speaker": acc,
                }),
            );
            stitched.push((t.kind, got));
        }
        std::fs::write(out.join("stitched.json"), serde_json::to_string(&stitched)?)?;
        let new = pipeline::run_with(
            &tracks,
            &cached(),
            &asr,
            &cfg,
            Hooks {
                pieces: Some(&pieces),
                turns: Some(&stitched),
                ..Default::default()
            },
        )?;
        std::fs::write(
            out.join("new_live_speakers.json"),
            serde_json::to_string_pretty(&new.transcript)?,
        )?;
        let as_turns = |tr: &Transcript, kind: TrackKind| -> Vec<SpeakerTurn> {
            tr.segments
                .iter()
                .filter(|s| s.track == kind)
                .map(|s| SpeakerTurn {
                    start_s: s.start_s,
                    end_s: s.end_s,
                    speaker: s.speaker.clone(),
                    embedding: None,
                })
                .collect()
        };
        let mut words = serde_json::Map::new();
        for kind in [TrackKind::Mic, TrackKind::System] {
            let a = track_text(&today_transcript, kind);
            if normalize_words(&a).is_empty() {
                continue;
            }
            let (_, acc) = agreement(
                &as_turns(&today_transcript, kind),
                &as_turns(&new.transcript, kind),
                duration_s,
            );
            words.insert(
                format!("{kind:?}"),
                serde_json::json!({
                    "word_diff": wer(&a, &track_text(&new.transcript, kind)),
                    "same_speaker_in_lines": acc,
                }),
            );
        }
        report["live_speakers"] = serde_json::json!({
            "config": [sc.step_s, sc.overlap_s, sc.guard_s, sc.same_voice],
            "tracks": per_track,
            "transcript_vs_today": words,
            "speakers_today": today_transcript.speakers(),
            "speakers_live": new.transcript.speakers(),
        });
    }
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}
