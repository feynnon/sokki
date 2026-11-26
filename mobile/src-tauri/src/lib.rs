mod dictation;
#[cfg(target_os = "android")]
mod keyboard_voice;
mod link;
mod pieces;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default().plugin(tauri_plugin_opener::init());
    #[cfg(mobile)]
    let builder = builder.plugin(tauri_plugin_barcode_scanner::init());
    builder
        .manage(dictation::Dictation::default())
        .manage(link::Link::default())
        .setup(|app| {
            dictation::update_model(app.handle());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            dictation::dictation_model,
            dictation::dictation_download,
            dictation::dictation_load,
            dictation::dictation_start,
            dictation::dictation_push,
            dictation::dictation_stop,
            link::link_pairing,
            link::link_pair,
            link::link_unpair,
            link::link_ping,
            link::link_upload,
            link::link_status,
            link::link_send_journal,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
