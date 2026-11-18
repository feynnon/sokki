//! Acoustic echo detection between the microphone and system-audio tracks.
//!
//! During a laptop call without headphones the microphone also picks up the
//! remote voices from the speakers: a quieter, delayed, filtered copy of the
//! system track. Speech recognition often turns that faint echo into
//! unrelated words (or another language), so text-based dedup can miss it.
//! This check works on the audio itself, before transcription: a mic stretch
//! whose loudness contour follows the system track's (with a small delay) is
//! an echo.

use crate::SAMPLE_RATE;

/// Frame energy below this (dBFS) is silence.
const SILENT_DB: f32 = -55.0;

#[derive(Debug, Clone, PartialEq)]
pub struct EchoConfig {
    /// Analysis frame, seconds.
    pub frame_s: f64,
    /// Largest delay of the echo behind the system audio, seconds.
    pub max_lag_s: f64,
    /// Window over which correlation is judged, seconds.
    pub window_s: f64,
    /// Correlation of log-energy contours above which a window is echo.
    pub min_correlation: f32,
    /// System audio quieter than this (RMS) means "remote side silent".
    pub system_silence_rms: f32,
    /// A mic turn is dropped when at least this share of its windows is echo.
    pub drop_share: f32,
}

impl Default for EchoConfig {
    fn default() -> Self {
        Self {
            frame_s: 0.02,
            max_lag_s: 0.3,
            window_s: 1.0,
            min_correlation: 0.7,
            system_silence_rms: 0.003,
            drop_share: 0.7,
        }
    }
}

/// Share (0..=1) of the mic stretch `[start_s, end_s)` that is echo of the
/// system track. Windows where the system side is silent never count as echo.
pub fn echo_share(mic: &[f32], system: &[f32], start_s: f64, end_s: f64, cfg: &EchoConfig) -> f32 {
    let frame = (cfg.frame_s * SAMPLE_RATE as f64).round().max(1.0) as usize;
    let max_lag = (cfg.max_lag_s / cfg.frame_s).round() as usize;
    let win = ((cfg.window_s / cfg.frame_s).round() as usize).max(4);
    let f0 = (start_s.max(0.0) * SAMPLE_RATE as f64) as usize / frame;
    let f1 = ((end_s * SAMPLE_RATE as f64) as usize / frame).min(mic.len() / frame);
    if f1 <= f0 + 4 {
        return 0.0;
    }
    let mic_e = log_energy(mic, frame, f0, f1);
    // System frames from `max_lag` before the stretch, so lags look back in time.
    let s0 = f0.saturating_sub(max_lag);
    let sys_e = log_energy(system, frame, s0, f1.min(system.len() / frame));
    let sys_rms = rms_frames(system, frame, s0, f1.min(system.len() / frame));

    let mut windows = 0usize;
    let mut echo = 0usize;
    let mut w = 0usize;
    while w < mic_e.len() {
        let end = (w + win).min(mic_e.len());
        if end - w < win / 2 {
            break;
        }
        let m = &mic_e[w..end];
        // Windows where the mic is silent say nothing either way.
        if m.iter().cloned().fold(f32::MIN, f32::max) < SILENT_DB {
            w = end;
            continue;
        }
        windows += 1;
        // Remote side must actually be talking somewhere in this window.
        let base = f0 - s0 + w;
        let loud = (0..=max_lag).any(|lag| {
            let a = base.saturating_sub(lag);
            sys_rms
                .get(a..(a + m.len()).min(sys_rms.len()))
                .is_some_and(|s| {
                    s.iter().filter(|&&r| r > cfg.system_silence_rms).count() * 5 > s.len()
                })
        });
        // A flat loudness contour (steady noise, hum) carries no evidence.
        // Echoes can sit only a few dB above the noise floor, hence 1 dB.
        let varied = std_dev(m) > 1.0;
        if loud && varied {
            let best = (0..=max_lag)
                .filter_map(|lag| {
                    // At the very start there is no system history to look
                    // back into: compare the part of the window that has one.
                    let skip = lag.saturating_sub(base);
                    let a = base + skip - lag;
                    let len = m.len().checked_sub(skip)?;
                    let s = sys_e.get(a..a + len)?;
                    Some(pearson(&m[skip..], s))
                })
                .fold(f32::MIN, f32::max);
            if best >= cfg.min_correlation {
                echo += 1;
            }
        }
        w = end;
    }
    if windows == 0 {
        0.0
    } else {
        echo as f32 / windows as f32
    }
}

