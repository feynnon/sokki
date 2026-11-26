//! How many speakers one real recording comes out with, and why: the
//! app's way (windows joined by `meeting_core::stitch`, as during a call)
//! and the whole track at once, with each voice's speech time, its
//! similarity to the other voices on the track and to the saved voice
//! profiles. Prints numbers and generic labels only, no meeting content.
//!
//!   cargo run --release --example speaker_count -- <meeting dir>
//!
//! The meeting dir holds `mic.flac`, `system.flac` (optional) and
//! `timing.json`, as the app records them. Needs ffmpeg.
//! Env: MERGE=0.75 merges voices on one track at that similarity inside
//! the diarizer (`DiarizeConfig::merge_similarity`); NO_AEC=1 skips echo
//! cancellation; SAME_VOICE=off runs the pipeline without joining same
//! voices; USER_PROFILE=<i> makes voice profile i the user; NO_WHOLE=1 skips the whole-track run; PROFILES=<file> compares with other voice profiles.
//! With MEETING_ASR_MODEL (the app's model file) it also runs the
//! meeting pipeline on the stitched speakers, as the app does after the
//! stop, and prints the speakers the note gets; OUT=<file> writes that
//! transcript as JSON (keep it outside the repository).
use meeting_core::stitch::{StitchConfig, Stitcher};
use meeting_core::{Diarizer, SpeakerTurn, Track, TrackKind};
use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

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

fn slice(x: &[f32], a: f64, b: f64) -> &[f32] {
    let i = ((a * 16_000.0) as usize).min(x.len());
    let j = ((b * 16_000.0) as usize).min(x.len());
    &x[i..j.max(i)]
}

/// Per speaker: speech seconds and its (first) embedding.
fn voices(turns: &[SpeakerTurn]) -> BTreeMap<String, (f64, Option<Vec<f32>>)> {
    let mut m: BTreeMap<String, (f64, Option<Vec<f32>>)> = BTreeMap::new();
    for t in turns {
        let e = m.entry(t.speaker.clone()).or_insert((0.0, None));
        e.0 += t.end_s - t.start_s;
        if e.1.is_none() {
            e.1 = t.embedding.clone();
        }
    }
    m
}

fn report(label: &str, turns: &[SpeakerTurn], profiles: Option<&diarize::VoiceProfiles>) {
    let v = voices(turns);
    println!("  {label}: {} speaker(s)", v.len());
    for (name, (speech, emb)) in &v {
        let mut line = format!("    {name}: {speech:6.1}s");
        if let Some(e) = emb {
            for (other, (_, f)) in &v {
                if other != name {
                    if let Some(f) = f {
                        line += &format!("  vs {other} {:.2}", diarize::cosine(e, f));
                    }
                }
            }
            if let Some(p) = profiles {
                let scores = p.scores(e);
                for (i, name) in p.names().iter().enumerate() {
                    if let Some((_, sim)) = scores.iter().find(|(n, _)| n == name) {
                        line += &format!("  profile#{i} {sim:.2}");
                    }
                }
            }
        }
        println!("{line}");
    }
}

