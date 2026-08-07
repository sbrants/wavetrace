//! Pull The Tower `playerInfo.dat` from an Android emulator via host ADB.

use std::collections::hash_map::DefaultHasher;
use std::fs::{self, File};
use std::hash::{Hash, Hasher};
use std::io::{copy, Cursor, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use chrono::Local;
use serde::Serialize;
use zip::ZipArchive;

use crate::db;
use crate::settings::{self, Settings};

const TOWER_ANDROID_PACKAGE: &str = "com.TechTreeGames.TheTower";
const TOWER_LEGACY_ANDROID_PACKAGE: &str = "com.thetowergame";
const TOWER_SAVE_FILENAME: &str = "playerInfo.dat";

const KNOWN_EMULATOR_ADB_HOSTS: &[&str] = &[
    "127.0.0.1:7555",
    "127.0.0.1:16384",
    "127.0.0.1:5555",
    "127.0.0.1:5556",
    "127.0.0.1:5557",
    "127.0.0.1:5558",
    "127.0.0.1:5559",
    "127.0.0.1:21503",
    "127.0.0.1:62001",
];

const ADB_CONNECT_TIMEOUT_MS: u64 = 2_500;
const ADB_PATH_PROBE_TIMEOUT_MS: u64 = 5_000;
const ADB_FIND_SAVE_TIMEOUT_MS: u64 = 12_000;

static DOWNLOAD_LOCK: Mutex<()> = Mutex::new(());
static AUTO_PULL_STARTED: AtomicBool = AtomicBool::new(false);
static LAST_WRITTEN_HASH: Mutex<Option<String>> = Mutex::new(None);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameSaveStatus {
    pub ready: bool,
    pub adb_path: Option<String>,
    pub device_serial: Option<String>,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameSavePullResult {
    pub path: String,
    pub bytes: u64,
    pub remote_path: String,
    pub device_serial: String,
    /// False when content matched the last pulled hash and nothing was written.
    pub written: bool,
    pub content_hash: String,
}

#[derive(Debug, Clone)]
pub struct PullWriteOptions {
    pub custom_port: Option<u32>,
    pub output_dir: PathBuf,
    pub timestamp_filename: bool,
    pub skip_if_same_hash: bool,
}

impl PullWriteOptions {
    pub fn from_settings(s: &Settings) -> Self {
        Self {
            custom_port: s.save_pull_adb_port,
            output_dir: resolve_output_dir(&s.save_pull_dir),
            timestamp_filename: s.save_pull_timestamp_filename,
            skip_if_same_hash: true,
        }
    }
}

fn adb_binary_name() -> &'static str {
    if cfg!(windows) {
        "adb.exe"
    } else {
        "adb"
    }
}

fn app_platform_tools_root() -> PathBuf {
    db::app_data_dir().join("platform-tools")
}

fn app_platform_tools_adb() -> PathBuf {
    // Google zip extracts to platform-tools/adb(.exe)
    app_platform_tools_root()
        .join("platform-tools")
        .join(adb_binary_name())
}

fn resources_platform_tools_adb() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let exe_dir = exe.parent()?;
    let candidates = [
        exe_dir
            .join("resources")
            .join("platform-tools")
            .join(adb_binary_name()),
        exe_dir.join("platform-tools").join(adb_binary_name()),
        // macOS .app Resources
        exe_dir
            .parent()
            .and_then(|p| p.parent())
            .map(|contents| {
                contents
                    .join("Resources")
                    .join("platform-tools")
                    .join(adb_binary_name())
            })
            .unwrap_or_default(),
    ];
    candidates.into_iter().find(|p| !p.as_os_str().is_empty() && p.is_file())
}

