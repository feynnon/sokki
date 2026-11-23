//! Sokki's name and icons. The menu bar icons live in
//! `resources/brands/sokki/`; the app icon and update feed in
//! `brands/sokki/tauri.conf.json`.

/// The app's name in windows, dialogs and the tray menu.
pub fn app_name() -> &'static str {
    "Sokki"
}

/// A tray icon resource path ("resources/tray_idle.png") for this brand.
pub fn tray_icon(default: &'static str) -> &'static str {
    match default {
        "resources/tray_idle.png" => "resources/brands/sokki/tray_idle.png",
        "resources/tray_idle_dark.png" => "resources/brands/sokki/tray_idle_dark.png",
        "resources/tray_recording.png" => "resources/brands/sokki/tray_recording.png",
        "resources/tray_recording_dark.png" => "resources/brands/sokki/tray_recording_dark.png",
        "resources/tray_transcribing.png" => "resources/brands/sokki/tray_transcribing.png",
        "resources/tray_transcribing_dark.png" => {
            "resources/brands/sokki/tray_transcribing_dark.png"
        }
        "resources/tray_idle_warning.png" => "resources/brands/sokki/tray_idle_warning.png",
        "resources/tray_idle_warning_dark.png" => {
            "resources/brands/sokki/tray_idle_warning_dark.png"
        }
        "resources/recording.png" => "resources/brands/sokki/recording.png",
        "resources/transcribing.png" => "resources/brands/sokki/transcribing.png",
        other => other,
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn every_brand_tray_icon_exists() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        for f in std::fs::read_dir(root.join("resources/brands/sokki")).unwrap() {
            let name = f.unwrap().file_name().into_string().unwrap();
            assert!(name.ends_with(".png"), "{name}");
        }
        for icon in [
            "tray_idle",
            "tray_idle_dark",
            "tray_recording",
            "tray_recording_dark",
            "tray_transcribing",
            "tray_transcribing_dark",
            "tray_idle_warning",
            "tray_idle_warning_dark",
            "sokki",
            "sokki_warning",
            "recording",
            "transcribing",
        ] {
            assert!(
                root.join(format!("resources/brands/sokki/{icon}.png"))
                    .exists(),
                "{icon}"
            );
        }
    }
}
