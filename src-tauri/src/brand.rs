//! Build-time brand profile. The default build is Handy; a build with
//! `HANDY_BRAND=verbatim` (and `VITE_HANDY_BRAND=verbatim` for the frontend,
//! plus `--config src-tauri/brands/verbatim/tauri.conf.json`) is "Verbatim":
//! its own name, app icon, menu bar icons, logo and colours. Data, settings
//! and the bundle identifier are the same for every brand.
//! `scripts/build-and-install.sh` sets all three.

/// The brand id this binary was built for: "handy" or "verbatim".
pub fn id() -> &'static str {
    option_env!("HANDY_BRAND").unwrap_or("handy")
}

pub fn is_verbatim() -> bool {
    id() == "verbatim"
}

/// The app's name in windows, dialogs and the tray menu.
pub fn app_name() -> &'static str {
    if is_verbatim() {
        "Verbatim"
    } else {
        "Handy Meetings"
    }
}

/// A tray icon resource path ("resources/tray_idle.png") for this brand.
pub fn tray_icon(default: &'static str) -> &'static str {
    if !is_verbatim() {
        return default;
    }
    match default {
        "resources/tray_idle.png" => "resources/brands/verbatim/tray_idle.png",
        "resources/tray_idle_dark.png" => "resources/brands/verbatim/tray_idle_dark.png",
        "resources/tray_recording.png" => "resources/brands/verbatim/tray_recording.png",
        "resources/tray_recording_dark.png" => "resources/brands/verbatim/tray_recording_dark.png",
        "resources/tray_transcribing.png" => "resources/brands/verbatim/tray_transcribing.png",
        "resources/tray_transcribing_dark.png" => {
            "resources/brands/verbatim/tray_transcribing_dark.png"
        }
        "resources/tray_idle_warning.png" => "resources/brands/verbatim/tray_idle_warning.png",
        "resources/tray_idle_warning_dark.png" => {
            "resources/brands/verbatim/tray_idle_warning_dark.png"
        }
        "resources/handy.png" => "resources/brands/verbatim/verbatim.png",
        "resources/handy_warning.png" => "resources/brands/verbatim/verbatim_warning.png",
        "resources/recording.png" => "resources/brands/verbatim/recording.png",
        "resources/transcribing.png" => "resources/brands/verbatim/transcribing.png",
        other => other,
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn every_brand_tray_icon_exists() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        for f in std::fs::read_dir(root.join("resources/brands/verbatim")).unwrap() {
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
            "verbatim",
            "verbatim_warning",
            "recording",
            "transcribing",
        ] {
            assert!(
                root.join(format!("resources/brands/verbatim/{icon}.png"))
                    .exists(),
                "{icon}"
            );
        }
    }
}