fn main() -> anyhow::Result<()> {
    let dir = std::env::args().nth(1).expect("meeting dir");
    let dir = Path::new(&dir);
    transcribe_cpp::init_backends_default().map_err(|e| anyhow::anyhow!("{e}"))?;
    let mut tracks = vec![Track {
        kind: TrackKind::Mic,
        samples: decode(&dir.join("mic.flac"))?,
    }];
    if dir.join("system.flac").exists() {
        tracks.push(Track {
            kind: TrackKind::System,
            samples: decode(&dir.join("system.flac"))?,
        });
    }
    // Align as the app does (`sokki_engine::tracks::track_pads`): the later
    // track is padded however late it started.
    // Echo is removed from where the system audio starts, as in the app.
    let mut echo_from = 0usize;
    if let Ok(t) = std::fs::read_to_string(dir.join("timing.json")) {
        let v: serde_json::Value = serde_json::from_str(&t)?;
        let mic = v["mic_first_us"].as_u64().unwrap_or(0) as i64;
        let sys = v["system_first_us"].as_u64().unwrap_or(0) as i64;
        if mic > 0 && sys > 0 && tracks.len() == 2 {
            let off = mic - sys;
            let later = if off > 0 { 0 } else { 1 };
            let pad = (off.unsigned_abs() * 16_000 / 1_000_000) as usize;
            let mut p = vec![0.0f32; pad];
            p.extend_from_slice(&tracks[later].samples);
            tracks[later].samples = p;
            if later == 1 {
                echo_from = pad;
            }
            println!(
                "aligned: {} padded {:.1}s",
                if later == 0 { "mic" } else { "system" },
                pad as f64 / 16_000.0
            );
        }
    }
    if std::env::var_os("NO_AEC").is_none() {
        if meeting_core::aec::echo_applies(&tracks, echo_from) {
            let ran = meeting_core::aec::cancel_echo_in_tracks(&mut tracks, echo_from);
            println!(
                "echo cancellation from {:.1}s: {ran}",
                echo_from as f64 / 16_000.0
            );
        }
    }
    let home = std::env::var("HOME")?;
    let models = Path::new(&home).join("Library/Caches/sokki/diarize-models");
    let mut cfg = diarize::DiarizeConfig::new(&models);
    cfg.allow_download = false;
    if let Ok(m) = std::env::var("MERGE") {
        cfg.merge_similarity = Some(m.parse()?);
        println!("merge_similarity: {m}");
    }
    let d = diarize::SortformerDiarizer::new(cfg)?;
    let profiles_path = std::env::var("PROFILES").unwrap_or_else(|_| {
        format!("{home}/Library/Application Support/org.sokki.app/voice-profiles.json")
    });
    let profiles = diarize::VoiceProfiles::open(&profiles_path).ok();
    let sc = StitchConfig::default();
    let mut stitched: Vec<(TrackKind, Vec<SpeakerTurn>)> = Vec::new();
    for t in &tracks {
        let len_s = t.samples.len() as f64 / 16_000.0;
        println!("{:?} track, {len_s:.0}s", t.kind);
        // Windows as during the call, then the rest at the stop.
        let mut st = Stitcher::new(sc);
        let mut have = 0.0;
        while have < len_s {
            have = (have + 1.0).min(len_s);
            if let Some((a, b)) = sc.next_window(st.last_end_s(), have) {
                let w = d.diarize(slice(&t.samples, a, b))?;
                report(&format!("window {a:.0}-{b:.0}"), &w, None);
                st.add_window(a, b, w, false);
            }
        }
        let a = st.last_end_s().map_or(0.0, |e| (e - sc.overlap_s).max(0.0));
        let w = d.diarize(slice(&t.samples, a, len_s))?;
        report(&format!("window {a:.0}-{len_s:.0} (last)"), &w, None);
        st.add_window(a, len_s, w, true);
        report("stitched (the app)", &st.turns(), profiles.as_ref());
        let (joined, n) = meeting_core::stitch::merge_same_voice(st.turns(), 0.75);
        if n > 0 {
            report(
                &format!("stitched, same voices joined ({n})"),
                &joined,
                profiles.as_ref(),
            );
        }
        stitched.push((t.kind, st.turns()));
        if std::env::var_os("NO_WHOLE").is_none() {
            let whole = d.diarize(&t.samples)?;
            report("whole track", &whole, profiles.as_ref());
        }
    }
    if std::env::var_os("MEETING_ASR_MODEL").is_none() {
        return Ok(());
    }
    let Some(asr) = meeting_e2e::EngineTranscriber::from_env()? else {
        return Ok(());
    };
    let names: Vec<String> = profiles.as_ref().map(|p| p.names()).unwrap_or_default();
    let namer = |_: TrackKind, _: &str, e: Option<&[f32]>| -> Option<String> {
        let (n, _) = profiles.as_ref()?.identify(e?)?;
        Some(format!("profile#{}", names.iter().position(|x| *x == n)?))
    };
    let mut pcfg = meeting_core::pipeline::PipelineConfig::default();
    // The user's own voice profile, as the app's user name.
    if let Ok(i) = std::env::var("USER_PROFILE") {
        pcfg.user_name = format!("profile#{i}");
    }
    if std::env::var("SAME_VOICE").is_ok_and(|v| v == "off") {
        pcfg.same_voice = None;
    }
    let r = meeting_core::pipeline::run_with(
        &tracks,
        &d,
        &asr,
        &pcfg,
        meeting_core::pipeline::Hooks {
            namer: Some(&namer),
            turns: Some(&stitched),
            ..Default::default()
        },
    )?;
    println!(
        "note: {} speaker(s), {} lines, {} joined as the same voice (same_voice {:?})",
        r.speakers.len(),
        r.transcript.segments.len(),
        r.voices_merged,
        pcfg.same_voice
    );
    for s in &r.speakers {
        let lines = r
            .transcript
            .segments
            .iter()
            .filter(|x| x.speaker == s.name)
            .count();
        println!(
            "  {:?} {} ({}): {:.1}s, {lines} lines",
            s.track, s.name, s.cluster, s.speech_s
        );
    }
    if let Ok(out) = std::env::var("OUT") {
        std::fs::write(out, serde_json::to_string_pretty(&r.transcript)?)?;
    }
    Ok(())
}