fn common_adb_candidates() -> Vec<PathBuf> {
    let mut out = Vec::new();
    let bin = adb_binary_name();
    if let Ok(from_env) = std::env::var("ADB_PATH") {
        let p = PathBuf::from(from_env.trim());
        if p.is_file() {
            out.push(p);
        }
    }
    if let Some(bundled) = resources_platform_tools_adb() {
        out.push(bundled);
    }
    let app_adb = app_platform_tools_adb();
    if app_adb.is_file() {
        out.push(app_adb);
    }
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        let base = PathBuf::from(local);
        out.push(base.join("Android/Sdk/platform-tools").join(bin));
        // WinGet Links shim or package tree
        out.push(base.join("Microsoft/WinGet/Links").join(bin));
        if let Ok(entries) = fs::read_dir(base.join("Microsoft/WinGet/Packages")) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if !name.to_lowercase().contains("platformtools")
                    && !name.to_lowercase().contains("platform-tools")
                {
                    continue;
                }
                out.push(entry.path().join("platform-tools").join(bin));
                out.push(entry.path().join(bin));
            }
        }
    }
    if let Some(home) = dirs::home_dir() {
        out.push(home.join("Library/Android/sdk/platform-tools").join(bin));
        out.push(home.join("Android/Sdk/platform-tools").join(bin));
        out.push(home.join("android-sdk/platform-tools").join(bin));
    }
    out.push(PathBuf::from("/opt/homebrew/bin").join(bin));
    out.push(PathBuf::from("/usr/local/bin").join(bin));
    out.push(PathBuf::from("/usr/bin").join(bin));
    out.push(PathBuf::from(r"C:\Android\platform-tools").join(bin));
    out.push(PathBuf::from(r"C:\platform-tools").join(bin));
    out
}

fn find_adb_on_path() -> Option<PathBuf> {
    let bin = adb_binary_name();
    let which = if cfg!(windows) { "where" } else { "which" };
    let output = Command::new(which)
        .arg(bin.trim_end_matches(".exe"))
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let first = text.lines().next()?.trim();
    if first.is_empty() {
        return None;
    }
    let path = PathBuf::from(first);
    path.is_file().then_some(path)
}

/// Locate an existing adb binary without downloading.
pub fn find_existing_adb() -> Option<PathBuf> {
    for candidate in common_adb_candidates() {
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    find_adb_on_path()
}

fn platform_tools_download_url() -> &'static str {
    if cfg!(windows) {
        "https://dl.google.com/android/repository/platform-tools-latest-windows.zip"
    } else if cfg!(target_os = "macos") {
        "https://dl.google.com/android/repository/platform-tools-latest-darwin.zip"
    } else {
        "https://dl.google.com/android/repository/platform-tools-latest-linux.zip"
    }
}

fn extract_platform_tools_zip(bytes: &[u8], dest_root: &Path) -> Result<PathBuf, String> {
    if dest_root.exists() {
        fs::remove_dir_all(dest_root).map_err(|e| format!("clear platform-tools: {e}"))?;
    }
    fs::create_dir_all(dest_root).map_err(|e| format!("create platform-tools: {e}"))?;

    let mut archive =
        ZipArchive::new(Cursor::new(bytes)).map_err(|e| format!("Invalid platform-tools zip: {e}"))?;
    for i in 0..archive.len() {
        let mut file = archive
            .by_index(i)
            .map_err(|e| format!("zip entry {i}: {e}"))?;
        let Some(rel) = file.enclosed_name().map(|p| p.to_path_buf()) else {
            continue;
        };
        let out_path = dest_root.join(&rel);
        if file.is_dir() {
            fs::create_dir_all(&out_path).map_err(|e| e.to_string())?;
            continue;
        }
        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let mut out = File::create(&out_path).map_err(|e| e.to_string())?;
        copy(&mut file, &mut out).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if rel.file_name().and_then(|n| n.to_str()) == Some("adb") {
                let mut perms = out.metadata().map_err(|e| e.to_string())?.permissions();
                perms.set_mode(0o755);
                fs::set_permissions(&out_path, perms).map_err(|e| e.to_string())?;
            }
        }
    }
    let adb = dest_root.join("platform-tools").join(adb_binary_name());
    if !adb.is_file() {
        return Err("Downloaded platform-tools zip did not contain adb.".into());
    }
    Ok(adb)
}

