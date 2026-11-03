//! The "Disable hardware acceleration" option, for PCs where WebView2's GPU rendering makes
//! the whole display stutter while WaveTrace has focus.
//!
//! It's one choice per PC, not per account, because it describes the machine. WebView2
//! refuses to start in a data folder whose running browser process has different flags, and
//! that browser process can briefly outlive a restart, so each mode has its own data folder.

use std::fs;
use std::io::ErrorKind;
use std::path::Path;
use std::sync::OnceLock;

use serde::Serialize;
use tauri::{AppHandle, WebviewWindowBuilder, Wry};

const SOFTWARE_RENDERING_FILE: &str = "software_rendering";
/// Under the app's local data folder, where WebView2 keeps its default data folder too.
#[cfg(windows)]
const SOFTWARE_RENDERING_DATA_DIR: &str = "software-rendering";
/// Setting browser args replaces wry's defaults, so they're repeated here.
#[cfg(windows)]
const SOFTWARE_RENDERING_BROWSER_ARGS: &str =
    "--disable-gpu --disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection";

/// The mode this process started in. Fixed for its lifetime: changing the saved choice only
/// applies on restart, and a window rebuilt after a crash has to stay in the same mode.
static ACTIVE: OnceLock<bool> = OnceLock::new();

#[derive(Debug, Serialize)]
pub struct SoftwareRendering {
    /// Whether this platform has the option at all.
    pub supported: bool,
    /// The choice to apply on the next start.
    pub saved: bool,
    /// The mode this process is running in.
    pub active: bool,
}

pub fn status() -> SoftwareRendering {
    SoftwareRendering {
        supported: cfg!(windows),
        saved: saved(),
        active: active(),
    }
}

pub fn active() -> bool {
    *ACTIVE.get_or_init(saved)
}

fn saved() -> bool {
    cfg!(windows) && read_choice(&crate::accounts::data_root())
}

pub fn save(enabled: bool) -> Result<(), String> {
    if !cfg!(windows) {
        return Err("This option is only available on Windows.".into());
    }
    // Pin the running mode before the saved choice changes under it.
    active();
    write_choice(&crate::accounts::data_root(), enabled).map_err(|e| e.to_string())
}

fn read_choice(root: &Path) -> bool {
    root.join(SOFTWARE_RENDERING_FILE).exists()
}

fn write_choice(root: &Path, enabled: bool) -> std::io::Result<()> {
    let path = root.join(SOFTWARE_RENDERING_FILE);
    if enabled {
        return fs::write(path, "");
    }
    match fs::remove_file(path) {
        Err(e) if e.kind() != ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

/// The main window as configured, in the mode this process started in.
pub fn main_window_builder(
    app: &AppHandle,
) -> Result<WebviewWindowBuilder<'_, Wry, AppHandle>, String> {
    let config = app
        .config()
        .app
        .windows
        .iter()
        .find(|w| w.label == "main")
        .cloned()
        .ok_or("no \"main\" window in the app config")?;
    let builder = WebviewWindowBuilder::from_config(app, &config).map_err(|e| e.to_string())?;
    #[cfg(windows)]
    if active() {
        use tauri::Manager;
        let data_dir = app
            .path()
            .app_local_data_dir()
            .map_err(|e| e.to_string())?
            .join(SOFTWARE_RENDERING_DATA_DIR);
        return Ok(builder
            .additional_browser_args(SOFTWARE_RENDERING_BROWSER_ARGS)
            .data_directory(data_dir));
    }
    Ok(builder)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("wavetrace-render-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn the_choice_is_off_until_saved_and_can_be_saved_twice() {
        let root = temp_root();
        assert!(!read_choice(&root));
        write_choice(&root, true).unwrap();
        write_choice(&root, true).unwrap();
        assert!(read_choice(&root));
        write_choice(&root, false).unwrap();
        assert!(!read_choice(&root));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn turning_it_off_when_already_off_is_fine() {
        let root = temp_root();
        write_choice(&root, false).unwrap();
        assert!(!read_choice(&root));
        fs::remove_dir_all(root).unwrap();
    }
}
