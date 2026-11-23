//! Transcribing a meeting while it is recorded, the macOS side of
//! [`sokki_engine::live`]: both FLAC writers hand their samples to one
//! [`LiveSession`] as they write them, resampled to 16 kHz exactly as
//! decoding the files will do it, and the pieces go to processing at the
//! stop. The transcriber is Handy's engine, which yields to dictation.

use meeting_core::live::CutConfig;
use meeting_core::TrackKind;
use sokki_engine::live::{LiveFeed, LiveOptions, LiveSession};
use sokki_engine::tracks::TrackTiming;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tauri::AppHandle;

use super::flac::Tee;
use super::process::EngineTranscriber;
use crate::audio_toolkit::audio::LiveResampler;

/// A meeting's live transcription, from the start of the recording to the
/// start of its processing.
pub struct Live {
    session: LiveSession,
    /// Processing goes on with the same transcriber, so the lines it takes
    /// from the pieces are cleaned up the way the pieces were heard.
    transcriber: Arc<EngineTranscriber>,
}

impl Live {
    /// Starts the session; `with_system`: system audio is recorded too.
    /// `dir`: the meeting's folder (scratch space for speaker detection).
    pub fn start(app: &AppHandle, with_system: bool, dir: &std::path::Path) -> Option<Self> {
        let transcriber = Arc::new(EngineTranscriber::new(app));
        // Speakers too, window by window, in the same worker process as
        // after the call. Off when speaker separation is off.
        let settings = crate::settings::get_settings(app).meetings;
        let diarizer = settings
            .diarization_enabled
            .then(|| super::process::build_diarizer(app, &settings, dir, Default::default()).ok())
            .flatten();
        let options = LiveOptions {
            with_system,
            timing: Box::new(|| TrackTiming {
                mic_first_us: super::MIC_FIRST_US.load(Ordering::Relaxed),
                system_first_us: super::SYSTEM_FIRST_US.load(Ordering::Relaxed),
            }),
            on_start: Box::new(lower_priority),
            now_us: Box::new(|| {
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_micros() as u64)
                    .unwrap_or(0)
            }),
            cut: CutConfig::default(),
            diarizer,
            stitch: Default::default(),
        };
        match LiveSession::start(transcriber.clone(), options) {
            Ok(session) => Some(Self {
                session,
                transcriber,
            }),
            Err(e) => {
                log::warn!("Live transcript not started: {e}");
                None
            }
        }
    }

    /// The tee for one track's FLAC writer.
    pub fn tee(&self, track: TrackKind) -> Tee {
        tee(self.session.feed(), track)
    }

    /// Transcribes the rest; returns the pieces and the transcriber to go on
    /// with.
    pub(super) fn finish(self) -> (Option<sokki_engine::live::Live>, Arc<EngineTranscriber>) {
        (self.session.finish(), self.transcriber)
    }
}

/// Resamples a writer's samples to 16 kHz (the decoder's resampler) and
/// pushes them to the session. A resampler error ends this track's feed;
/// the turns it would have covered are transcribed after the call.
fn tee(feed: LiveFeed, track: TrackKind) -> Tee {
    let mut resampler: Option<(u32, LiveResampler)> = None;
    let mut broken = false;
    Box::new(move |samples: &[f32], rate: u32| {
        if broken {
            return;
        }
        if rate == meeting_core::SAMPLE_RATE {
            feed.push(track, samples);
            return;
        }
        if resampler.as_ref().is_none_or(|(r, _)| *r != rate) {
            match LiveResampler::new(rate) {
                Ok(r) => resampler = Some((rate, r)),
                Err(e) => {
                    log::warn!("Live transcript: can't resample {rate} Hz: {e:#}");
                    broken = true;
                    return;
                }
            }
        }
        let (_, r) = resampler.as_mut().expect("set above");
        match r.push(samples) {
            Ok(out) => feed.push(track, &out),
            Err(e) => {
                log::warn!("Live transcript: resampling failed: {e:#}");
                broken = true;
            }
        }
    })
}

/// The live worker runs below the call: macOS's "utility" class, for work
/// the person doesn't wait on.
fn lower_priority() {
    #[cfg(target_os = "macos")]
    // SAFETY: changes only the calling thread's own scheduling class.
    unsafe {
        libc::pthread_set_qos_class_self_np(libc::qos_class_t::QOS_CLASS_UTILITY, 0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio_toolkit::decode_audio_file;
    use std::sync::Mutex;

    /// What decoding the file gives is what the tee gave, sample for sample,
    /// at 16 kHz and at a device rate that is resampled.
    #[test]
    fn the_tee_hears_what_decoding_the_file_gives() {
        for rate in [16_000u32, 48_000] {
            let dir = std::env::temp_dir().join(format!("sokki-tee-{}-{rate}", std::process::id()));
            std::fs::create_dir_all(&dir).unwrap();
            let path = dir.join("mic.flac");
            let teed = Arc::new(Mutex::new(Vec::<f32>::new()));
            let sink = teed.clone();
            let mut writer = super::super::flac::FlacWriter::create(&path, rate).unwrap();
            let mut resampler: Option<LiveResampler> = None;
            writer.set_tee(Box::new(move |s: &[f32], r: u32| {
                let out = if r == 16_000 {
                    s.to_vec()
                } else {
                    resampler
                        .get_or_insert_with(|| LiveResampler::new(r).unwrap())
                        .push(s)
                        .unwrap()
                };
                sink.lock().unwrap().extend(out);
            }));
            let n = rate as usize * 3;
            let audio: Vec<f32> = (0..n)
                .map(|i| {
                    0.4 * ((i as f32) * 2.0 * std::f32::consts::PI * 300.0 / rate as f32).sin()
                })
                .collect();
            // Stereo, in uneven blocks, as a microphone delivers it.
            let stereo: Vec<f32> = audio.iter().flat_map(|&x| [x, x * 0.5]).collect();
            for block in stereo.chunks(2 * 997) {
                writer.write_interleaved(block, 2).unwrap();
            }
            writer.finish().unwrap();
            let decoded = decode_audio_file(&path).unwrap();
            let teed = teed.lock().unwrap();
            assert!(teed.len() <= decoded.len());
            // Held back: at most one resampler step plus its delay (~20 ms).
            assert!(
                decoded.len() - teed.len() < 800,
                "{} vs {}",
                teed.len(),
                decoded.len()
            );
            assert!(
                teed[..] == decoded[..teed.len()],
                "at {rate} Hz the tee differs from the file"
            );
            let _ = std::fs::remove_dir_all(&dir);
        }
    }
}
