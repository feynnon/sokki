//! Echo cancellation of the microphone track with the system track as the
//! far-end reference (WebRTC AEC3, via the pure-Rust `sonora` port).
//!
//! In a laptop call without headphones the microphone also records the
//! remote voices from the speakers. Speaker detection then finds them as an
//! extra voice on the mic ("one speaker too many"). Removing that echo from
//! the audio before diarization fixes it at the source; the user's own speech
//! stays (measured on three real calls: 97–98 % of the user's words
//! transcribed identically before and after).

use sonora::config::{EchoCanceller, HighPassFilter};
use sonora::{AudioProcessing, Config, StreamConfig};

use crate::{Track, TrackKind, SAMPLE_RATE};

/// 10 ms, the frame size AEC3 works in.
const FRAME: usize = SAMPLE_RATE as usize / 100;

/// System audio quieter than this (RMS over the whole track) carries no
/// remote voices, so there is nothing to cancel.
const SILENT_SYSTEM_RMS: f32 = 1e-4;

/// Removes the echo of `system` from `mic` in place. Both are 16 kHz mono
/// and aligned in time (sample `i` is the same moment on both). The mic part
/// after the end of `system` is processed against silence.
pub fn cancel_echo(mic: &mut [f32], system: &[f32]) {
    let config = Config {
        echo_canceller: Some(EchoCanceller::default()),
        high_pass_filter: Some(HighPassFilter::default()),
        ..Default::default()
    };
    let stream = StreamConfig::new(SAMPLE_RATE as u32, 1);
    let mut apm = AudioProcessing::builder()
        .config(config)
        .capture_config(stream)
        .render_config(stream)
        .build();
    let silence = [0.0f32; FRAME];
    let mut far_out = [0.0f32; FRAME];
    let mut near_in = [0.0f32; FRAME];
    let mut i = 0;
    while i + FRAME <= mic.len() {
        let far = system.get(i..i + FRAME).unwrap_or(&silence);
        near_in.copy_from_slice(&mic[i..i + FRAME]);
        // Errors only come from unsupported formats, which these are not;
        // on one anyway the frame stays as recorded.
        let _ = apm.process_render_f32(&[far], &mut [&mut far_out]);
        if apm
            .process_capture_f32(&[&near_in], &mut [&mut mic[i..i + FRAME]])
            .is_err()
        {
            mic[i..i + FRAME].copy_from_slice(&near_in);
        }
        i += FRAME;
    }
}

/// Cancels the echo on the mic track when there is a system track with
/// sound. Returns whether it ran.
pub fn cancel_echo_in_tracks(tracks: &mut [Track]) -> bool {
    let Some(si) = tracks.iter().position(|t| t.kind == TrackKind::System) else {
        return false;
    };
    let Some(mi) = tracks.iter().position(|t| t.kind == TrackKind::Mic) else {
        return false;
    };
    let sys = &tracks[si].samples;
    if sys.is_empty() {
        return false;
    }
    let rms =
        (sys.iter().map(|x| (*x as f64) * (*x as f64)).sum::<f64>() / sys.len() as f64).sqrt();
    if (rms as f32) < SILENT_SYSTEM_RMS {
        return false;
    }
    // Two disjoint borrows of the slice.
    let (mic, system) = if mi < si {
        let (a, b) = tracks.split_at_mut(si);
        (&mut a[mi].samples, &b[0].samples)
    } else {
        let (a, b) = tracks.split_at_mut(mi);
        (&mut b[0].samples, &a[si].samples)
    };
    cancel_echo(mic, system);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Deterministic noise-like "voice" (a few mixed tones with a slow
    /// amplitude envelope) so the test needs no audio files.
    fn voice(n: usize, seed: f32) -> Vec<f32> {
        (0..n)
            .map(|i| {
                let t = i as f32 / SAMPLE_RATE as f32;
                let env = 0.5 + 0.5 * (t * 2.3 + seed).sin();
                env * 0.2
                    * ((t * (180.0 + seed * 40.0) * std::f32::consts::TAU).sin()
                        + 0.6 * (t * (410.0 + seed * 25.0) * std::f32::consts::TAU).sin()
                        + 0.3 * (t * (1130.0 + seed * 70.0) * std::f32::consts::TAU).sin())
            })
            .collect()
    }

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt()
    }

    #[test]
    fn removes_a_delayed_copy_of_the_far_end() {
        let n = SAMPLE_RATE as usize * 12;
        let far = voice(n, 1.0);
        // Mic: the far end from the speakers, 40 ms late and quieter; no near speech.
        let delay = SAMPLE_RATE as usize / 25;
        let mut mic = vec![0.0f32; n];
        for i in delay..n {
            mic[i] = 0.4 * far[i - delay];
        }
        let before = rms(&mic[n / 2..]);
        cancel_echo(&mut mic, &far);
        let after = rms(&mic[n / 2..]);
        assert!(after < before * 0.3, "echo left: {before} -> {after}");
    }

    #[test]
    fn keeps_near_speech_when_the_far_end_is_silent() {
        let n = SAMPLE_RATE as usize * 8;
        let near = voice(n, 3.0);
        let mut mic = near.clone();
        cancel_echo(&mut mic, &vec![0.0; n]);
        let (a, b) = (rms(&near[n / 2..]), rms(&mic[n / 2..]));
        assert!(b > a * 0.7, "near speech lost: {a} -> {b}");
    }

    #[test]
    fn skips_without_system_sound() {
        let n = SAMPLE_RATE as usize;
        let mut tracks = vec![
            Track {
                kind: TrackKind::Mic,
                samples: voice(n, 2.0),
            },
            Track {
                kind: TrackKind::System,
                samples: vec![0.0; n],
            },
        ];
        let before = tracks[0].samples.clone();
        assert!(!cancel_echo_in_tracks(&mut tracks));
        assert_eq!(tracks[0].samples, before);
        let mut only_mic = vec![Track {
            kind: TrackKind::Mic,
            samples: voice(n, 2.0),
        }];
        assert!(!cancel_echo_in_tracks(&mut only_mic));
    }
}
