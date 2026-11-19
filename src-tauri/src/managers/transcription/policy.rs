//! How audio is decoded: the one place the model's traits and the settings
//! choose between streaming, long-form chunking and a single run.

use crate::engine_supervisor::LoadedInfo;

/// What a model can do that decides how its audio is decoded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModelTraits {
    /// Decodes audio incrementally as it is recorded.
    pub streaming: bool,
    /// Keeps its accuracy on audio longer than its native window in one run
    /// (whisper today).
    pub long_form: bool,
}

impl ModelTraits {
    /// The loaded model's own traits: the authority once a model is loaded.
    pub fn of(info: &LoadedInfo) -> Self {
        Self {
            streaming: info.capabilities.supports_streaming,
            long_form: info.supports_long_form,
        }
    }
}

/// How a recording is decoded while it is still going.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LiveDecode {
    /// The model decodes the audio as it arrives.
    Stream,
    /// Long audio is cut at pauses and each chunk decoded once its cut is
    /// certain. With live text on, the chunk still growing is previewed too.
    Chunked,
    /// Live text only: decoded as `Chunked` for the overlay, then the whole
    /// recording in batch when it stops, so the result is what it would be
    /// without live text.
    Preview,
    /// Nothing until the recording stops; then it is decoded in batch.
    None,
}

/// How a whole clip is decoded: a recording without a live result, a
/// re-transcribed history entry, or a file from the CLI.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BatchDecode {
    /// Cut at pauses into chunks of about 30 s, each decoded in turn; a clip
    /// of 40 s or less is one chunk.
    Chunked,
    /// One run.
    Whole,
}

/// Streaming wins: a streaming model handles long audio itself. Otherwise
/// long audio is chunked unless the model keeps its accuracy on it, and
/// `live_text` (the Live overlay) previews what a model that doesn't stream
/// hears.
pub fn live_decode(model: ModelTraits, long_form_chunking: bool, live_text: bool) -> LiveDecode {
    if model.streaming {
        LiveDecode::Stream
    } else if long_form_chunking && !model.long_form {
        LiveDecode::Chunked
    } else if live_text {
        LiveDecode::Preview
    } else {
        LiveDecode::None
    }
}

/// As live, except that a streaming model takes a whole clip in one run, as
/// it did before long-form chunking existed.
pub fn batch_decode(model: ModelTraits, long_form_chunking: bool) -> BatchDecode {
    if long_form_chunking && !model.long_form && !model.streaming {
        BatchDecode::Chunked
    } else {
        BatchDecode::Whole
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PARAKEET: ModelTraits = ModelTraits {
        streaming: false,
        long_form: false,
    };
    const WHISPER: ModelTraits = ModelTraits {
        streaming: false,
        long_form: true,
    };
    const NEMOTRON: ModelTraits = ModelTraits {
        streaming: true,
        long_form: false,
    };

    #[test]
    fn live_text_previews_models_that_dont_stream() {
        assert_eq!(live_decode(NEMOTRON, true, true), LiveDecode::Stream);
        // Chunked keeps its result; live text only adds previews to it.
        assert_eq!(live_decode(PARAKEET, true, true), LiveDecode::Chunked);
        assert_eq!(live_decode(PARAKEET, false, true), LiveDecode::Preview);
        assert_eq!(live_decode(WHISPER, true, true), LiveDecode::Preview);
        assert_eq!(live_decode(WHISPER, true, false), LiveDecode::None);
        assert_eq!(live_decode(PARAKEET, false, false), LiveDecode::None);
    }
}