fn download_platform_tools() -> Result<PathBuf, String> {
    let _guard = DOWNLOAD_LOCK
        .lock()
        .map_err(|_| "platform-tools download lock poisoned".to_string())?;
    // Another caller may have finished while we waited.
    let existing = app_platform_tools_adb();
    if existing.is_file() {
        return Ok(existing);
    }

    let url = platform_tools_download_url();
    let response = ureq::get(url)
        .call()
        .map_err(|e| format!("Failed to download Android platform-tools: {e}"))?;
    let status = response.status().as_u16();
    if !(200..300).contains(&status) {
        return Err(format!(
            "Failed to download Android platform-tools (HTTP {status})."
        ));
    }
    let mut reader = response.into_body().into_reader();
    let mut bytes = Vec::new();
    reader
        .read_to_end(&mut bytes)
        .map_err(|e| format!("Failed reading platform-tools download: {e}"))?;
    if bytes.len() < 1_000 {
        return Err("Downloaded platform-tools archive looks too small.".into());
    }
    extract_platform_tools_zip(&bytes, &app_platform_tools_root())
}

/// Resolve adb, optionally downloading Google platform-tools into app data.
pub fn ensure_adb(allow_download: bool) -> Result<PathBuf, String> {
    if let Some(path) = find_existing_adb() {
        return Ok(path);
    }
    if !allow_download {
        return Err("ADB tools not installed yet.".into());
    }
    download_platform_tools()
}

pub fn build_known_hosts(custom_port: Option<u32>) -> Vec<String> {
    let mut hosts = Vec::new();
    if let Some(port) = custom_port.filter(|p| (1_000..65_536).contains(p)) {
        hosts.push(format!("127.0.0.1:{port}"));
    }
    for host in KNOWN_EMULATOR_ADB_HOSTS {
        if !hosts.iter().any(|h| h == host) {
            hosts.push((*host).to_string());
        }
    }
    hosts
}

pub fn parse_adb_devices(listing: &str) -> Vec<String> {
    let mut serials = Vec::new();
    for line in listing.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("List of devices") {
            continue;
        }
        let mut parts = trimmed.split_whitespace();
        let Some(serial) = parts.next() else {
            continue;
        };
        let Some(state) = parts.next() else {
            continue;
        };
        if state == "device" {
            serials.push(serial.to_string());
        }
    }
    serials
}

pub fn order_serials_for_pull(serials: &[String], host_priority: &[String]) -> Vec<String> {
    let mut ordered = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for host in host_priority {
        if serials.iter().any(|s| s == host) && seen.insert(host.clone()) {
            ordered.push(host.clone());
        }
    }
    for serial in serials {
        if seen.insert(serial.clone()) {
            ordered.push(serial.clone());
        }
    }
    ordered
}

pub fn bytes_look_like_shell_failure(bytes: &[u8]) -> bool {
    if bytes.is_empty() {
        return true;
    }
    let sample = String::from_utf8_lossy(&bytes[..bytes.len().min(512)]);
    sample.lines().any(|l| l.starts_with("cat: ") || l.starts_with("adb: "))
        || sample.to_lowercase().contains("no such file")
        || sample.to_lowercase().contains("permission denied")
        || sample.to_lowercase().contains("cannot open")
        || sample.to_lowercase().contains("is a directory")
}

/// True when bytes are plausibly playerInfo.dat (often gzip on disk), not an adb error string.
pub fn bytes_look_like_player_info_save(bytes: &[u8]) -> bool {
    if bytes.len() < 32 || bytes_look_like_shell_failure(bytes) {
        return false;
    }
    if bytes[0] == 0x1f && bytes[1] == 0x8b {
        return true;
    }
    bytes.len() >= 64
}

pub fn emulator_pull_paths() -> Vec<String> {
    let packages = [TOWER_ANDROID_PACKAGE, TOWER_LEGACY_ANDROID_PACKAGE];
    let mut paths = Vec::new();
    let mut add = |p: String| {
        if !paths.contains(&p) {
            paths.push(p);
        }
    };
    for pkg in packages {
        for root in ["/storage/emulated/0", "/sdcard", "/mnt/sdcard"] {
            add(format!("{root}/Android/data/{pkg}/files/{TOWER_SAVE_FILENAME}"));
            add(format!("{root}/Android/data/{pkg}/files/PlayerInfo.dat"));
        }
        add(format!("/data/data/{pkg}/files/{TOWER_SAVE_FILENAME}"));
    }
    for root in ["/storage/emulated/0", "/sdcard"] {
        add(format!("{root}/Download/{TOWER_SAVE_FILENAME}"));
        add(format!("{root}/Downloads/{TOWER_SAVE_FILENAME}"));
        add(format!("{root}/Download/PlayerInfo.dat"));
        add(format!("{root}/Downloads/PlayerInfo.dat"));
    }
    paths
}

