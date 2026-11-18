//! Build-time brand profile. The default build is Handy; a build with
//! `HANDY_BRAND=sokki` (and `VITE_HANDY_BRAND=sokki` for the frontend,
//! plus `--config src-tauri/brands/sokki/tauri.conf.json`) is "Sokki":
//! its own name, app icon, menu bar icons, logo and colours. Data, settings
//! and the bundle identifier are the same for every brand.
//! `scripts/build-and-install.sh` sets all three.

/// The brand id this binary was built for: "handy" or "sokki".
pub fn id() -> &'static str {
    option_env!("HANDY_BRAND").unwrap_or("handy")
}

pub fn is_sokki() -> bool {
    id() == "sokki"
}

/// The app's name in windows, dialogs and the tray menu.
pub fn app_name() -> &'static str {
    if is_sokki() {
        "Sokki"
    } else {
        "Handy Meetings"
    }
}

/// A tray icon resource path ("resources/tray_idle.png") for this brand.
pub fn tray_icon(default: &'static str) -> &'static str {
    if !is_sokki() {
        return default;
    }
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
        "resources/handy.png" => "resources/brands/sokki/sokki.png",
        "resources/handy_warning.png" => "resources/brands/sokki/sokki_warning.png",
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
