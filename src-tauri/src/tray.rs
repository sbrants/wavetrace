//! System tray icon, menu, and close-to-tray behavior.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent},
    AppHandle, Manager, RunEvent, WindowEvent,
};
use crate::commands::AppState;
use crate::scanner::ScanStartMode;
use crate::settings;
use crate::state_machine::LiveState;

const TRAY_ID: &str = "main";

pub struct TrayController {
    #[allow(dead_code)]
    tray: TrayIcon,
    #[allow(dead_code)]
    show_item: MenuItem<tauri::Wry>,
    new_run_item: MenuItem<tauri::Wry>,
    resume_item: MenuItem<tauri::Wry>,
    stop_item: MenuItem<tauri::Wry>,
    #[allow(dead_code)]
    quit_item: MenuItem<tauri::Wry>,
    pub allow_exit: Arc<AtomicBool>,
}

pub fn setup(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let show_item = MenuItem::with_id(app, "tray_show", "Show WaveTrace", true, None::<&str>)?;
    let new_run_item = MenuItem::with_id(app, "tray_new_run", "New run", true, None::<&str>)?;
    let resume_item = MenuItem::with_id(app, "tray_resume", "Resume run", false, None::<&str>)?;
    let stop_item = MenuItem::with_id(app, "tray_stop", "Stop scanner", false, None::<&str>)?;
    let quit_item = MenuItem::with_id(app, "tray_quit", "Quit", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(
        app,
        &[
            &show_item,
            &separator,
            &new_run_item,
            &resume_item,
            &stop_item,
            &PredefinedMenuItem::separator(app)?,
            &quit_item,
        ],
    )?;

    let icon = app
        .default_window_icon()
        .ok_or("missing default window icon")?
        .clone();

    let allow_exit = Arc::new(AtomicBool::new(false));

    let tray = TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .tooltip("WaveTrace — stopped")
        .on_menu_event(move |app, event| {
            on_menu_event(app, event.id.as_ref());
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main_window(tray.app_handle());
            }
        })
        .build(app)?;

    app.manage(TrayController {
        tray,
        show_item,
        new_run_item,
        resume_item,
        stop_item,
        quit_item,
        allow_exit,
    });

    if let Some(window) = app.get_webview_window("main") {
        let app_handle = app.handle().clone();
        window.on_window_event(move |event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                let minimize = crate::db::open()
                    .map(|conn| settings::load(&conn).minimize_to_tray)
                    .unwrap_or(true);
                let exiting = app_handle
                    .try_state::<TrayController>()
                    .map(|t| t.allow_exit.load(Ordering::SeqCst))
                    .unwrap_or(false);
                if minimize && !exiting {
                    api.prevent_close();
                    if let Some(w) = app_handle.get_webview_window("main") {
                        let _ = w.hide();
                    }
                }
            }
        });
    }

    Ok(())
}

pub fn on_run_event(app: &AppHandle, event: &RunEvent) {
    if let RunEvent::ExitRequested { api, .. } = event {
        let exiting = app
            .try_state::<TrayController>()
            .map(|t| t.allow_exit.load(Ordering::SeqCst))
            .unwrap_or(false);
        if !exiting {
            let minimize = crate::db::open()
                .map(|conn| settings::load(&conn).minimize_to_tray)
                .unwrap_or(true);
            if minimize {
                api.prevent_exit();
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.hide();
                }
            }
        }
    }
}

pub fn update_scanner_ui(app: &AppHandle, status: &str, live: &LiveState) {
    let Some(tray) = app.try_state::<TrayController>() else {
        return;
    };
    let running = status != "stopped";
    let resumable = app
        .try_state::<AppState>()
        .and_then(|s| s.scanner.has_resumable_run().ok())
        .unwrap_or(false);

    let wave = live.wave.map(|w| w.to_string()).unwrap_or_else(|| "—".into());
    let tooltip = match status {
        "scanning" => format!("WaveTrace — scanning (wave {wave})"),
        "window_not_found" => "WaveTrace — game window not found".to_string(),
        "starting" => "WaveTrace — starting…".to_string(),
        _ => "WaveTrace — stopped".to_string(),
    };
    let _ = tray.tray.set_tooltip(Some(&tooltip));

    let _ = tray.new_run_item.set_enabled(!running);
    let _ = tray.resume_item.set_enabled(!running && resumable);
    let _ = tray.stop_item.set_enabled(running);
}

fn on_menu_event(app: &AppHandle, id: &str) {
    match id {
        "tray_show" => show_main_window(app),
        "tray_new_run" => {
            show_main_window(app);
            if let Some(state) = app.try_state::<AppState>() {
                if !state.scanner.is_running() {
                    let _ = state
                        .scanner
                        .start(app.clone(), ScanStartMode::NewRun);
                }
            }
        }
        "tray_resume" => {
            show_main_window(app);
            if let Some(state) = app.try_state::<AppState>() {
                if !state.scanner.is_running() {
                    let _ = state
                        .scanner
                        .start(app.clone(), ScanStartMode::ResumePrevious);
                }
            }
        }
        "tray_stop" => {
            if let Some(state) = app.try_state::<AppState>() {
                state.scanner.stop();
            }
        }
        "tray_quit" => exit_app(app),
        _ => {}
    }
}

/// Fully exit the app (bypass close-to-tray). Used by the tray menu and in-app exit control.
pub fn exit_app(app: &AppHandle) {
    if let Some(tray) = app.try_state::<TrayController>() {
        tray.allow_exit.store(true, Ordering::SeqCst);
    }
    if let Some(state) = app.try_state::<AppState>() {
        state.scanner.stop();
    }
    app.exit(0);
}

pub fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}