fn configure_command(cmd: &mut Command) {
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
}

fn run_adb(
    adb: &Path,
    serial: Option<&str>,
    args: &[&str],
    timeout: Duration,
) -> Result<Vec<u8>, String> {
    let mut cmd = Command::new(adb);
    configure_command(&mut cmd);
    if let Some(serial) = serial {
        cmd.args(["-s", serial]);
    }
    cmd.args(args);
    let started = Instant::now();
    let mut child = cmd.spawn().map_err(|e| format!("Failed to start adb: {e}"))?;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let mut stdout = Vec::new();
                if let Some(mut out) = child.stdout.take() {
                    out.read_to_end(&mut stdout).ok();
                }
                let mut stderr = Vec::new();
                if let Some(mut err) = child.stderr.take() {
                    err.read_to_end(&mut stderr).ok();
                }
                if !status.success() {
                    let err = String::from_utf8_lossy(&stderr);
                    let out = String::from_utf8_lossy(&stdout);
                    let detail = if !err.trim().is_empty() {
                        err.trim().to_string()
                    } else {
                        out.trim().to_string()
                    };
                    return Err(if detail.is_empty() {
                        format!("adb {:?} failed", args)
                    } else {
                        detail
                    });
                }
                return Ok(stdout);
            }
            Ok(None) => {
                if started.elapsed() > timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!("adb {:?} timed out", args));
                }
                std::thread::sleep(Duration::from_millis(40));
            }
            Err(e) => return Err(format!("adb wait failed: {e}")),
        }
    }
}

fn run_adb_text(
    adb: &Path,
    serial: Option<&str>,
    args: &[&str],
    timeout: Duration,
) -> Result<String, String> {
    let bytes = run_adb(adb, serial, args, timeout)?;
    Ok(String::from_utf8_lossy(&bytes).trim().to_string())
}

fn ensure_adb_server(adb: &Path) {
    let _ = run_adb(
        adb,
        None,
        &["start-server"],
        Duration::from_secs(20),
    );
}

fn connect_host(adb: &Path, host: &str) {
    let _ = run_adb(
        adb,
        None,
        &["connect", host],
        Duration::from_millis(ADB_CONNECT_TIMEOUT_MS),
    );
}

fn list_online_serials(adb: &Path) -> Result<Vec<String>, String> {
    let listing = run_adb_text(adb, None, &["devices"], Duration::from_secs(15))?;
    Ok(parse_adb_devices(&listing))
}

fn connect_and_list(adb: &Path, custom_port: Option<u32>) -> Result<(Vec<String>, Vec<String>), String> {
    ensure_adb_server(adb);
    let hosts = build_known_hosts(custom_port);
    let mut serials = list_online_serials(adb)?;
    // Always try known hosts (custom port first) so a cold emulator becomes visible.
    if serials.is_empty() || custom_port.is_some() {
        for host in &hosts {
            connect_host(adb, host);
        }
        serials = list_online_serials(adb)?;
    }
    let ordered = order_serials_for_pull(&serials, &hosts);
    Ok((ordered, hosts))
}

/// Probe whether ADB and an emulator device are available.
pub fn probe_game_save(custom_port: Option<u32>, allow_download: bool) -> GameSaveStatus {
    let adb = match ensure_adb(allow_download) {
        Ok(p) => p,
        Err(e) => {
            return GameSaveStatus {
                ready: false,
                adb_path: None,
                device_serial: None,
                detail: e,
            };
        }
    };
    match connect_and_list(&adb, custom_port) {
        Ok((serials, _)) if !serials.is_empty() => GameSaveStatus {
            ready: true,
            adb_path: Some(adb.display().to_string()),
            device_serial: Some(serials[0].clone()),
            detail: format!("Emulator ready · {}", serials[0]),
        },
        Ok(_) => GameSaveStatus {
            ready: false,
            adb_path: Some(adb.display().to_string()),
            device_serial: None,
            detail: "No emulator via ADB".into(),
        },
        Err(e) => GameSaveStatus {
            ready: false,
            adb_path: Some(adb.display().to_string()),
            device_serial: None,
            detail: e,
        },
    }
}

