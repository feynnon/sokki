//! Speaker embeddings with sherpa-onnx's extractor.

use std::collections::HashMap;

use anyhow::{anyhow, Result};
use meeting_core::SAMPLE_RATE;
use sherpa_onnx::{SpeakerEmbeddingExtractor, SpeakerEmbeddingExtractorConfig};

use crate::models::{self, EmbeddingModel};
use crate::postprocess::RawSegment;
use crate::vector::l2_normalize;
use crate::DiarizeConfig;

/// Audio per speaker used for its embedding; longer turns first.
const MAX_AUDIO_PER_SPEAKER_S: f64 = 60.0;
/// Longest single piece fed to the extractor.
const MAX_PIECE_S: f64 = 15.0;
/// Turns shorter than this are skipped when the speaker has longer ones.
const MIN_PIECE_S: f64 = 1.0;
/// Trimmed off each end of a turn (boundaries often bleed into neighbours).
const EDGE_TRIM_S: f64 = 0.1;

/// Computes L2-normalised speaker embeddings from 16 kHz mono audio.
pub struct SpeakerEmbedder {
    extractor: SpeakerEmbeddingExtractor,
    model: EmbeddingModel,
}

impl SpeakerEmbedder {
    /// Loads `config.embedding_model` (downloading it when allowed).
    pub fn new(config: &DiarizeConfig) -> Result<Self> {
        let extractor = models::ensure_and_load(
            &config.models_dir,
            &config.embedding_model.file(),
            config.allow_download,
            |path| {
                SpeakerEmbeddingExtractor::create(&SpeakerEmbeddingExtractorConfig {
                    model: Some(path.to_string_lossy().into_owned()),
                    num_threads: config.num_threads as i32,
                    debug: false,
                    provider: Some("cpu".into()),
                })
                .ok_or_else(|| anyhow!("failed to load speaker embedding model {}", path.display()))
            },
        )?;
        Ok(Self {
            extractor,
            model: config.embedding_model,
        })
    }

    pub fn model(&self) -> EmbeddingModel {
        self.model
    }

    pub fn dim(&self) -> usize {
        self.extractor.dim() as usize
    }

    /// Embedding of one stretch of audio (ideally one speaker, ≥ 1 s).
    pub fn embed(&self, samples: &[f32]) -> Result<Vec<f32>> {
        let stream = self
            .extractor
            .create_stream()
            .ok_or_else(|| anyhow!("failed to create embedding stream"))?;
        stream.accept_waveform(SAMPLE_RATE as i32, samples);
        stream.input_finished();
        if !self.extractor.is_ready(&stream) {
            return Err(anyhow!(
                "audio too short for a speaker embedding ({:.2} s)",
                samples.len() as f64 / SAMPLE_RATE as f64
            ));
        }
        let e = self
            .extractor
            .compute(&stream)
            .ok_or_else(|| anyhow!("speaker embedding failed"))?;
        Ok(l2_normalize(&e))
    }

    /// Embedding of all audio in `spans` (seconds), as a duration-weighted
    /// mean of per-span embeddings, longest spans first, up to about a minute.
    pub fn embed_spans(&self, samples: &[f32], spans: &[(f64, f64)]) -> Result<Vec<f32>> {
        let mut spans: Vec<(f64, f64)> = spans.iter().copied().filter(|(a, b)| b > a).collect();
        spans.sort_by(|x, y| (y.1 - y.0).total_cmp(&(x.1 - x.0)));
        if spans.is_empty() {
            return Err(anyhow!("no audio to embed"));
        }
        let has_long = spans[0].1 - spans[0].0 >= MIN_PIECE_S;
        let mut sum: Vec<f32> = Vec::new();
        let mut used = 0.0;
        let mut last_err = None;
        for (a, b) in spans {
            if used >= MAX_AUDIO_PER_SPEAKER_S {
                break;
            }
            let dur = b - a;
            if has_long && dur < MIN_PIECE_S {
                break; // sorted: the rest are shorter
            }
            let (mut a, mut b) = (a, b);
            if dur > 2.0 * EDGE_TRIM_S + MIN_PIECE_S {
                a += EDGE_TRIM_S;
                b -= EDGE_TRIM_S;
            }
            if b - a > MAX_PIECE_S {
                let mid = (a + b) / 2.0;
                a = mid - MAX_PIECE_S / 2.0;
                b = mid + MAX_PIECE_S / 2.0;
            }
            let i0 = ((a * SAMPLE_RATE as f64) as usize).min(samples.len());
            let i1 = ((b * SAMPLE_RATE as f64) as usize).min(samples.len());
            match self.embed(&samples[i0..i1]) {
                Ok(e) => {
                    let w = (b - a) as f32;
                    if sum.is_empty() {
                        sum = vec![0.0; e.len()];
                    }
                    for (s, x) in sum.iter_mut().zip(&e) {
                        *s += w * x;
                    }
                    used += b - a;
                }
                Err(e) => last_err = Some(e),
            }
        }
        if sum.is_empty() {
            return Err(last_err.unwrap_or_else(|| anyhow!("no audio to embed")));
        }
        Ok(l2_normalize(&sum))
    }

    /// One embedding per cluster in `segs`. Clusters whose audio is too short
    /// to embed are left out.
    pub(crate) fn cluster_embeddings(
        &self,
        samples: &[f32],
        segs: &[RawSegment],
    ) -> HashMap<i32, Vec<f32>> {
        let mut by_cluster: HashMap<i32, Vec<(f64, f64)>> = HashMap::new();
        for s in segs {
            by_cluster
                .entry(s.cluster)
                .or_default()
                .push((s.start_s, s.end_s));
        }
        by_cluster
            .into_iter()
            .filter_map(|(c, spans)| self.embed_spans(samples, &spans).ok().map(|e| (c, e)))
            .collect()
    }
}
