//! The heads-up line for the call chat: says Sokki takes notes, that nothing
//! leaves the laptop, asks everyone's okay and names sokki.org. Sokki only
//! puts it on the clipboard; the user sends it.

use tauri::AppHandle;
use tauri_plugin_clipboard_manager::ClipboardExt;

const DE: &str =
    "Ich schreibe mit Sokki mit. Läuft lokal auf meinem Laptop, nichts wird hochgeladen. Okay für euch? sokki.org";
const EN: &str =
    "I'm taking notes with Sokki. It runs on my laptop and uploads nothing. Okay for you? sokki.org";

/// The line in the app's language: German for `de*`, English otherwise.
pub fn text(app_language: &str) -> &'static str {
    if app_language.starts_with("de") {
        DE
    } else {
        EN
    }
}

/// Puts the heads-up on the clipboard.
pub fn copy(app: &AppHandle) -> Result<(), String> {
    let language = crate::settings::get_settings(app).app_language;
    app.clipboard()
        .write_text(text(&language))
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_the_app_language() {
        assert_eq!(text("de"), DE);
        assert_eq!(text("de-AT"), DE);
        assert_eq!(text("en-US"), EN);
        assert_eq!(text("fr"), EN);
    }
}