fn remote_path_exists(adb: &Path, serial: &str, remote_path: &str) -> bool {
    run_adb_text(
        adb,
        Some(serial),
        &["shell", "test", "-f", remote_path, "&&", "echo", "ok"],
        Duration::from_millis(ADB_PATH_PROBE_TIMEOUT_MS),
    )
    .map(|out| out.contains("ok"))
    .unwrap_or(false)
}

fn pull_remote_path(adb: &Path, serial: &str, remote_path: &str) -> Result<Vec<u8>, String> {
    let tmp_dir = std::env::temp_dir().join(format!("wavetrace-adb-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&tmp_dir).map_err(|e| e.to_string())?;
    let local = tmp_dir.join(TOWER_SAVE_FILENAME);
    let result = (|| {
        run_adb(
            adb,
            Some(serial),
            &[
                "pull",
                remote_path,
                local.to_str().ok_or("invalid temp path")?,
            ],
            Duration::from_secs(120),
        )?;
        let bytes = fs::read(&local).map_err(|e| e.to_string())?;
        if bytes.is_empty() {
            return Err("Pulled save file is empty.".into());
        }
        Ok(bytes)
    })();
    let _ = fs::remove_dir_all(&tmp_dir);
    result
}

fn read_via_shell_cat(adb: &Path, serial: &str, remote_path: &str) -> Option<Vec<u8>> {
    let bytes = run_adb(
        adb,
        Some(serial),
        &["exec-out", "cat", remote_path],
        Duration::from_secs(60),
    )
    .ok()?;
    if !bytes.is_empty() && bytes_look_like_player_info_save(&bytes) {
        Some(bytes)
    } else {
        None
    }
}

fn try_pull_path(adb: &Path, serial: &str, remote_path: &str) -> Option<(String, Vec<u8>)> {
    if !remote_path_exists(adb, serial, remote_path) {
        return None;
    }
    if let Ok(bytes) = pull_remote_path(adb, serial, remote_path) {
        if bytes_look_like_player_info_save(&bytes) {
            return Some((remote_path.to_string(), bytes));
        }
    }
    read_via_shell_cat(adb, serial, remote_path).map(|b| (remote_path.to_string(), b))
}

fn pull_via_run_as(adb: &Path, serial: &str) -> Option<(String, Vec<u8>)> {
    for pkg in [TOWER_ANDROID_PACKAGE, TOWER_LEGACY_ANDROID_PACKAGE] {
        for rel in [format!("files/{TOWER_SAVE_FILENAME}"), "files/PlayerInfo.dat".into()] {
            let bytes = run_adb(
                adb,
                Some(serial),
                &["exec-out", "run-as", pkg, "cat", &rel],
                Duration::from_secs(60),
            )
            .ok();
            if let Some(bytes) = bytes {
                if bytes_look_like_player_info_save(&bytes) {
                    return Some((format!("run-as:{pkg}/{rel}"), bytes));
                }
            }
        }
    }
    None
}

fn find_player_info_paths(adb: &Path, serial: &str) -> Vec<String> {
    let script =
        "find /sdcard /storage/emulated/0 -maxdepth 8 -iname playerinfo.dat 2>/dev/null | head -n 6";
    run_adb_text(
        adb,
        Some(serial),
        &["shell", script],
        Duration::from_millis(ADB_FIND_SAVE_TIMEOUT_MS),
    )
    .map(|out| {
        out.lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(|s| s.to_string())
            .collect()
    })
    .unwrap_or_default()
}

fn discover_and_pull_on_serial(adb: &Path, serial: &str) -> Option<(String, Vec<u8>)> {
    for path in emulator_pull_paths() {
        if let Some(pulled) = try_pull_path(adb, serial, &path) {
            return Some(pulled);
        }
    }
    if let Some(pulled) = pull_via_run_as(adb, serial) {
        return Some(pulled);
    }
    for path in find_player_info_paths(adb, serial) {
        if let Some(pulled) = try_pull_path(adb, serial, &path) {
            return Some(pulled);
        }
    }
    None
}

fn downloads_dir() -> PathBuf {
    dirs::download_dir().unwrap_or_else(|| db::app_data_dir().join("exports"))
}

/// Empty/whitespace `save_pull_dir` → Downloads; otherwise the configured folder.
pub fn resolve_output_dir(configured: &str) -> PathBuf {
    let trimmed = configured.trim();
    if trimmed.is_empty() {
        downloads_dir()
    } else {
        PathBuf::from(trimmed)
    }
}

pub fn content_hash(bytes: &[u8]) -> String {
    let mut hasher = DefaultHasher::new();
    bytes.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

fn load_persisted_hash() -> Option<String> {
    if let Ok(guard) = LAST_WRITTEN_HASH.lock() {
        if guard.is_some() {
            return guard.clone();
        }
    }
    let conn = db::open().ok()?;
    let v = db::get_setting(&conn, "save_pull_last_hash").ok()??;
    let trimmed = v.trim().to_string();
    if trimmed.is_empty() {
        return None;
    }
    if let Ok(mut guard) = LAST_WRITTEN_HASH.lock() {
        *guard = Some(trimmed.clone());
    }
    Some(trimmed)
}

fn persist_hash(hash: &str) {
    if let Ok(mut guard) = LAST_WRITTEN_HASH.lock() {
        *guard = Some(hash.to_string());
    }
    if let Ok(conn) = db::open() {
        let _ = db::set_setting(&conn, "save_pull_last_hash", hash);
    }
}

/// Destination path for a pull. Always overwrites if the path already exists.
pub fn output_file_path(dir: &Path, timestamp_filename: bool) -> PathBuf {
    let _ = fs::create_dir_all(dir);
    if timestamp_filename {
        let stamp = Local::now().format("%Y%m%d-%H%M%S");
        dir.join(format!("playerInfo-{stamp}.dat"))
    } else {
        dir.join(TOWER_SAVE_FILENAME)
    }
}

/// Pull playerInfo.dat and write according to options. Skips write when hash unchanged
/// and `skip_if_same_hash` is set.
pub fn pull_game_save_with_options(
    opts: &PullWriteOptions,
) -> Result<GameSavePullResult, String> {
    let adb = ensure_adb(true)?;
    let (serials, _) = connect_and_list(&adb, opts.custom_port)?;
    if serials.is_empty() {
        return Err(
            "No supported emulator detected. Start your emulator with ADB debugging, then try again."
                .into(),
        );
    }
    let mut last_serial = serials[0].clone();
    for serial in &serials {
        last_serial = serial.clone();
        if let Some((remote_path, bytes)) = discover_and_pull_on_serial(&adb, serial) {
            let hash = content_hash(&bytes);
            if opts.skip_if_same_hash {
                if let Some(prev) = load_persisted_hash() {
                    if prev == hash {
                        let path = output_file_path(&opts.output_dir, opts.timestamp_filename);
                        return Ok(GameSavePullResult {
                            path: path.display().to_string(),
                            bytes: bytes.len() as u64,
                            remote_path,
                            device_serial: serial.clone(),
                            written: false,
                            content_hash: hash,
                        });
                    }
                }
            }
            let path = output_file_path(&opts.output_dir, opts.timestamp_filename);
            {
                let mut file = File::create(&path).map_err(|e| e.to_string())?;
                file.write_all(&bytes).map_err(|e| e.to_string())?;
            }
            persist_hash(&hash);
            return Ok(GameSavePullResult {
                path: path.display().to_string(),
                bytes: bytes.len() as u64,
                remote_path,
                device_serial: serial.clone(),
                written: true,
                content_hash: hash,
            });
        }
    }
    Err(format!(
        "Save file '{TOWER_SAVE_FILENAME}' not found via ADB on {last_serial}. Open The Tower, save your progress, then try again."
    ))
}

/// Manual / command pull using current settings (hash skip so repeat clicks don't rewrite identical files).
pub fn pull_game_save_from_settings() -> Result<GameSavePullResult, String> {
    let s = settings::load(&db::open().map_err(|e| e.to_string())?);
    let mut opts = PullWriteOptions::from_settings(&s);
    opts.skip_if_same_hash = true;
    pull_game_save_with_options(&opts)
}

/// Native folder picker for the save output directory.
pub fn pick_output_dir() -> Result<Option<String>, String> {
    let picked = rfd::FileDialog::new()
        .set_title("Choose folder for playerInfo.dat")
        .pick_folder();
    Ok(picked.map(|p| p.display().to_string()))
}

/// Spawn a background loop that auto-pulls when enabled in settings.
pub fn ensure_auto_pull_loop() {
    if AUTO_PULL_STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    std::thread::Builder::new()
        .name("wavetrace-save-auto-pull".into())
        .spawn(|| {
            let mut last_attempt = Instant::now()
                .checked_sub(Duration::from_secs(3600))
                .unwrap_or_else(Instant::now);
            loop {
                std::thread::sleep(Duration::from_secs(5));
                let Ok(conn) = db::open() else {
                    continue;
                };
                let s = settings::load(&conn);
                if !s.save_pull_enabled || !s.save_pull_auto {
                    continue;
                }
                let interval = Duration::from_secs(u64::from(s.save_pull_auto_interval_secs.max(15)));
                if last_attempt.elapsed() < interval {
                    continue;
                }
                last_attempt = Instant::now();
                let opts = PullWriteOptions::from_settings(&s);
                match pull_game_save_with_options(&opts) {
                    Ok(result) if result.written => {
                        db::append_app_log(&format!(
                            "Auto-pulled game save ({} bytes) → {}",
                            result.bytes, result.path
                        ));
                    }
                    _ => {}
                }
            }
        })
        .ok();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_adb_devices_online_only() {
        let listing = "\
List of devices attached
127.0.0.1:62001\tdevice
emulator-5554\toffline
R58M123\tunauthorized
";
        assert_eq!(parse_adb_devices(listing), vec!["127.0.0.1:62001".to_string()]);
    }

    #[test]
    fn build_known_hosts_puts_custom_first() {
        let hosts = build_known_hosts(Some(62001));
        assert_eq!(hosts[0], "127.0.0.1:62001");
        assert!(hosts.contains(&"127.0.0.1:5555".to_string()));
    }

    #[test]
    fn order_serials_prefers_host_priority() {
        let serials = vec![
            "usb-phone".into(),
            "127.0.0.1:5555".into(),
            "127.0.0.1:62001".into(),
        ];
        let priority = vec!["127.0.0.1:62001".into(), "127.0.0.1:5555".into()];
        assert_eq!(
            order_serials_for_pull(&serials, &priority),
            vec![
                "127.0.0.1:62001".to_string(),
                "127.0.0.1:5555".to_string(),
                "usb-phone".to_string(),
            ]
        );
    }

    #[test]
    fn gzip_magic_accepted_as_save() {
        let mut bytes = vec![0x1f, 0x8b, 0x08];
        bytes.resize(64, 0);
        assert!(bytes_look_like_player_info_save(&bytes));
    }

    #[test]
    fn shell_failure_rejected() {
        let bytes = b"cat: /data/x: Permission denied\n";
        assert!(bytes_look_like_shell_failure(bytes));
        assert!(!bytes_look_like_player_info_save(bytes));
    }

    #[test]
    fn emulator_pull_paths_include_official_package() {
        let paths = emulator_pull_paths();
        assert!(paths.iter().any(|p| p.contains(TOWER_ANDROID_PACKAGE)
            && p.ends_with(TOWER_SAVE_FILENAME)));
    }

    #[test]
    fn find_existing_adb_does_not_panic() {
        let _ = find_existing_adb();
    }

    #[test]
    fn resolve_output_dir_defaults_to_downloads_when_empty() {
        let p = resolve_output_dir("  ");
        assert_eq!(p, downloads_dir());
        let custom = resolve_output_dir(r"C:\Saves\Tower");
        assert_eq!(custom, PathBuf::from(r"C:\Saves\Tower"));
    }

    #[test]
    fn content_hash_stable_for_same_bytes() {
        assert_eq!(content_hash(b"abc"), content_hash(b"abc"));
        assert_ne!(content_hash(b"abc"), content_hash(b"abd"));
    }

    #[test]
    fn output_file_path_overwrite_vs_timestamp() {
        let dir = std::env::temp_dir().join("wavetrace-save-path-test");
        let plain = output_file_path(&dir, false);
        assert!(plain.ends_with(TOWER_SAVE_FILENAME));
        let stamped = output_file_path(&dir, true);
        let name = stamped.file_name().unwrap().to_string_lossy();
        assert!(name.starts_with("playerInfo-"));
        assert!(name.ends_with(".dat"));
        let _ = fs::remove_dir_all(&dir);
    }
}
