pub mod accounts;
pub mod adb_save;
pub mod app_icon;
pub mod backup;
pub mod capture;
pub mod classify;
pub mod commands;
pub mod db;
pub mod debug_package;
pub mod diagnostics;
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
pub mod shutdown_hook;
pub mod state_machine;
pub mod tray;
pub mod window_probe;

use commands::AppState;
use notifications::NotifyState;
use tauri::Manager;

/// Route panics into the app log. A panicking background thread (e.g. the scanner)
/// otherwise dies silently: its message goes to stderr, which a windowed build has
/// nowhere to show, so the symptom reaching us is only "it stopped collecting data".
fn log_panics() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let location = info
            .location()
            .map(|l| format!("{}:{}", l.file(), l.line()))
            .unwrap_or_else(|| "unknown location".to_string());
        let thread = std::thread::current();
        let thread_name = thread.name().unwrap_or("unnamed").to_string();
        let payload = info.payload();
        let message = payload
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| payload.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "<non-string panic payload>".to_string());
        // A panic while logging the panic would abort the process, so absorb it.
        let _ = std::panic::catch_unwind(|| {
            db::append_app_log(&format!(
                "PANIC in thread {thread_name} at {location}: {message}"
            ));
        });
        default_hook(info);
    }));
}

fn show_startup_error(message: &str) {
    eprintln!("WaveTrace: {message}");
    #[cfg(windows)]
    {
        use windows::core::HSTRING;
        use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};
        unsafe {
            let _ = MessageBoxW(
                None,
                &HSTRING::from(message),
                &HSTRING::from("WaveTrace"),
                MB_OK | MB_ICONERROR,
            );
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    if let Err(e) = accounts::init() {
        show_startup_error(e.message());
        std::process::exit(if e.is_already_open() { 0 } else { 1 });
    }
    // Ensure the database and its directory exist before anything else runs.
    db::open().expect("failed to open database");
    log_panics();
    let account = accounts::current_account();
    db::append_app_log(&format!(
        "app starting account={} ({})",
        account.id, account.name
    ));

    tauri::Builder::default()
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(NotifyState::default())
        .manage(AppState {
            scanner: scanner::Scanner::default(),
            compare_capture_active: std::sync::Mutex::new(false),
            pending_wave_milestone_ntfy: std::sync::Mutex::new(None),
        })
        .setup(|app| {
            tray::setup(app)?;
            app_icon::apply_branding(app)?;
            if let Some(notify) = app.try_state::<NotifyState>() {
                notify.ensure_permission(app.handle());
            }
            if let Some(state) = app.try_state::<AppState>() {
                if let Ok(conn) = db::open() {
                    let active = settings::load(&conn).compare_capture_active;
                    *state.compare_capture_active.lock().unwrap() = active;
                }
            }
            // On macOS, prompt for Screen Recording on first launch so window
            // enumeration can read titles and capture can read pixels. No-op
            // on Windows/Linux, which don't gate this behind a permission.
            let _ = capture::request_screen_capture_access();
            shutdown_hook::install(app.handle());
            crate::adb_save::ensure_auto_pull_loop();
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
            commands::capture_app_window,
            commands::generate_debug_package,
            commands::quit_app,
            commands::get_settings,
            commands::save_settings,
            commands::send_test_ntfy,
            commands::get_ntfy_status,
            commands::clear_ntfy_rate_limit,
            commands::set_compare_capture_active,
            commands::focus_main_window,
            commands::prepare_main_window_for_capture,
            commands::complete_wave_milestone_ntfy,
            commands::has_resumable_run,
            commands::start_scanner,
            commands::stop_scanner,
            commands::scanner_running,
            commands::list_runs,
            commands::set_run_comment,
            commands::set_run_type,
            commands::delete_runs,
            commands::delete_snapshot,
            commands::delete_snapshots,
            commands::update_snapshot_golden_combo,
            commands::clear_snapshot_golden_combo,
            commands::delete_wave_skips,
            commands::delete_wave_skip,
            commands::combine_runs,
            commands::run_snapshots,
            commands::current_run_dashboard,
            commands::run_dashboard_data,
            commands::run_wave_skips,
            commands::export_csv,
            commands::export_workbook,
            commands::export_backup,
            commands::restore_backup,
            commands::preview_capture,
            commands::probe_ocr,
            commands::copy_image_to_clipboard,
            commands::read_scanner_log,
            commands::get_app_data_info,
            commands::game_save_status,
            commands::pull_game_save,
            commands::pick_save_pull_dir,
            commands::list_game_save_devices,
            commands::list_accounts,
            commands::create_account,
            commands::update_account,
            commands::delete_account,
            commands::switch_account,
            commands::open_account_window,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            tray::on_run_event(app, &event);
        });
}
