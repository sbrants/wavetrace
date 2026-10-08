//! Reloads the main window's page when its WebView2 renderer process dies or hangs.
//!
//! WebView2 does not recover a crashed renderer on its own: the window just goes
//! blank while the backend keeps scanning. A renderer killed by a JS heap OOM after
//! a long session is the case that motivated this — reloading gives the UI a fresh
//! heap and the scanner never notices.

use tauri::AppHandle;

pub fn install(app: &AppHandle) {
    #[cfg(windows)]
    if let Err(e) = windows::install(app) {
        crate::db::append_app_log(&format!("renderer recovery hook failed: {e}"));
    }
    #[cfg(not(windows))]
    let _ = app;
}

#[cfg(windows)]
mod windows {
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    use tauri::{AppHandle, Manager};
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        COREWEBVIEW2_PROCESS_FAILED_KIND, COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_EXITED,
        COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_UNRESPONSIVE,
    };
    use webview2_com::ProcessFailedEventHandler;

    /// Cap on automatic reloads inside `RELOAD_WINDOW`, so a page that dies right
    /// after loading can't spin in a crash/reload loop.
    const MAX_RELOADS: usize = 3;
    const RELOAD_WINDOW: Duration = Duration::from_secs(10 * 60);

    static RECENT_RELOADS: Mutex<Vec<Instant>> = Mutex::new(Vec::new());

    pub fn install(app: &AppHandle) -> Result<(), String> {
        let window = app
            .get_webview_window("main")
            .ok_or("main window not found for renderer recovery")?;
        window
            .with_webview(|webview| {
                if let Err(e) = attach(&webview) {
                    crate::db::append_app_log(&format!("renderer recovery attach failed: {e}"));
                }
            })
            .map_err(|e| e.to_string())
    }

    fn attach(webview: &tauri::webview::PlatformWebview) -> windows::core::Result<()> {
        let handler = ProcessFailedEventHandler::create(Box::new(|sender, args| {
            let Some(args) = args else { return Ok(()) };
            let mut kind = COREWEBVIEW2_PROCESS_FAILED_KIND::default();
            unsafe { args.ProcessFailedKind(&mut kind)? };
            let reason = if kind == COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_EXITED {
                "exited"
            } else if kind == COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_UNRESPONSIVE {
                "unresponsive"
            } else {
                crate::db::append_app_log(&format!("WebView2 process failed (kind {})", kind.0));
                return Ok(());
            };
            if !allow_reload() {
                crate::db::append_app_log(&format!(
                    "WebView2 renderer {reason}; not reloading ({MAX_RELOADS} reloads in the last {} min)",
                    RELOAD_WINDOW.as_secs() / 60
                ));
                return Ok(());
            }
            crate::db::append_app_log(&format!("WebView2 renderer {reason}; reloading page"));
            if let Some(sender) = sender {
                unsafe { sender.Reload()? };
            }
            Ok(())
        }));
        let mut token = 0i64;
        unsafe {
            webview
                .controller()
                .CoreWebView2()?
                .add_ProcessFailed(&handler, &mut token)
        }
    }

    fn allow_reload() -> bool {
        let Ok(mut recent) = RECENT_RELOADS.lock() else { return false };
        let now = Instant::now();
        recent.retain(|t| now.duration_since(*t) < RELOAD_WINDOW);
        if recent.len() >= MAX_RELOADS {
            return false;
        }
        recent.push(now);
        true
    }
}
