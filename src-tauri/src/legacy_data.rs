//! Sokki's data used to live under the bundle id from its "Handy Meetings"
//! days. On the first start under the new id the old folder moves over, and a
//! link stays at the old place so older builds and scripts still find it.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// (current bundle id, the id it replaced)
const RENAMED: &[(&str, &str)] = &[
    ("org.sokki.app", "dev.handymeetings.app"),
    ("org.sokki.test", "dev.handymeetings.test"),
];

static OUTCOME: OnceLock<String> = OnceLock::new();

/// Move the data folder of the old bundle id to `identifier`'s, once. Runs
/// before anything opens the data folder (and before logging is set up; see
/// [`outcome`]).
pub fn migrate(identifier: &str) {
    if crate::portable::data_dir().is_some() {
        return;
    }
    #[cfg(target_os = "macos")]
    if let Some(home) = std::env::var_os("HOME") {
        let base = PathBuf::from(home).join("Library/Application Support");
        if let Some(message) = migrate_in(&base, identifier) {
            let _ = OUTCOME.set(message);
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = identifier;
}

/// What [`migrate`] did, for the log once it is running.
pub fn outcome() -> Option<&'static str> {
    OUTCOME.get().map(String::as_str)
}

fn migrate_in(base: &Path, identifier: &str) -> Option<String> {
    let (_, old_id) = RENAMED.iter().find(|(new, _)| *new == identifier)?;
    let old = base.join(old_id);
    let new = base.join(identifier);
    if new.exists() || old.is_symlink() || !old.is_dir() {
        return None;
    }
    if let Err(e) = std::fs::rename(&old, &new) {
        return Some(format!(
            "Could not move {} to {}: {e}",
            old.display(),
            new.display()
        ));
    }
    #[cfg(unix)]
    let link = std::os::unix::fs::symlink(&new, &old);
    #[cfg(not(unix))]
    let link: std::io::Result<()> = Ok(());
    Some(match link {
        Ok(()) => format!("Moved {} to {}", old.display(), new.display()),
        Err(e) => format!(
            "Moved {} to {} (no link left behind: {e})",
            old.display(),
            new.display()
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moves_the_old_folder_and_leaves_a_link() {
        let base = tempfile::tempdir().unwrap();
        let old = base.path().join("dev.handymeetings.app");
        std::fs::create_dir_all(old.join("meetings")).unwrap();
        std::fs::write(old.join("settings_store.json"), "{}").unwrap();

        assert!(migrate_in(base.path(), "org.sokki.app").is_some());
        let new = base.path().join("org.sokki.app");
        assert!(new.join("settings_store.json").is_file());
        assert!(new.join("meetings").is_dir());
        assert!(old.is_symlink());
        assert!(old.join("settings_store.json").is_file());

        // A second start does nothing.
        assert!(migrate_in(base.path(), "org.sokki.app").is_none());
    }

    #[test]
    fn keeps_an_existing_new_folder() {
        let base = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(base.path().join("dev.handymeetings.app")).unwrap();
        std::fs::create_dir_all(base.path().join("org.sokki.app")).unwrap();
        assert!(migrate_in(base.path(), "org.sokki.app").is_none());
        assert!(!base.path().join("dev.handymeetings.app").is_symlink());
    }

    #[test]
    fn the_test_app_moves_only_its_own_folder() {
        let base = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(base.path().join("dev.handymeetings.app")).unwrap();
        std::fs::create_dir_all(base.path().join("dev.handymeetings.test")).unwrap();
        assert!(migrate_in(base.path(), "org.sokki.test").is_some());
        assert!(base.path().join("dev.handymeetings.app").is_dir());
        assert!(!base.path().join("dev.handymeetings.app").is_symlink());
        assert!(!base.path().join("org.sokki.app").exists());
    }

    #[test]
    fn unknown_ids_and_missing_folders_do_nothing() {
        let base = tempfile::tempdir().unwrap();
        assert!(migrate_in(base.path(), "org.sokki.app").is_none());
        assert!(migrate_in(base.path(), "com.example.other").is_none());
    }
}
