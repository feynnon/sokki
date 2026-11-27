//! Streaming mono FLAC writer for meeting recordings.
//!
//! Same contract as the WAV writer it replaces: samples are written as they
//! arrive, audio goes to `<name>.partial`, and the file is renamed to `<name>`
//! on `finish`, so a file without the `.partial` suffix is always complete.
//!
//! FLAC is lossless and compresses digital silence to almost nothing, which is
//! what makes long meetings with quiet stretches small on disk. The frames are
//! written one by one, so memory stays flat. The STREAMINFO header holds the
//! final sample count and MD5, so it is rewritten at the start of the file in
//! `finish`; its length never changes, so the rewrite is in place.

use anyhow::{anyhow, bail, Context, Result};
use flacenc::bitsink::ByteSink;
use flacenc::component::{BitRepr, Stream};
use flacenc::config;
use flacenc::error::{Verified, Verify};
use flacenc::source::{Context as EncodeContext, Fill, FrameBuf};
use std::fs::{self, File};
use std::io::{BufWriter, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

/// Samples per FLAC frame. Only the last frame may be shorter.
const BLOCK_SIZE: usize = 4096;
/// Shortest frame the format allows; a shorter last frame is padded to this.
const MIN_BLOCK_SIZE: usize = 16;
const BITS_PER_SAMPLE: usize = 16;

pub struct FlacWriter {
    out: BufWriter<File>,
    partial_path: PathBuf,
    final_path: PathBuf,
    config: Verified<config::Encoder>,
    /// Holds only the STREAMINFO; frames are written as they are encoded.
    stream: Stream,
    /// Frame buffer and encoder context (frame counter, MD5), fed together.
    dest: (FrameBuf, EncodeContext),
    /// Samples waiting for a full frame, as 16-bit values.
    pending: Vec<i32>,
    header_len: usize,
    /// Smallest and largest frame written, in bytes, for STREAMINFO.
    frame_sizes: (usize, usize),
    frames: u64,
    sample_rate: u32,
    /// Gets every sample as written, as decoding the file will give it back.
    tee: Option<Tee>,
}

/// Receives the samples a [`FlacWriter`] writes (mono, at its rate), e.g.
/// to transcribe them during the recording.
pub type Tee = Box<dyn FnMut(&[f32], u32) + Send>;

impl FlacWriter {
    /// Creates `<final_path>.partial` and writes the FLAC header at `sample_rate`.
    pub fn create(final_path: &Path, sample_rate: u32) -> Result<Self> {
        if sample_rate == 0 {
            bail!("FLAC sample rate must be non-zero");
        }
        let config = config::Encoder::default()
            .into_verified()
            .map_err(|e| anyhow!("FLAC encoder config is invalid: {e:?}"))?;
        let mut stream = Stream::new(sample_rate as usize, 1, BITS_PER_SAMPLE)
            .map_err(|e| anyhow!("FLAC stream setup failed: {e:?}"))?;
        stream
            .stream_info_mut()
            .set_block_sizes(BLOCK_SIZE, BLOCK_SIZE)
            .map_err(|e| anyhow!("FLAC block size setup failed: {e:?}"))?;
        let framebuf = FrameBuf::with_size(1, BLOCK_SIZE)
            .map_err(|e| anyhow!("FLAC frame buffer setup failed: {e:?}"))?;
        let context = EncodeContext::new(BITS_PER_SAMPLE, 1);

        let partial_path = PathBuf::from(format!("{}.partial", final_path.display()));
        let file = File::create(&partial_path)
            .with_context(|| format!("Creating {}", partial_path.display()))?;
        let mut out = BufWriter::new(file);
        let header = header_bytes(&stream)?;
        out.write_all(&header)
            .with_context(|| format!("Writing {}", partial_path.display()))?;

        Ok(Self {
            out,
            partial_path,
            final_path: final_path.to_path_buf(),
            config,
            stream,
            dest: (framebuf, context),
            pending: Vec::with_capacity(BLOCK_SIZE),
            header_len: header.len(),
            frame_sizes: (usize::MAX, 0),
            frames: 0,
            sample_rate,
            tee: None,
        })
    }

    /// Appends interleaved `samples` with `channels` channels, averaging them
    /// down to mono. `samples.len()` must be a multiple of `channels`.
    pub fn write_interleaved(&mut self, samples: &[f32], channels: usize) -> Result<()> {
        if channels == 0 || samples.len() % channels != 0 {
            bail!(
                "{} samples do not split into {channels} channels",
                samples.len()
            );
        }
        let mut teed = Vec::new();
        for frame in samples.chunks_exact(channels) {
            let mono = frame.iter().sum::<f32>() / channels as f32;
            let value = to_i16(mono);
            if self.tee.is_some() {
                teed.push(from_i16(value));
            }
            self.pending.push(value as i32);
            self.frames += 1;
            if self.pending.len() == BLOCK_SIZE {
                self.encode_pending()?;
            }
        }
        if let Some(tee) = &mut self.tee {
            tee(&teed, self.sample_rate);
        }
        Ok(())
    }

    /// Hands every sample written from now on to `tee` as well.
    pub fn set_tee(&mut self, tee: Tee) {
        self.tee = Some(tee);
    }

    /// Encodes the pending samples as one frame. A full block is `BLOCK_SIZE`;
    /// the last one keeps its own length, padded only below `MIN_BLOCK_SIZE`.
    fn encode_pending(&mut self) -> Result<()> {
        if self.pending.is_empty() {
            return Ok(());
        }
        if self.pending.len() < MIN_BLOCK_SIZE {
            self.pending.resize(MIN_BLOCK_SIZE, 0);
        }
        let samples = std::mem::take(&mut self.pending);
        self.dest
            .fill_interleaved(&samples)
            .map_err(|e| anyhow!("FLAC frame fill failed: {e:?}"))?;
        let frame_number = self
            .dest
            .1
            .current_frame_number()
            .ok_or_else(|| anyhow!("FLAC frame counter overflowed"))?;
        let frame = flacenc::encode_fixed_size_frame(
            &self.config,
            &self.dest.0,
            frame_number,
            self.stream.stream_info(),
        )
        .map_err(|e| anyhow!("FLAC frame encoding failed: {e:?}"))?;
        let mut sink = ByteSink::new();
        frame
            .write(&mut sink)
            .map_err(|e| anyhow!("FLAC frame serialization failed: {e:?}"))?;
        self.out
            .write_all(sink.as_slice())
            .with_context(|| format!("Writing {}", self.partial_path.display()))?;
        let size = sink.as_slice().len();
        self.frame_sizes = (self.frame_sizes.0.min(size), self.frame_sizes.1.max(size));
        // Reuse the allocation for the next frame.
        self.pending = samples;
        self.pending.clear();
        Ok(())
    }

    /// Encodes the last frame, writes the final header and renames the file.
    /// Returns the number of samples recorded.
    pub fn finish(mut self) -> Result<u64> {
        if !self.pending.is_empty() {
            self.encode_pending()?;
        }
        let total = self.frames;
        self.stream
            .stream_info_mut()
            .set_md5_digest(&self.dest.1.md5_digest());
        self.stream
            .stream_info_mut()
            .set_total_samples(total as usize);
        // Frames are written here, not added to `stream`, so its frame sizes
        // would stay at their start values (min 0xFFFFFF > max 0). AVFoundation
        // then decodes no audio at all, so a "Meeting with screen" video was
        // silent (2026-11-27). 0 means "unknown" in the FLAC format.
        let (min, max) = match self.frame_sizes {
            (min, max) if min <= max => (min, max),
            _ => (0, 0),
        };
        self.stream
            .stream_info_mut()
            .set_frame_sizes(min, max)
            .map_err(|e| anyhow!("FLAC frame sizes: {e:?}"))?;

        let header = header_bytes(&self.stream)?;
        if header.len() != self.header_len {
            bail!("FLAC header changed size; the file cannot be finalized");
        }
        let mut file = self
            .out
            .into_inner()
            .map_err(|e| anyhow!("Flushing {}: {e}", self.partial_path.display()))?;
        file.seek(SeekFrom::Start(0))
            .with_context(|| format!("Finalizing {}", self.partial_path.display()))?;
        file.write_all(&header)
            .with_context(|| format!("Finalizing {}", self.partial_path.display()))?;
        file.flush()
            .with_context(|| format!("Finalizing {}", self.partial_path.display()))?;
        drop(file);

        fs::rename(&self.partial_path, &self.final_path).with_context(|| {
            format!(
                "Renaming {} to {}",
                self.partial_path.display(),
                self.final_path.display()
            )
        })?;
        Ok(total)
    }
}

/// The FLAC stream marker and STREAMINFO for `stream`, with no frames.
fn header_bytes(stream: &Stream) -> Result<Vec<u8>> {
    let mut sink = ByteSink::new();
    stream
        .write(&mut sink)
        .map_err(|e| anyhow!("FLAC header serialization failed: {e:?}"))?;
    Ok(sink.as_slice().to_vec())
}

/// A stored sample as decoding gives it back (symphonia scales by 2^15).
fn from_i16(value: i16) -> f32 {
    value as f32 / 32768.0
}

fn to_i16(sample: f32) -> i16 {
    let clamped = sample.clamp(-1.0, 1.0);
    (clamped * i16::MAX as f32).round() as i16
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio_toolkit::audio::decode_audio_file;
    use std::path::PathBuf;

    fn temp_path(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mandy-flac-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        dir.join(name)
    }

    #[test]
    fn round_trips_speech_and_silence_through_the_decoder() {
        let final_path = temp_path("roundtrip.flac");
        let _ = fs::remove_file(&final_path);
        let rate = 16_000u32;

        let mut writer = FlacWriter::create(&final_path, rate).unwrap();
        // 1.5 s of a 440 Hz tone, in uneven chunks to exercise block boundaries.
        let tone: Vec<f32> = (0..24_000)
            .map(|i| 0.3 * (2.0 * std::f32::consts::PI * 440.0 * i as f32 / rate as f32).sin())
            .collect();
        for chunk in tone.chunks(1000) {
            writer.write_interleaved(chunk, 1).unwrap();
        }
        // 1 s of digital silence.
        writer.write_interleaved(&vec![0.0; 16_000], 1).unwrap();
        let frames = writer.finish().unwrap();

        assert_eq!(frames, 40_000);
        assert!(final_path.exists());
        assert!(!PathBuf::from(format!("{}.partial", final_path.display())).exists());

        let decoded = decode_audio_file(&final_path).unwrap();
        assert_eq!(decoded.len(), 40_000);
        let tone_rms = (decoded[..24_000].iter().map(|x| x * x).sum::<f32>() / 24_000.0).sqrt();
        assert!(
            (tone_rms - 0.3 / 2f32.sqrt()).abs() < 0.01,
            "tone rms {tone_rms}"
        );
        assert!(decoded[24_000..].iter().all(|x| x.abs() < 1e-4));

        // STREAMINFO's frame sizes are real (bytes 12..18 after "fLaC" and the
        // block header): min ≤ max. AVFoundation reads no audio otherwise.
        let bytes = fs::read(&final_path).unwrap();
        let min = u32::from_be_bytes([0, bytes[12], bytes[13], bytes[14]]);
        let max = u32::from_be_bytes([0, bytes[15], bytes[16], bytes[17]]);
        assert!(min > 0 && min <= max, "frame sizes {min}..{max}");

        let size = fs::metadata(&final_path).unwrap().len();
        // 40k samples of 16-bit PCM would be 80 KB; the silent second adds almost nothing.
        assert!(size < 80_000 / 2, "flac is {size} bytes");
        let _ = fs::remove_file(&final_path);
    }
}
