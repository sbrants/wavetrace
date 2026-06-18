pub mod capture;
pub mod classify;
pub mod commands;
pub mod db;
pub mod export;
pub mod fields;
pub mod fixture_capture;
pub mod fixture_corpus;
pub mod notifications;
pub mod ocr;
pub mod parser;
pub mod scanner;
pub mod settings;
pub mod state_machine;
pub mod tray;

use commands::AppState;
use notifications::NotifyState;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Ensure the database and its directory exist before anything else runs.
    db::open().expect("failed to open database");

    tauri::Builder::default()
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(NotifyState::default())
        .manage(AppState {
            scanner: scanner::Scanner::default(),
        })
        .setup(|app| {
            tray::setup(app)?;
            if let Some(notify) = app.try_state::<NotifyState>() {
                notify.ensure_permission(app.handle());
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_windows,
            commands::get_settings,
            commands::save_settings,
            commands::has_resumable_run,
            commands::start_scanner,
            commands::stop_scanner,
            commands::scanner_running,
            commands::live_state,
            commands::manual_new_run,
            commands::list_runs,
            commands::set_run_comment,
            commands::delete_runs,
            commands::delete_snapshot,
            commands::delete_snapshots,
            commands::combine_runs,
            commands::run_snapshots,
            commands::current_run_snapshots,
            commands::export_csv,
            commands::export_workbook,
            commands::preview_capture,
            commands::probe_ocr,
            #[cfg(debug_assertions)]
            commands::capture_fixture_once,
            #[cfg(debug_assertions)]
            commands::capture_fixture_burst,
            commands::copy_image_to_clipboard,
            commands::read_scanner_log,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            tray::on_run_event(app, &event);
        });
}