fn log_energy(x: &[f32], frame: usize, f0: usize, f1: usize) -> Vec<f32> {
    (f0..f1.max(f0))
        .map(|f| {
            let s = &x[(f * frame).min(x.len())..((f + 1) * frame).min(x.len())];
            let e = s.iter().map(|v| v * v).sum::<f32>() / s.len().max(1) as f32;
            10.0 * (e + 1e-7).log10()
        })
        .collect()
}

fn rms_frames(x: &[f32], frame: usize, f0: usize, f1: usize) -> Vec<f32> {
    (f0..f1.max(f0))
        .map(|f| {
            let s = &x[(f * frame).min(x.len())..((f + 1) * frame).min(x.len())];
            (s.iter().map(|v| v * v).sum::<f32>() / s.len().max(1) as f32).sqrt()
        })
        .collect()
}

fn std_dev(x: &[f32]) -> f32 {
    let n = x.len().max(1) as f32;
    let m = x.iter().sum::<f32>() / n;
    (x.iter().map(|v| (v - m) * (v - m)).sum::<f32>() / n).sqrt()
}

fn pearson(a: &[f32], b: &[f32]) -> f32 {
    let n = a.len().min(b.len());
    if n < 4 {
        return 0.0;
    }
    let (a, b) = (&a[..n], &b[..n]);
    let ma = a.iter().sum::<f32>() / n as f32;
    let mb = b.iter().sum::<f32>() / n as f32;
    let (mut num, mut da, mut db) = (0.0f32, 0.0f32, 0.0f32);
    for i in 0..n {
        let (x, y) = (a[i] - ma, b[i] - mb);
        num += x * y;
        da += x * x;
        db += y * y;
    }
    if da <= 1e-6 || db <= 1e-6 {
        0.0
    } else {
        num / (da.sqrt() * db.sqrt())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Speech-like noise: random bursts of different loudness.
    fn bursts(seed: u64, secs: f64) -> Vec<f32> {
        let n = (secs * SAMPLE_RATE as f64) as usize;
        let mut state = seed;
        let mut rnd = || {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((state >> 32) as u32 as f32 / u32::MAX as f32) * 2.0 - 1.0
        };
        let mut out = vec![0.0; n];
        let syl = SAMPLE_RATE as usize / 6;
        let mut i = 0;
        while i < n {
            let amp = if rnd() > -0.2 {
                0.2 + 0.3 * rnd().abs()
            } else {
                0.0
            };
            for j in i..(i + syl).min(n) {
                out[j] = amp * rnd();
            }
            i += syl;
        }
        out
    }

    fn delayed_quiet(x: &[f32], delay_s: f64, gain: f32) -> Vec<f32> {
        let d = (delay_s * SAMPLE_RATE as f64) as usize;
        let mut out = vec![0.0; x.len()];
        for i in d..x.len() {
            out[i] = gain * x[i - d];
        }
        out
    }

    #[test]
    fn pure_echo_is_detected() {
        let system = bursts(1, 6.0);
        let mic = delayed_quiet(&system, 0.04, 0.12);
        let share = echo_share(&mic, &system, 0.0, 6.0, &EchoConfig::default());
        assert!(share > 0.8, "share {share}");
    }

    #[test]
    fn own_speech_while_remote_silent_is_not_echo() {
        let system = vec![0.0; 6 * SAMPLE_RATE as usize];
        let mic = bursts(2, 6.0);
        assert_eq!(
            echo_share(&mic, &system, 0.0, 6.0, &EchoConfig::default()),
            0.0
        );
    }

    #[test]
    fn independent_speech_over_remote_speech_is_not_echo() {
        let system = bursts(3, 6.0);
        let mic = bursts(4, 6.0);
        assert!(echo_share(&mic, &system, 0.0, 6.0, &EchoConfig::default()) < 0.3);
    }
}
