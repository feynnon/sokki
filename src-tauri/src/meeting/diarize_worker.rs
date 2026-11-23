//! Speaker detection in a short-lived child process, on the GPU.
//!
//! transcribe.cpp registers its compute backends once per process. The app
//! process must stay CPU-only for dictation's Silero VAD (registering the GPU
//! backends there broke dictation with "init_backends_with: status 8"), so
//! Sortformer used to run on the CPU in the app: ~60 s for a 22-minute call.
//! Like the transcription worker, this runs it in its own process instead:
//! the app starts itself with [`FLAG`], passes the audio as a raw f32 file,
//! and reads the speaker turns back as JSON. The child exits when done, so
//! its GPU memory is freed at once.
//!
//! The parent watches the child: it is killed after [`TIMEOUT`] or above
//! [`MAX_BYTES`] of memory. On any failure the caller falls back to the
//! in-process CPU diarizer.

use anyhow::{bail, Context, Result};
use meeting_core::SpeakerTurn;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

pub const FLAG: &str = "--diarize-worker";
/// Starts the worker's own error line on stderr; everything before it is
/// ggml's start-up logging.
const ERROR_PREFIX: &str = "diarize worker: ";

/// What the worker hands back.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct WorkerOutput {
    pub turns: Vec<SpeakerTurn>,
    /// Set when the voice-embedding model could not be loaded: speakers
    /// were told apart, but known voices can't be recognised.
    #[serde(default)]
    pub embedder_error: Option<String>,
}
const TIMEOUT: Duration = Duration::from_secs(15 * 60);
const MAX_BYTES: u64 = 6 * 1024 * 1024 * 1024;

/// Whether this process was started as the diarization worker.
pub fn is_invocation() -> bool {
    std::env::args_os().nth(1).is_some_and(|a| a == FLAG)
}

/// Worker entry: `--diarize-worker <models_dir> <in.f32> <out.json>`.
/// Returns the process exit code.
pub fn run() -> i32 {
    let args: Vec<String> = std::env::args().skip(2).collect();
    match run_inner(&args) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("{ERROR_PREFIX}{e:#}");
            1
        }
    }
}

fn run_inner(args: &[String]) -> Result<()> {
    let [models, input, output] = args else {
        bail!("usage: {FLAG} <models_dir> <in.f32> <out.json>");
    };
    let bytes = std::fs::read(input).with_context(|| format!("read {input}"))?;
    let samples: Vec<f32> = bytes
        .chunks_exact(4)
        .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .collect();
    // GPU (Metal) plus CPU: this process does nothing else.
    transcribe_cpp::init_backends_default().map_err(|e| anyhow::anyhow!("backends: {e}"))?;
    let diarizer =
        diarize::SortformerDiarizer::new(diarize::DiarizeConfig::new(PathBuf::from(models)))?;
    let turns = meeting_core::Diarizer::diarize(&diarizer, &samples)?;
    let out = WorkerOutput {
        turns,
        embedder_error: diarizer.embedder_error().map(str::to_string),
    };
    drop(diarizer);
    let tmp = format!("{output}.tmp");
    std::fs::write(&tmp, serde_json::to_vec(&out)?)?;
    std::fs::rename(&tmp, output)?;
    Ok(())
}

/// The worker's own error from its stderr, without ggml's start-up logging
/// (which comes first and can run to dozens of lines).
fn worker_error(stderr: &str) -> &str {
    stderr
        .lines()
        .rev()
        .find_map(|l| l.strip_prefix(ERROR_PREFIX))
        .unwrap_or("no error message")
        .trim()
}

/// Runs the worker on `samples` and returns what it found. `scratch` is a
/// directory for the two exchange files (removed afterwards).
pub fn diarize_in_child(
    samples: &[f32],
    models_dir: &Path,
    scratch: &Path,
) -> Result<WorkerOutput> {
    std::fs::create_dir_all(scratch)?;
    let stamp = format!(
        "{}-{}",
        std::process::id(),
        Instant::now().elapsed().as_nanos()
    );
    let input = scratch.join(format!("diarize-{stamp}.f32"));
    let output = scratch.join(format!("diarize-{stamp}.json"));
    let mut buf = Vec::with_capacity(samples.len() * 4);
    for s in samples {
        buf.extend_from_slice(&s.to_le_bytes());
    }
    std::fs::write(&input, &buf)?;
    drop(buf);
    let result = (|| -> Result<WorkerOutput> {
        let exe = std::env::current_exe()?;
        let mut child = Command::new(exe)
            .arg(FLAG)
            .arg(models_dir)
            .arg(&input)
            .arg(&output)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .context("start the diarization worker")?;
        let started = Instant::now();
        let mut peak = 0u64;
        // In the background band while a meeting records (recording wins).
        let mut band = super::priority::Class::Normal;
        let status = loop {
            if let Some(status) = child.try_wait()? {
                break status;
            }
            super::priority::follow_recording_child(child.id(), &mut band);
            if let Some(b) = super::memguard::footprint_of(child.id() as i32) {
                peak = peak.max(b);
                if b > MAX_BYTES {
                    let _ = child.kill();
                    let _ = child.wait();
                    bail!("diarization worker used {:.1} GB; stopped", b as f64 / 1e9);
                }
            }
            if started.elapsed() > TIMEOUT {
                let _ = child.kill();
                let _ = child.wait();
                bail!(
                    "diarization worker took longer than {} min; stopped",
                    TIMEOUT.as_secs() / 60
                );
            }
            std::thread::sleep(Duration::from_millis(250));
        };
        if !status.success() {
            let mut err = String::new();
            if let Some(mut e) = child.stderr.take() {
                use std::io::Read;
                let _ = e.read_to_string(&mut err);
            }
            log::warn!("Diarization worker output:\n{}", err.trim());
            bail!(
                "diarization worker failed ({status}): {}",
                worker_error(&err)
            );
        }
        log::info!(
            "Diarized {:.0}s of audio on the GPU in {:.1}s (worker peak {:.0} MB)",
            samples.len() as f64 / 16_000.0,
            started.elapsed().as_secs_f64(),
            peak as f64 / 1e6
        );
        Ok(serde_json::from_slice(&std::fs::read(&output)?)?)
    })();
    let _ = std::fs::remove_file(&input);
    let _ = std::fs::remove_file(&output);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_error_line_is_found_under_ggml_logging() {
        let stderr = "ggml_metal_device_init: tensor API disabled for pre-M5 and pre-A19 devices
ggml_metal_library_init: using embedded metal library
ggml_metal_library_compile_all: compiled 'fa_aux' library in 0.002 sec
diarize worker: no space left on device
";
        assert_eq!(worker_error(stderr), "no space left on device");
        assert_eq!(
            worker_error("ggml_metal_device_init: x\n"),
            "no error message"
        );
    }
}
