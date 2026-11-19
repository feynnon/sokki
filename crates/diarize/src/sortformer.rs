//! Sortformer backend: NVIDIA Streaming Sortformer 4spk v2.1 (GGUF) through
//! transcribe-cpp, the engine Handy already ships (ggml; Metal on Apple
//! Silicon). Speaker embeddings for its turns come from the sherpa-onnx
//! extractor when the `sherpa` feature is on ("hybrid").

use std::collections::HashMap;
use std::sync::Mutex;

use anyhow::{anyhow, Context, Result};
use meeting_core::{Diarizer, SpeakerTurn, SAMPLE_RATE};
use transcribe_cpp::{
    DiarizeExtension, DiarizeOptions, DiarizeSession, DiarizeSessionOptions, Model,
    SortformerDiarizeOptions, SortformerPreset,
};

use crate::models::{self, SORTFORMER_Q8_0};
use crate::postprocess::{self, RawSegment};
use crate::{to_turns, DiarizeConfig};

#[cfg(feature = "sherpa")]
use crate::embedder::SpeakerEmbedder;

const MIN_INPUT_S: f64 = 0.5;

/// Diarization with Streaming Sortformer (at most 4 speakers per track).
pub struct SortformerDiarizer {
    config: DiarizeConfig,
    // Kept alive alongside the session (the session also holds a reference).
    _model: Model,
    session: Mutex<DiarizeSession>,
    preset: Option<SortformerPreset>,
    #[cfg(feature = "sherpa")]
    embedder: Option<SpeakerEmbedder>,
    #[cfg(feature = "sherpa")]
    embedder_error: Option<String>,
}

impl SortformerDiarizer {
    /// Most speakers the model can tell apart.
    pub const MAX_SPEAKERS: usize = 4;

    /// Loads the Sortformer model (and the embedding model when
    /// `config.speaker_embeddings` is set), downloading them when allowed.
    pub fn new(config: DiarizeConfig) -> Result<Self> {
        let model = models::ensure_and_load(
            &config.models_dir,
            &SORTFORMER_Q8_0,
            config.allow_download,
            |path| {
                Model::load(path)
                    .with_context(|| format!("load Sortformer model {}", path.display()))
            },
        )?;
        let info = model.diarize_info().context("model has no diarize role")?;
        if info.sample_rate != SAMPLE_RATE as i32 {
            return Err(anyhow!("unexpected model sample rate {}", info.sample_rate));
        }
        let session = model
            .diarize_session_with(&DiarizeSessionOptions {
                n_threads: config.num_threads as i32,
            })
            .context("open Sortformer diarize session")?;
        // Without the embedding model, speakers are still told apart; only
        // known voices go unrecognised. So its failure is not fatal here.
        #[cfg(feature = "sherpa")]
        let (embedder, embedder_error) = if config.speaker_embeddings {
            match SpeakerEmbedder::new(&config) {
                Ok(e) => (Some(e), None),
                Err(e) => {
                    log::warn!("Speaker embeddings unavailable: {e:#}");
                    (None, Some(format!("{e:#}")))
                }
            }
        } else {
            (None, None)
        };
        Ok(Self {
            config,
            _model: model,
            session: Mutex::new(session),
            preset: Some(SortformerPreset::VeryHighLatency),
            #[cfg(feature = "sherpa")]
            embedder,
            #[cfg(feature = "sherpa")]
            embedder_error,
        })
    }

    /// Why speaker embeddings are missing although they were asked for
    /// (the embedding model could not be loaded); `None` otherwise.
    #[cfg(feature = "sherpa")]
    pub fn embedder_error(&self) -> Option<&str> {
        self.embedder_error.as_deref()
    }

    /// Sortformer operating point. Default here: `VeryHighLatency`
    /// (~30 s lookahead), the offline-file setting. `None` uses the
    /// checkpoint's own configuration.
    pub fn with_preset(mut self, preset: Option<SortformerPreset>) -> Self {
        self.preset = preset;
        self
    }

    #[cfg(feature = "sherpa")]
    pub fn embedder(&self) -> Option<&SpeakerEmbedder> {
        self.embedder.as_ref()
    }

    fn raw_segments(&self, samples: &[f32]) -> Result<Vec<RawSegment>> {
        let mut session = self
            .session
            .lock()
            .map_err(|_| anyhow!("diarizer poisoned"))?;
        let options = DiarizeOptions {
            family: Some(DiarizeExtension::Sortformer(SortformerDiarizeOptions {
                preset: self.preset,
            })),
        };
        let segs = session.run(samples, &options).context("Sortformer run")?;
        Ok(segs
            .into_iter()
            .map(|s| RawSegment {
                start_s: s.t0_ms as f64 / 1000.0,
                end_s: s.t1_ms as f64 / 1000.0,
                cluster: s.speaker_id,
            })
            .collect())
    }

    fn max_speakers(&self) -> Option<usize> {
        match (self.config.num_speakers, self.config.max_speakers) {
            (Some(n), _) => Some(n),
            (None, m) => m,
        }
    }
}

impl Diarizer for SortformerDiarizer {
    fn diarize(&self, samples: &[f32]) -> Result<Vec<SpeakerTurn>> {
        if (samples.len() as f64) < MIN_INPUT_S * SAMPLE_RATE as f64 {
            return Ok(Vec::new());
        }
        let raw = self.raw_segments(samples)?;
        let mut segs = postprocess::clean(&raw, &self.config.postprocess);
        #[allow(unused_mut)]
        let mut embeddings: HashMap<i32, Vec<f32>> = HashMap::new();
        #[cfg(feature = "sherpa")]
        if let Some(e) = &self.embedder {
            if !segs.is_empty() {
                embeddings = e.cluster_embeddings(samples, &segs);
            }
        }
        if let Some(min) = self.config.merge_similarity {
            if postprocess::merge_similar(&mut segs, &mut embeddings, min) > 0 {
                segs = postprocess::clean(&segs, &self.config.postprocess);
            }
        }
        if let Some(max) = self.max_speakers() {
            postprocess::merge_clusters_to(&mut segs, &mut embeddings, max.max(1));
            segs = postprocess::clean(&segs, &self.config.postprocess);
        }
        Ok(to_turns(&segs, &embeddings))
    }
}
