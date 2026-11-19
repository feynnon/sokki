//! Track alignment and the "no speaker detection" diarizer.

use anyhow::Result;
use meeting_core::{SpeakerTurn, Track, TrackKind, SAMPLE_RATE};
use serde::{Deserialize, Serialize};

/// When each track's first sample was captured (µs since the Unix epoch),
/// saved next to the audio as `timing.json`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrackTiming {
    pub mic_first_us: u64,
    pub system_first_us: u64,
}

pub const TIMING_FILE: &str = "timing.json";
pub const MIC_FILE: &str = "mic.flac";
pub const SYSTEM_FILE: &str = "system.flac";

/// Lines the tracks up in time: the track that started later gets leading
/// silence for the difference, so the same moment has the same timestamp on
/// both. Returns a warning when the offset looks implausible and is ignored.
pub fn align_tracks(
    tracks: &mut [Track],
    mic_first_us: u64,
    system_first_us: u64,
) -> Option<String> {
    if mic_first_us == 0 || system_first_us == 0 || tracks.len() < 2 {
        return None;
    }
    let offset_us = mic_first_us as i64 - system_first_us as i64;
    if offset_us.abs() > 5_000_000 {
        return Some(format!(
            "track start offset {offset_us} µs looks wrong; not aligned"
        ));
    }
    let pad = (offset_us.unsigned_abs() * SAMPLE_RATE as u64 / 1_000_000) as usize;
    // Positive offset: the microphone started later.
    let later = if offset_us > 0 {
        TrackKind::Mic
    } else {
        TrackKind::System
    };
    if let Some(t) = tracks.iter_mut().find(|t| t.kind == later) {
        let mut padded = vec![0.0f32; pad];
        padded.extend_from_slice(&t.samples);
        t.samples = padded;
    }
    log::info!(
        "Aligned tracks: {} started {} ms later",
        if offset_us > 0 { "mic" } else { "system" },
        offset_us.abs() / 1000
    );
    None
}

/// No speaker detection: the whole track is one speaker.
pub struct WholeTrack;

impl meeting_core::Diarizer for WholeTrack {
    fn diarize(&self, samples: &[f32]) -> Result<Vec<SpeakerTurn>> {
        if samples.is_empty() {
            return Ok(Vec::new());
        }
        Ok(vec![SpeakerTurn {
            start_s: 0.0,
            end_s: samples.len() as f64 / SAMPLE_RATE as f64,
            speaker: "S1".into(),
            embedding: None,
        }])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn later_track_gets_leading_silence() {
        let mut tracks = vec![
            Track {
                kind: TrackKind::Mic,
                samples: vec![1.0; 10],
            },
            Track {
                kind: TrackKind::System,
                samples: vec![1.0; 10],
            },
        ];
        // Mic started 250 ms after system audio.
        assert!(align_tracks(&mut tracks, 1_000_250_000, 1_000_000_000).is_none());
        assert_eq!(tracks[0].samples.len(), 10 + 4_000);
        assert_eq!(tracks[0].samples[0], 0.0);
        assert_eq!(tracks[1].samples.len(), 10);
        // Implausible offsets are ignored.
        assert!(align_tracks(&mut tracks, 10_000_000_000, 1_000_000_000).is_some());
    }
}
