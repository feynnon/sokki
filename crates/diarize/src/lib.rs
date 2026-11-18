//! diarize: on-device speaker diarization for meeting recordings.
//!
//! Implements [`meeting_core::Diarizer`] ("who spoke when" on one 16 kHz mono
//! track) with two interchangeable backends, plus [`VoiceProfiles`] for
//! recognising known voices across meetings.
//!
//! | Backend | Feature | Engine | Speakers | Embeddings |
//! |---|---|---|---|---|
//! | [`SortformerDiarizer`] | `sortformer` | NVIDIA Streaming Sortformer 4spk v2.1 (GGUF) via transcribe-cpp, Metal | at most 4 | from the sherpa-onnx extractor on its turns (needs `sherpa`) |
//! | [`SherpaDiarizer`] | `sherpa` | sherpa-onnx: pyannote-3.0 segmentation + speaker embeddings + clustering, CPU | any | same extractor |
//!
//! Both return turns that follow the `Diarizer` contract: sorted, no
//! overlap, silence uncovered, fragments under `min_turn_s` absorbed into a
//! neighbour, same-speaker turns across short gaps merged, speakers named
//! `S1`, `S2`, … by first appearance. When embeddings are enabled every turn
//! carries its speaker's L2-normalised embedding (identical for all turns of
//! one speaker), ready for [`VoiceProfiles::identify`].
//!
//! Models are downloaded on first use into [`DiarizeConfig::models_dir`]
//! (see [`models`]); nothing is bundled.
//!
//! **Recommended default: [`SortformerDiarizer`]** with sherpa embeddings
//! (both default features). On the synthetic fixtures it matched or beat
//! the sherpa pipeline everywhere, needs no clustering threshold, kept three
//! distinct voices apart where sherpa merged two, and ran about 4x faster.
//! Use [`SherpaDiarizer`] when a track can hold more than four speakers.
//! Results per scenario: `cargo test --release --test fixtures report -- --nocapture`.
//!
//! Profile embeddings default to 3D-Speaker ERes2Net (VoxCeleb): it
//! separated different voices far better than WeSpeaker ResNet34 (cosine
//! ≤ 0.40 vs. up to 0.80 between different voices; 0.97 for the same voice
//! with both), which sherpa still uses for clustering.
//!
//! Drop a [`SortformerDiarizer`] before the process exits: ggml-metal
//! asserts at exit when a Metal-backed model is still alive (e.g. kept in a
//! `static`).
//!
//! ```no_run
//! # fn main() -> anyhow::Result<()> {
//! use diarize::{DiarizeConfig, SortformerDiarizer, VoiceProfiles};
//! use meeting_core::Diarizer;
//! let config = DiarizeConfig::new("/path/to/models");
//! let diarizer = SortformerDiarizer::new(config)?;
//! # let samples: Vec<f32> = vec![];
//! let turns = diarizer.diarize(&samples)?;
//! let voices = VoiceProfiles::open("/path/to/voices.json")?;
//! for t in &turns {
//!     let who = t.embedding.as_deref().and_then(|e| voices.identify(e));
//!     println!("{:.1}-{:.1} {} {:?}", t.start_s, t.end_s, t.speaker, who);
//! }
//! # Ok(()) }
//! ```

use std::path::PathBuf;

pub mod models;
pub mod postprocess;
pub mod profiles;
mod vector;

#[cfg(feature = "sherpa")]
mod embedder;
#[cfg(feature = "sherpa")]
mod sherpa;
#[cfg(feature = "sortformer")]
mod sortformer;

#[cfg(feature = "sherpa")]
pub use embedder::SpeakerEmbedder;
pub use meeting_core::{Diarizer, SpeakerTurn};
pub use models::EmbeddingModel;
pub use postprocess::PostprocessConfig;
pub use profiles::{VoiceProfile, VoiceProfiles};
#[cfg(feature = "sherpa")]
pub use sherpa::SherpaDiarizer;
#[cfg(feature = "sortformer")]
pub use sortformer::SortformerDiarizer;
#[cfg(feature = "sortformer")]
pub use transcribe_cpp::SortformerPreset;
pub use vector::{cosine, l2_normalize};

