pub mod anchor;
pub mod capture;
pub mod classify;
pub mod commands;
pub mod db;
pub mod fields;
pub mod fixture_capture;
pub mod fixture_corpus;
pub mod ocr;
pub mod parser;
pub mod scanner;
pub mod settings;
pub mod state_machine;

use commands::AppState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Ensure the database and its directory exist before anything else runs.
    db::open().expect("failed to open database");

    tauri::Builder::default()
        .manage(AppState {
            scanner: scanner::Scanner::default(),
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_windows,
            commands::get_settings,
            commands::save_settings,
            commands::start_scanner,
            commands::stop_scanner,
            commands::scanner_running,
            commands::live_state,
            commands::manual_new_run,
            commands::list_runs,
            commands::set_run_comment,
            commands::delete_runs,
            commands::run_snapshots,
            commands::current_run_snapshots,
            commands::export_csv,
            commands::preview_capture,
            commands::capture_fixture_once,
            commands::capture_fixture_burst,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
