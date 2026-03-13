mod cache;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            cache::initialize_database(app.handle())
                .map_err(|error| -> Box<dyn std::error::Error> { error.into() })?;

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
