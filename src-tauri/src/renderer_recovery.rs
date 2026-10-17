//! Recovers the main window's UI when its WebView2 processes die or hang.
//!
//! WebView2 does not recover on its own: the window just goes blank while the backend
//! keeps scanning. Two cases:
//! - The **renderer** (the page) exits or hangs — e.g. killed by a JS heap OOM after a long
//!   session. Reloading gives the UI a fresh heap and the scanner never notices.
//! - The **browser process** exits — seen as a WebView2 out-of-memory crash at ~14 GB
//!   after ~13 hours. That takes the whole WebView down permanently ("the app has to
//!   recreate a new WebView", per the WebView2 docs), so the `main` window is rebuilt
//!   from its config, which starts a fresh browser process. Scanning isn't interrupted.

use std::sync::atomic::{AtomicBool, Ordering};

use tauri::AppHandle;

/// True while the main window is being rebuilt. Destroying it leaves the app with no
/// windows for a moment, which Tauri reports as an exit request; `tray::on_run_event`
/// checks this to keep the app alive through it.
static REBUILDING_WINDOW: AtomicBool = AtomicBool::new(false);

pub fn rebuilding_window() -> bool {
    REBUILDING_WINDOW.load(Ordering::SeqCst)
}

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
    use std::sync::atomic::Ordering;
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    use tauri::{AppHandle, Manager};
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        COREWEBVIEW2_PROCESS_FAILED_KIND, COREWEBVIEW2_PROCESS_FAILED_KIND_BROWSER_PROCESS_EXITED,
        COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_EXITED,
        COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_UNRESPONSIVE,
    };
    use webview2_com::ProcessFailedEventHandler;

    use super::REBUILDING_WINDOW;

    /// Cap on automatic recoveries (reloads and window rebuilds together) inside
    /// `RELOAD_WINDOW`, so a UI that dies right after loading can't spin in a loop.
    const MAX_RELOADS: usize = 3;
    const RELOAD_WINDOW: Duration = Duration::from_secs(10 * 60);

    /// How long to wait for the destroyed window to leave Tauri's window list before
    /// giving up on rebuilding it (its label has to be free to reuse).
    const DESTROY_TIMEOUT: Duration = Duration::from_secs(5);

    /// Keep the exit guard up a little past the rebuild: the "last window closed" exit
    /// request is delivered on the event loop and can arrive after the new window exists.
    const EXIT_GUARD_LINGER: Duration = Duration::from_secs(2);

    static RECENT_RELOADS: Mutex<Vec<Instant>> = Mutex::new(Vec::new());

    pub fn install(app: &AppHandle) -> Result<(), String> {
        let window = app
            .get_webview_window("main")
            .ok_or("main window not found for renderer recovery")?;
        let handle = app.clone();
        window
            .with_webview(move |webview| {
                if let Err(e) = attach(&webview, handle) {
                    crate::db::append_app_log(&format!("renderer recovery attach failed: {e}"));
                }
            })
            .map_err(|e| e.to_string())
    }

    fn attach(webview: &tauri::webview::PlatformWebview, app: AppHandle) -> windows::core::Result<()> {
        let handler = ProcessFailedEventHandler::create(Box::new(move |sender, args| {
            let Some(args) = args else { return Ok(()) };
            let mut kind = COREWEBVIEW2_PROCESS_FAILED_KIND::default();
            unsafe { args.ProcessFailedKind(&mut kind)? };
            if kind == COREWEBVIEW2_PROCESS_FAILED_KIND_BROWSER_PROCESS_EXITED {
                if !allow_reload() {
                    crate::db::append_app_log(&format!(
                        "WebView2 browser process exited; not rebuilding the window ({MAX_RELOADS} recoveries in the last {} min)",
                        RELOAD_WINDOW.as_secs() / 60
                    ));
                    return Ok(());
                }
                crate::db::append_app_log("WebView2 browser process exited; rebuilding the window");
                // Not from inside this callback: it runs on the event loop that destroying
                // and building windows go through, and belongs to the webview being torn down.
                let app = app.clone();
                std::thread::spawn(move || rebuild_main_window(&app));
                return Ok(());
            }
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
                    "WebView2 renderer {reason}; not reloading ({MAX_RELOADS} recoveries in the last {} min)",
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

    fn rebuild_main_window(app: &AppHandle) {
        REBUILDING_WINDOW.store(true, Ordering::SeqCst);
        let result = try_rebuild_main_window(app);
        match &result {
            Ok(()) => crate::db::append_app_log("WebView2 window rebuilt"),
            Err(e) => crate::db::append_app_log(&format!("WebView2 window rebuild failed: {e}")),
        }
        std::thread::sleep(EXIT_GUARD_LINGER);
        REBUILDING_WINDOW.store(false, Ordering::SeqCst);
    }

    fn try_rebuild_main_window(app: &AppHandle) -> Result<(), String> {
        let old = app
            .get_webview_window("main")
            .ok_or("main window not found")?;
        // The native window outlives its dead WebView, so its geometry is still readable.
        let visible = old.is_visible().unwrap_or(true);
        let minimized = old.is_minimized().unwrap_or(false);
        let maximized = old.is_maximized().unwrap_or(false);
        let position = old.outer_position().ok();
        let size = old.inner_size().ok();

        old.destroy().map_err(|e| format!("destroying the old window: {e}"))?;
        let deadline = Instant::now() + DESTROY_TIMEOUT;
        while app.get_webview_window("main").is_some() {
            if Instant::now() >= deadline {
                return Err("old window still registered after destroy".into());
            }
            std::thread::sleep(Duration::from_millis(50));
        }

        let config = app
            .config()
            .app
            .windows
            .iter()
            .find(|w| w.label == "main")
            .cloned()
            .ok_or("no \"main\" window in the app config")?;
        let window = tauri::WebviewWindowBuilder::from_config(app, &config)
            .map_err(|e| e.to_string())?
            .visible(false)
            .build()
            .map_err(|e| format!("building the new window: {e}"))?;
        if let Some(position) = position {
            let _ = window.set_position(position);
        }
        if let Some(size) = size {
            let _ = window.set_size(size);
        }
        if maximized {
            let _ = window.maximize();
        }
        // Stays hidden if it had been minimized to the tray.
        if visible {
            let _ = window.show();
            if minimized {
                let _ = window.minimize();
            }
        }

        // Everything `setup` attached to the old window.
        crate::tray::attach_main_window(app);
        if let Err(e) = crate::app_icon::apply_branding_to(app) {
            crate::db::append_app_log(&format!("WebView2 window rebuild: branding failed: {e}"));
        }
        // SetWindowSubclass only works from the thread that owns the window.
        let handle = app.clone();
        app.run_on_main_thread(move || crate::shutdown_hook::install(&handle))
            .map_err(|e| format!("installing the shutdown hook: {e}"))?;
        super::install(app);
        Ok(())
    }
}
