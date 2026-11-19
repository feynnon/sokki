//! `sokki process <meeting-id>`: turns one recorded meeting into a note.
//!
//! Runs as a child of the daemon, so a model crash or a memory spike never
//! takes the service, a recording or dictation down. Progress goes to
//! stdout as JSON lines (`{"stage":"transcribing","progress":0.4}`), the
//! result as a last line (`{"done":true}` or `{"error":"…"}`).

use anyhow::{Context, Result};
use meeting_core::{Track, TrackKind, SAMPLE_RATE};
use std::path::Path;
use std::sync::{Arc, Mutex};

use crate::config::Config;
use crate::store::{Status, Store};

fn emit(v: serde_json::Value) {
    use std::io::Write;
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{v}");
    let _ = out.flush();
}

pub fn main(id: &str) -> i32 {
    match run(id) {
        Ok(()) => {
            emit(serde_json::json!({ "done": true }));
            0
        }
        Err(e) => {
            let msg = format!("{e:#}");
            log::error!("Meeting {id} failed: {msg}");
            let store = Store::new(crate::paths::meetings_dir());
            let _ = store.update(id, |r| {
                r.status = Status::Failed;
                r.error = Some(msg.clone());
            });
            emit(serde_json::json!({ "error": msg }));
            1
        }
    }
}

fn run(id: &str) -> Result<()> {
    let cfg = Config::load(&crate::paths::config_file())?;
    let store = Store::new(crate::paths::meetings_dir());
    let record = store.update(id, |r| r.status = Status::Processing)?;
    let dir = store.dir(id);

    let mut tracks = Vec::new();
    for file in &record.tracks {
        let kind = if file.starts_with("mic.") {
            TrackKind::Mic
        } else if file.starts_with("system.") {
            TrackKind::System
        } else {
            continue;
        };
        let path = dir.join(file);
        if !path.exists() {
            continue;
        }
        let samples = decode_flac(&path).with_context(|| format!("decoding {}", path.display()))?;
        tracks.push(Track { kind, samples });
    }
    let timing = std::fs::read_to_string(dir.join(sokki_engine::tracks::TIMING_FILE))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok());

    // A local Ollama that is installed but not running is started, so the
    // note gets its summary (the person chose a local model; nothing else
    // needs it).
    if cfg.meetings.summary || cfg.meetings.correction {
        let status = sokki_engine::llm::check(&cfg.llm, &cfg.meetings.user_name, false);
        if status.state == "not_running" {
            match sokki_engine::llm::start_ollama(&cfg.llm) {
                Ok(()) => log::info!("Started Ollama for the summary"),
                Err(e) => log::warn!("Could not start Ollama: {e:#}"),
            }
        }
    }

    // Speech recogniser and speaker detection share transcribe.cpp's
    // backends (Vulkan if a GPU is there, else CPU).
    emit(serde_json::json!({ "stage": "preparing" }));
    let models = crate::paths::models_dir();
    let mut transcriber = sokki_engine::asr::LocalTranscriber::load(
        &crate::models::ensure(&crate::models::PARAKEET, |_, _| {})?,
    )?;
    let lang = cfg.dictation.language.trim();
    if !lang.is_empty() {
        transcriber.set_language(Some(lang.to_string()));
    }
    let notes = Arc::new(Mutex::new(Vec::new()));
    let mut warnings = record.warnings.clone();
    let diarizer: Box<dyn meeting_core::Diarizer> = if !cfg.meetings.speakers {
        Box::new(sokki_engine::tracks::WholeTrack)
    } else {
        match diarize::SortformerDiarizer::new(diarize::DiarizeConfig::new(&models)) {
            Ok(d) => Box::new(d),
            Err(e) => {
                log::warn!("Speakers not separated: {e:#}");
                meeting_core::pipeline::warn_once(
                    &mut warnings,
                    meeting_core::pipeline::SPEAKERS_NOT_SEPARATED,
                );
                Box::new(sokki_engine::tracks::WholeTrack)
            }
        }
    };
    let profiles = diarize::VoiceProfiles::open(crate::paths::voice_profiles()).ok();
    let namer = |_t: TrackKind, _c: &str, e: Option<&[f32]>| -> Option<String> {
        profiles.as_ref()?.identify(e?).map(|(name, _)| name)
    };
    let progress = |stage: sokki_engine::Stage, p: Option<f32>| {
        emit(serde_json::json!({ "stage": stage.as_str(), "progress": p }));
    };
    let out = sokki_engine::run(
        sokki_engine::Input {
            tracks,
            timing,
            started_at: record.started_at.clone(),
            source_app: record.source_app.clone(),
            previous_note: record.note_path.as_deref().map(Into::into),
            warnings,
        },
        sokki_engine::Deps {
            transcriber: &transcriber,
            diarizer,
            diarizer_notes: Some(notes),
            namer: Some(&namer),
            progress: &progress,
        },
        &cfg.engine(),
    )?;
    sokki_engine::speakers::save(&dir, &out.speaker_info);

    let delete_audio = cfg.meetings.keep_audio_days == 0;
    if delete_audio {
        for f in &record.tracks {
            let _ = std::fs::remove_file(dir.join(f));
        }
    }
    store.update(id, |r| {
        r.status = Status::Done;
        r.title = Some(out.title.clone());
        r.note_path = Some(out.note_path.to_string_lossy().into_owned());
        r.speakers = out.speakers.clone();
        r.warnings = out.warnings.clone();
        r.duration_s = out.duration_s;
        r.error = None;
        r.audio_deleted |= delete_audio;
    })?;
    Ok(())
}

/// A FLAC file as 16 kHz mono f32. The recorder already writes 16 kHz mono;
/// other rates are converted by linear interpolation (good enough for
/// speech, and only for files from elsewhere).
pub fn decode_flac(path: &Path) -> Result<Vec<f32>> {
    let mut reader = claxon::FlacReader::open(path)?;
    let info = reader.streaminfo();
    let channels = info.channels as usize;
    let scale = (1i64 << (info.bits_per_sample - 1)) as f32;
    let mut mono = Vec::with_capacity(info.samples.unwrap_or(0) as usize);
    let mut frame = Vec::with_capacity(channels);
    for s in reader.samples() {
        frame.push(s? as f32 / scale);
        if frame.len() == channels {
            mono.push(frame.iter().sum::<f32>() / channels as f32);
            frame.clear();
        }
    }
    let rate = info.sample_rate;
    if rate == SAMPLE_RATE {
        return Ok(mono);
    }
    let ratio = rate as f64 / SAMPLE_RATE as f64;
    let n = (mono.len() as f64 / ratio) as usize;
    Ok((0..n)
        .map(|i| {
            let x = i as f64 * ratio;
            let j = x as usize;
            let f = (x - j as f64) as f32;
            let a = mono[j];
            let b = *mono.get(j + 1).unwrap_or(&a);
            a + (b - a) * f
        })
        .collect())
}