/// Settings shared by all backends.
#[derive(Debug, Clone, PartialEq)]
pub struct DiarizeConfig {
    /// Where model files are cached (created if missing).
    pub models_dir: PathBuf,
    /// Download missing models. When false, missing files are an error.
    pub allow_download: bool,
    /// Exact number of speakers, if known. Sherpa: forces the cluster count.
    /// Sortformer: treated as a maximum (it cannot split a speaker).
    pub num_speakers: Option<usize>,
    /// Lower bound on speakers (sherpa only; Sortformer cannot split).
    pub min_speakers: Option<usize>,
    /// Upper bound on speakers. Sortformer: closest speakers (by embedding)
    /// are merged until the bound holds; needs embeddings.
    pub max_speakers: Option<usize>,
    /// Sherpa clustering threshold (cosine distance); larger gives fewer
    /// speakers. Ignored when the speaker count is forced. Only tuned for
    /// `clustering_model = WespeakerResnet34Lm`: 0.4 split a phone voice in
    /// two on the fixtures, 0.6 merged two speakers.
    pub cluster_threshold: f32,
    /// Embedding model sherpa clusters with (sherpa backend only).
    pub clustering_model: EmbeddingModel,
    /// Embedding model for the per-speaker embeddings returned on turns,
    /// i.e. what [`VoiceProfiles`] stores and compares.
    pub embedding_model: EmbeddingModel,
    /// Compute a per-speaker embedding and attach it to turns.
    pub speaker_embeddings: bool,
    /// CPU threads for the ONNX models.
    pub num_threads: usize,
    /// Turn clean-up.
    pub postprocess: PostprocessConfig,
}

impl DiarizeConfig {
    pub fn new(models_dir: impl Into<PathBuf>) -> Self {
        Self {
            models_dir: models_dir.into(),
            allow_download: true,
            num_speakers: None,
            min_speakers: None,
            max_speakers: None,
            cluster_threshold: 0.5,
            clustering_model: EmbeddingModel::WespeakerResnet34Lm,
            embedding_model: EmbeddingModel::ERes2NetVoxceleb,
            speaker_embeddings: true,
            num_threads: 4,
            postprocess: PostprocessConfig::default(),
        }
    }
}

/// Converts cleaned segments into `SpeakerTurn`s labelled `S1`, `S2`, … by
/// first appearance, attaching per-cluster embeddings when present.
#[cfg(any(feature = "sherpa", feature = "sortformer"))]
fn to_turns(
    segs: &[postprocess::RawSegment],
    embeddings: &std::collections::HashMap<i32, Vec<f32>>,
) -> Vec<meeting_core::SpeakerTurn> {
    let order = postprocess::cluster_order(segs);
    segs.iter()
        .map(|s| {
            let idx = order.iter().position(|c| *c == s.cluster).unwrap_or(0);
            meeting_core::SpeakerTurn {
                start_s: s.start_s,
                end_s: s.end_s,
                speaker: format!("S{}", idx + 1),
                embedding: embeddings.get(&s.cluster).cloned(),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    #[allow(unused_imports)]
    use super::*;

    #[cfg(any(feature = "sherpa", feature = "sortformer"))]
    #[test]
    fn turns_labelled_by_first_appearance() {
        use postprocess::RawSegment;
        let segs = [
            RawSegment {
                start_s: 0.0,
                end_s: 1.0,
                cluster: 2,
            },
            RawSegment {
                start_s: 1.5,
                end_s: 2.0,
                cluster: 0,
            },
            RawSegment {
                start_s: 2.5,
                end_s: 3.0,
                cluster: 2,
            },
        ];
        let mut emb = std::collections::HashMap::new();
        emb.insert(0, vec![1.0, 0.0]);
        let turns = to_turns(&segs, &emb);
        let names: Vec<_> = turns.iter().map(|t| t.speaker.as_str()).collect();
        assert_eq!(names, vec!["S1", "S2", "S1"]);
        assert_eq!(turns[1].embedding, Some(vec![1.0, 0.0]));
        assert_eq!(turns[0].embedding, None);
    }
}
