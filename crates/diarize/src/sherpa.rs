//! sherpa-onnx backend: pyannote-3.0 segmentation, speaker embeddings,
//! agglomerative clustering (all CPU, ONNX Runtime).

use std::collections::HashMap;
use std::sync::Mutex;

use anyhow::{anyhow, Result};
use meeting_core::{Diarizer, SpeakerTurn, SAMPLE_RATE};
use sherpa_onnx::{
    FastClusteringConfig, OfflineSpeakerDiarization, OfflineSpeakerDiarizationConfig,
    OfflineSpeakerSegmentationModelConfig, OfflineSpeakerSegmentationPyannoteModelConfig,
    SpeakerEmbeddingExtractorConfig,
};

use crate::embedder::SpeakerEmbedder;
use crate::models::{self, PYANNOTE_SEGMENTATION};
use crate::postprocess::{self, RawSegment};
use crate::{to_turns, DiarizeConfig};

/// Shortest input worth running (the segmentation window is 10 s, but
/// sherpa pads shorter input).
const MIN_INPUT_S: f64 = 0.5;

/// Offline diarization with sherpa-onnx. See the crate docs.
pub struct SherpaDiarizer {
    config: DiarizeConfig,
    engine: Mutex<Engine>,
    embedder: Option<SpeakerEmbedder>,
}

struct Engine {
    sd: OfflineSpeakerDiarization,
    sd_config: OfflineSpeakerDiarizationConfig,
}

impl SherpaDiarizer {
    /// Loads the models (downloading them when allowed).
    pub fn new(config: DiarizeConfig) -> Result<Self> {
        let seg = models::ensure(
            &config.models_dir,
            &PYANNOTE_SEGMENTATION,
            config.allow_download,
        )?;
        let emb = models::ensure(
            &config.models_dir,
            &config.clustering_model.file(),
            config.allow_download,
        )?;
        let threads = config.num_threads.max(1) as i32;
        let sd_config = OfflineSpeakerDiarizationConfig {
            segmentation: OfflineSpeakerSegmentationModelConfig {
                pyannote: OfflineSpeakerSegmentationPyannoteModelConfig {
                    model: Some(seg.to_string_lossy().into_owned()),
                    ..Default::default()
                },
                num_threads: threads,
                debug: false,
                provider: Some("cpu".into()),
            },
            embedding: SpeakerEmbeddingExtractorConfig {
                model: Some(emb.to_string_lossy().into_owned()),
                num_threads: threads,
                debug: false,
                provider: Some("cpu".into()),
            },
            clustering: FastClusteringConfig {
                num_clusters: config.num_speakers.map_or(-1, |n| n as i32),
                threshold: config.cluster_threshold,
                compute_confidence: false,
            },
            min_duration_on: 0.3,
            min_duration_off: 0.5,
        };
        let sd = OfflineSpeakerDiarization::create(&sd_config)
            .ok_or_else(|| anyhow!("failed to create sherpa-onnx speaker diarization"))?;
        if sd.sample_rate() != SAMPLE_RATE as i32 {
            return Err(anyhow!("unexpected model sample rate {}", sd.sample_rate()));
        }
        let embedder = if config.speaker_embeddings {
            Some(SpeakerEmbedder::new(&config)?)
        } else {
            None
        };
        Ok(Self {
            config,
            engine: Mutex::new(Engine { sd, sd_config }),
            embedder,
        })
    }

    /// The embedder used for per-speaker embeddings, e.g. to enroll a voice
    /// from a separate recording.
    pub fn embedder(&self) -> Option<&SpeakerEmbedder> {
        self.embedder.as_ref()
    }

    fn run(engine: &Engine, samples: &[f32], num_clusters: i32) -> Result<Vec<RawSegment>> {
        let mut cfg = engine.sd_config.clone();
        cfg.clustering.num_clusters = num_clusters;
        engine.sd.set_config(&cfg);
        let result = engine
            .sd
            .process(samples)
            .ok_or_else(|| anyhow!("sherpa-onnx diarization failed"))?;
        Ok(result
            .sort_by_start_time()
            .into_iter()
            .map(|s| RawSegment {
                start_s: s.start as f64,
                end_s: s.end as f64,
                cluster: s.speaker,
            })
            .collect())
    }

    fn raw_segments(&self, samples: &[f32]) -> Result<Vec<RawSegment>> {
        let engine = self
            .engine
            .lock()
            .map_err(|_| anyhow!("diarizer poisoned"))?;
        let forced = self.config.num_speakers.map_or(-1, |n| n as i32);
        let mut segs = Self::run(&engine, samples, forced)?;
        if forced < 0 {
            let found = postprocess::cluster_order(&segs).len();
            let min = self.config.min_speakers.unwrap_or(0);
            let max = self.config.max_speakers.unwrap_or(usize::MAX);
            let want = found.clamp(min, max.max(min));
            if want != found && found > 0 {
                segs = Self::run(&engine, samples, want as i32)?;
            }
        }
        Ok(segs)
    }
}

impl Diarizer for SherpaDiarizer {
    fn diarize(&self, samples: &[f32]) -> Result<Vec<SpeakerTurn>> {
        if (samples.len() as f64) < MIN_INPUT_S * SAMPLE_RATE as f64 {
            return Ok(Vec::new());
        }
        let raw = self.raw_segments(samples)?;
        let segs = postprocess::clean(&raw, &self.config.postprocess);
        let embeddings = match &self.embedder {
            Some(e) if !segs.is_empty() => e.cluster_embeddings(samples, &segs),
            _ => HashMap::new(),
        };
        Ok(to_turns(&segs, &embeddings))
    }
}
