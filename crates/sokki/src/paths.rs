//! Where Sokki keeps things, following the XDG base directories.

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

/// `$XDG_RUNTIME_DIR/sokki/sokki.sock`
pub fn socket() -> PathBuf {
    let runtime = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join(format!("sokki-{}", unsafe { libc::getuid() })));
    runtime.join("sokki/sokki.sock")
}

/// Expands a leading `~/`.
pub fn expand(path: &str) -> PathBuf {
    sokki_engine::process::expand_home(path)
}
