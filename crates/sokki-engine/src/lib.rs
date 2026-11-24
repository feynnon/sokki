//! sokki-engine: the part of Sokki every host shares.
//!
//! - [`process`]: one recorded meeting in, one Markdown note out. The host
//!   passes in its speech recogniser and speaker detector (the macOS app uses
//!   Sokki's engine and a GPU worker; the Linux service its own).
//! - [`lifecycle`]: the meeting lifecycle (idle → asking → recording) as a
//!   pure state machine, so "never record without a yes" is testable.
//! - [`live`]: transcribing a meeting while it is recorded, so only the
//!   tail is left after the call.
//! - [`llm`]: language-model settings, status, starting Ollama and
//!   downloading a model.
//! - [`tracks`], [`speakers`]: small shared pieces (track alignment, the
//!   speakers file).
//!
//! No platform code lives here; see ARCHITECTURE.md.

#[cfg(feature = "asr")]
pub mod asr;
pub mod lifecycle;
pub mod live;
pub mod llm;
pub mod process;
pub mod speakers;
pub mod tracks;

pub use process::{
    provisional_title, run, summarize_note, Deps, Input, Output, Settings, Stage, Summarized,
    SummaryMissing, SUMMARY_FAILED, SUMMARY_WAITING,
};
