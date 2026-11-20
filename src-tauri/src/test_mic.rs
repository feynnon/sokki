//! Test mode, so Claude can test dictation end to end without a person
//! speaking: with `SOKKI_TEST_MIC=<WAV>` set, the microphone plays that file
//! in real time from the start of each recording (then silence), the text
//! is logged instead of pasted, and there are no shortcuts and no permission
//! resets. `SOKKI_TEST_LLM=1` also starts the built-in language model once
//! and logs its answer. Run it only in the separate test app (`scripts/test-dictation.sh`),
//! never in Robin's Sokki.

use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use handy_recorder::testing::FakeMic;
use handy_recorder::{Error, Recorder, RecorderConfig, Sink};
use log::{info, warn};

const RATE: u32 = 16_000;

fn path() -> Option<PathBuf> {
    std::env::var_os("SOKKI_TEST_MIC")
        .filter(|p| !p.is_empty())
        .map(PathBuf::from)
}

/// Whether this app runs in test mode.
pub fn active() -> bool {
    static ACTIVE: OnceLock<bool> = OnceLock::new();
    *ACTIVE.get_or_init(|| path().is_some())
}

/// The file's audio, 16 kHz mono (other rates are resampled crudely by
/// nearest sample; tests should pass 16 kHz).
fn load(path: &PathBuf) -> Vec<f32> {
    let Ok(mut reader) = hound::WavReader::open(path) else {
        warn!("Test mode: can't read {}", path.display());
        return Vec::new();
    };
    let spec = reader.spec();
    let channels = spec.channels.max(1) as usize;
    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader.samples::<f32>().filter_map(Result::ok).collect(),
        hound::SampleFormat::Int => {
            let scale = (1u64 << (spec.bits_per_sample - 1)) as f32;
            reader
                .samples::<i32>()
                .filter_map(Result::ok)
                .map(|s| s as f32 / scale)
                .collect()
        }
    };
    let mono: Vec<f32> = samples
        .chunks(channels)
        .map(|frame| frame.iter().sum::<f32>() / channels as f32)
        .collect();
    if spec.sample_rate == RATE {
        return mono;
    }
    let ratio = spec.sample_rate as f64 / RATE as f64;
    (0..(mono.len() as f64 / ratio) as usize)
        .map(|i| mono[((i as f64 * ratio) as usize).min(mono.len() - 1)])
        .collect()
}

/// The fake microphone, and a thread that plays the file into each new
/// recording in real time.
fn mic(block: usize) -> &'static FakeMic {
    static MIC: OnceLock<FakeMic> = OnceLock::new();
    MIC.get_or_init(|| {
        let mic = FakeMic::new(RATE, 1);
        let path = path().unwrap_or_default();
        let audio = load(&path);
        info!(
            "Test mode: the microphone plays {} ({:.1}s), nothing is pasted",
            path.display(),
            audio.len() as f64 / RATE as f64
        );
        let player = mic.clone();
        std::thread::Builder::new()
            .name("test-mic".into())
            .spawn(move || play(&player, &audio, block.max(1)))
            .ok();
        mic
    })
}

fn play(mic: &FakeMic, audio: &[f32], block: usize) {
    let silence = vec![0.0f32; block];
    loop {
        // Wait for the next recording's stream; it plays until that ends.
        while !mic.is_streaming() {
            std::thread::sleep(Duration::from_millis(2));
        }
        info!("Test mode: playing the file into the recording");
        let started = Instant::now();
        let mut sent = 0usize;
        loop {
            let piece = match audio.get(sent..(sent + block).min(audio.len())) {
                Some(p) if !p.is_empty() => p,
                _ => &silence,
            };
            if !mic.push(piece) {
                break;
            }
            sent += piece.len();
            let due = started + Duration::from_secs_f64(sent as f64 / RATE as f64);
            if let Some(wait) = due.checked_duration_since(Instant::now()) {
                std::thread::sleep(wait);
            }
        }
    }
}

/// Opens the fake microphone in place of the real one.
pub fn open<S: Sink>(
    config: RecorderConfig,
    handler: Box<dyn FnOnce(Error) + Send + 'static>,
) -> Result<Recorder<S>, Error> {
    let block = config.frames_per_chunk.unwrap_or(512);
    mic(block).open(config, Some(handler), Duration::from_secs(2))
}

/// Starts one dictation a few seconds after launch and stops it 1 s after
/// the file ends (or after `SOKKI_TEST_HOLD_S`), like the shortcut would, so
/// nothing outside the app has to press anything.
pub fn drive(app: &tauri::AppHandle) {
    if !active() {
        return;
    }
    let length = path()
        .and_then(|p| hound::WavReader::open(p).ok())
        .map(|r| r.duration() as f64 / r.spec().sample_rate.max(1) as f64)
        .unwrap_or(0.0);
    let hold = std::env::var("SOKKI_TEST_HOLD_S")
        .ok()
        .and_then(|s| s.parse::<f64>().ok())
        .unwrap_or(length + 1.0);
    if std::env::var_os("SOKKI_TEST_LLM").is_some() {
        std::thread::spawn(test_builtin_llm);
    }
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(3));
        info!("Test mode: dictation starts ({hold:.1}s)");
        crate::signal_handle::send_transcription_input(&app, "transcribe", "test");
        std::thread::sleep(Duration::from_secs_f64(hold));
        crate::signal_handle::send_transcription_input(&app, "transcribe", "test");
    });
}

/// With `SOKKI_TEST_LLM` set: starts the built-in language model the way a
/// summary does (the server bundled with this app, the model in its data
/// folder), asks it one short question, stops it, and logs each step.
fn test_builtin_llm() {
    let settings = sokki_engine::llm::LlmSettings {
        provider: sokki_engine::llm::BUILTIN.into(),
        ..Default::default()
    };
    let t0 = Instant::now();
    let model = match sokki_engine::llm::open(&settings, "Ich") {
        Ok(m) => m,
        Err(e) => {
            warn!("Test mode: built-in language model did not start: {e:#}");
            return;
        }
    };
    let llm = meeting_core::summary::LlmSummarizer::new(model.config());
    match llm.chat_plain("Answer with the single word OK.", "Ping") {
        Ok(reply) => info!(
            "Test mode: built-in language model answered {reply:?} after {:.1}s",
            t0.elapsed().as_secs_f64()
        ),
        Err(e) => warn!("Test mode: built-in language model failed: {e:#}"),
    }
    drop(model);
    info!("Test mode: built-in language model done");
}
