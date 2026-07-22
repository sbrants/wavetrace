//! Best-effort OS shutdown / restart detection (e.g. Windows Update reboot).

use std::sync::atomic::{AtomicBool, Ordering};

use tauri::{AppHandle, Manager};

static SHUTDOWN_NOTIFIED: AtomicBool = AtomicBool::new(false);

pub fn install(app: &AppHandle) {
    #[cfg(windows)]
    if let Err(e) = windows::install(app) {
        eprintln!("shutdown hook (windows) failed: {e}");
    }
    #[cfg(unix)]
    unix::install(app.clone());
}

fn notify_once(app: &AppHandle) {
    if SHUTDOWN_NOTIFIED.swap(true, Ordering::SeqCst) {
        return;
    }
    crate::notifications::on_system_shutdown(app);
}

#[cfg(windows)]
mod windows {
    use super::notify_once;
    use tauri::{AppHandle, Manager};
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
    use windows::Win32::UI::WindowsAndMessaging::WM_QUERYENDSESSION;

    const SUBCLASS_ID: usize = 0x5741_5645; // "WAVE"

    struct HookData {
        app: AppHandle,
    }

    pub fn install(app: &AppHandle) -> Result<(), String> {
        let window = app
            .get_webview_window("main")
            .ok_or("main window not found for shutdown hook")?;
        let hwnd = window
            .hwnd()
            .map_err(|e| format!("main window hwnd unavailable: {e}"))?;
        let hwnd = windows::Win32::Foundation::HWND(hwnd.0);
        let leaked = Box::leak(Box::new(HookData { app: app.clone() }));
        unsafe {
            if !SetWindowSubclass(
                hwnd,
                Some(shutdown_subclass_proc),
                SUBCLASS_ID,
                leaked as *const HookData as usize,
            )
            .as_bool()
            {
                return Err("SetWindowSubclass failed".into());
            }
        }
        Ok(())
    }

    unsafe extern "system" fn shutdown_subclass_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
        _id: usize,
        data: usize,
    ) -> LRESULT {
        if msg == WM_QUERYENDSESSION && wparam.0 != 0 {
            let hook = &*(data as *const HookData);
            notify_once(&hook.app);
        }
        unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
    }
}

#[cfg(unix)]
mod unix {
    use super::notify_once;
    use signal_hook::consts::SIGTERM;
    use signal_hook::iterator::Signals;
    use tauri::AppHandle;

    pub fn install(app: AppHandle) {
        std::thread::spawn(move || {
            let Ok(mut signals) = Signals::new([SIGTERM]) else {
                return;
            };
            for sig in &mut signals {
                if sig == SIGTERM {
                    notify_once(&app);
                }
            }
        });
    }
}
