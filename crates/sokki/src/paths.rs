//! Where Sokki keeps things, following the XDG base directories (Linux;
//! on the Mac only the socket is used here, see `mac.rs`).
#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

use std::path::PathBuf;

fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn xdg(var: &str, fallback: &str) -> PathBuf {
    std::env::var_os(var)
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| home().join(fallback))
}

/// `~/.config/sokki/sokki.toml`
pub fn config_file() -> PathBuf {
    xdg("XDG_CONFIG_HOME", ".config").join("sokki/sokki.toml")
}

/// `~/.local/share/sokki`
pub fn data_dir() -> PathBuf {
    xdg("XDG_DATA_HOME", ".local/share").join("sokki")
}

/// One folder per meeting: audio, `meeting.json`, `speakers.json`.
pub fn meetings_dir() -> PathBuf {
    data_dir().join("meetings")
}

pub fn models_dir() -> PathBuf {
    data_dir().join("models")
}

pub fn voice_profiles() -> PathBuf {
    data_dir().join("voice-profiles.json")
}

/// `$XDG_RUNTIME_DIR/sokki/sokki.sock` (Linux), `<data folder>/sokki.sock`
/// (macOS).
pub fn socket() -> PathBuf {
    // The Mac: in the installed app's data folder (`SOKKI_DATA_DIR` for a
    // test app), unless `SOKKI_SOCKET` says otherwise.
    #[cfg(target_os = "macos")]
    if std::env::var_os("SOKKI_SOCKET").is_none_or(|s| s.is_empty()) {
        return crate::mac::data_dir().join("sokki.sock");
    }
    sokki_api::socket_path()
}

/// Expands a leading `~/`.
pub fn expand(path: &str) -> PathBuf {
    sokki_engine::process::expand_home(path)
}
