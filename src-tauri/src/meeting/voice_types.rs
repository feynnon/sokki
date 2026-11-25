//! What the Sprecher page and a meeting's name proposals send to the app
//! (`voices.rs`, macOS only); here so every platform has the same bindings.

/// Something a voice said, to recognise it by.
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct VoiceLine {
    pub meeting_id: String,
    pub meeting_title: String,
    pub started_at: String,
    pub speaker: String,
    pub start_s: f64,
    pub text: String,
    /// The meeting's audio is still there, so the line can be played.
    pub audio: bool,
}

/// A name proposed for a voice, with why.
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct VoiceSuggestion {
    pub name: String,
    pub email: Option<String>,
    /// Meetings of this voice the person was invited to.
    pub invited: u32,
    /// Meetings of this voice with a known guest list.
    pub of: u32,
}

/// One voice on the Sprecher page.
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct VoiceInfo {
    pub id: String,
    pub name: Option<String>,
    pub is_user: bool,
    pub meetings: u32,
    /// Meetings where the voice is still "Speaker N".
    pub unnamed: u32,
    pub last: String,
    pub members: Vec<VoiceMember>,
    pub emails: Vec<String>,
    /// Sokki keeps this voice (the person agreed) and recognises it.
    pub remembered: bool,
    pub suggestion: Option<VoiceSuggestion>,
    pub lines: Vec<VoiceLine>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct VoiceMember {
    pub meeting_id: String,
    pub speaker: String,
}

#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct VoicesPage {
    /// Voices with a name, and voices without one heard in 2+ meetings.
    pub voices: Vec<VoiceInfo>,
    /// Voices without a name heard in one meeting only (named there).
    pub once: u32,
}

/// A name proposed for a speaker of a meeting, for the app.
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct NameCandidate {
    pub name: String,
    pub email: Option<String>,
    /// "voice": the same voice has this name elsewhere; "invited".
    pub source: String,
}

/// What Sokki proposes for one unnamed speaker, for the app.
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct SpeakerProposal {
    pub speaker: String,
    pub candidates: Vec<NameCandidate>,
}
