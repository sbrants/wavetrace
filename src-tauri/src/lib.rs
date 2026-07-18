pub mod app_icon;
pub mod backup;
pub mod capture;
pub mod classify;
pub mod commands;
pub mod db;
pub mod export;
pub mod fields;
pub mod fixture_capture;
pub mod fixture_corpus;
pub mod dissonance_icons;
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
            app_icon::apply_branding(app)?;
            if let Some(notify) = app.try_state::<NotifyState>() {
                notify.ensure_permission(app.handle());
            }
            // On macOS, prompt for Screen Recording on first launch so window
            // enumeration can read titles and capture can read pixels. No-op
            // on Windows/Linux, which don't gate this behind a permission.
            let _ = capture::request_screen_capture_access();
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_windows,
            commands::screen_capture_access,
            commands::request_screen_capture_access,
            commands::open_screen_recording_settings,
            commands::open_external_url,
            commands::open_scanner_logs_folder,
            commands::append_app_log,
            commands::quit_app,
            commands::get_settings,
            commands::save_settings,
            commands::send_test_ntfy,
            commands::has_resumable_run,
            commands::start_scanner,
            commands::stop_scanner,
            commands::scanner_running,
            commands::live_state,
            commands::manual_new_run,
            commands::list_runs,
            commands::set_run_comment,
            commands::set_run_type,
            commands::delete_runs,
            commands::delete_snapshot,
            commands::delete_snapshots,
            commands::delete_wave_skips,
            commands::delete_wave_skip,
            commands::combine_runs,
            commands::run_snapshots,
            commands::current_run_snapshots,
            commands::current_run_dashboard,
            commands::run_dashboard_data,
            commands::run_wave_skips,
            commands::current_run_wave_skips,
            commands::export_csv,
            commands::export_workbook,
            commands::export_backup,
            commands::restore_backup,
            commands::preview_capture,
            commands::probe_ocr,
            #[cfg(debug_assertions)]
            commands::capture_fixture_once,
            #[cfg(debug_assertions)]
            commands::capture_fixture_burst,
            commands::copy_image_to_clipboard,
            commands::read_scanner_log,
            commands::get_app_data_info,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            tray::on_run_event(app, &event);
        });
}
