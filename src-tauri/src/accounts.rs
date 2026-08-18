//! Named accounts: isolated settings/DB per Tower login, so several
//! instances can monitor in parallel.

use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

pub const DEFAULT_ACCOUNT_ID: &str = "default";
const DEFAULT_ACCOUNT_NAME: &str = "Default";
pub const DEFAULT_COLOR: &str = "#4cc2ff";
const REGISTRY_FILE: &str = "accounts.json";
const ACTIVE_FILE: &str = "active-account.txt";
const LOCK_FILE: &str = "instance.lock";
const MAX_ACCOUNTS: usize = 16;
const MAX_NAME_CHARS: usize = 40;

pub const COLOR_PALETTE: &[&str] = &[
    "#4cc2ff", "#34d399", "#fbbf24", "#f472b6", "#a78bfa", "#fb923c", "#2dd4bf",
    "#f87171",
];

static ACTIVE_ID: Mutex<Option<String>> = Mutex::new(None);
static INSTANCE_LOCK: Mutex<Option<File>> = Mutex::new(None);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Account {
    pub id: String,
    pub name: String,
    pub color: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Registry {
    #[serde(default = "registry_version")]
    version: u32,
    accounts: Vec<Account>,
}

fn registry_version() -> u32 {
    1
}

impl Default for Registry {
    fn default() -> Self {
        Self {
            version: 1,
            accounts: vec![default_account()],
        }
    }
}

fn default_account() -> Account {
    Account {
        id: DEFAULT_ACCOUNT_ID.into(),
        name: DEFAULT_ACCOUNT_NAME.into(),
        color: DEFAULT_COLOR.into(),
    }
}

/// Shared WaveTrace folder (registry, ADB tools, Default account data).
pub fn data_root() -> PathBuf {
    let base = dirs::data_dir().unwrap_or_else(|| PathBuf::from("."));
    let new_dir = base.join("wavetrace");
    if !new_dir.exists() {
        for legacy in ["wavewatch", "towerrun"] {
            let old_dir = base.join(legacy);
            if old_dir.exists() {
                if fs::rename(&old_dir, &new_dir).is_err() {
                    fs::create_dir_all(&old_dir).ok();
                    return old_dir;
                }
                break;
            }
        }
    }
    fs::create_dir_all(&new_dir).ok();
    new_dir
}

/// Data directory for the account this process is bound to.
pub fn active_data_dir() -> PathBuf {
    let root = data_root();
    match current_id() {
        Some(id) => account_data_dir(&root, &id),
        None => root,
    }
}

pub fn account_data_dir(root: &Path, id: &str) -> PathBuf {
    if id == DEFAULT_ACCOUNT_ID {
        root.to_path_buf()
    } else {
        root.join("accounts").join(sanitize_id(id))
    }
}

pub fn current_id() -> Option<String> {
    ACTIVE_ID.lock().ok().and_then(|g| g.clone())
}

pub fn current_account() -> Account {
    let Some(id) = current_id() else {
        return default_account();
    };
    load_or_init_registry(&data_root())
        .accounts
        .into_iter()
        .find(|a| a.id == id)
        .unwrap_or_else(default_account)
}

/// Bind this process to an account (CLI / last-used) and take its instance lock.
pub fn init() -> Result<Account, String> {
    let root = data_root();
    let registry = load_or_init_registry(&root);
    let requested = requested_account_id();
    let id = resolve_requested_id(&registry, requested.as_deref())?;
    let account = registry
        .accounts
        .iter()
        .find(|a| a.id == id)
        .cloned()
        .ok_or_else(|| format!("Unknown account '{id}'."))?;

    let dir = account_data_dir(&root, &id);
    fs::create_dir_all(&dir).map_err(|e| format!("Could not create account folder: {e}"))?;
    acquire_lock(&dir)?;

    if requested.is_none() {
        write_last_used(&root, &id);
    }

    *ACTIVE_ID.lock().map_err(|e| e.to_string())? = Some(id);
    Ok(account)
}

fn requested_account_id() -> Option<String> {
    if let Ok(env) = std::env::var("WAVETRACE_ACCOUNT") {
        let t = env.trim();
        if !t.is_empty() {
            return Some(t.to_string());
        }
    }
    let args: Vec<String> = std::env::args().collect();
    parse_account_arg(&args)
}

fn parse_account_arg(args: &[String]) -> Option<String> {
    let mut i = 1;
    while i < args.len() {
        let a = &args[i];
        if a == "--account" || a == "-a" {
            return args.get(i + 1).cloned();
        }
        if let Some(rest) = extra_arg_value(a, "--account=") {
            return Some(rest.to_string());
        }
        i += 1;
    }
    None
}

fn extra_arg_value<'a>(arg: &'a str, prefix: &str) -> Option<&'a str> {
    arg.strip_prefix(prefix).filter(|s| !s.is_empty())
}

fn resolve_requested_id(registry: &Registry, requested: Option<&str>) -> Result<String, String> {
    let root = data_root();
    if let Some(id) = requested {
        let id = sanitize_id(id);
        if registry.accounts.iter().any(|a| a.id == id) {
            return Ok(id);
        }
        return Err(format!(
            "Account '{id}' does not exist. Create it in Settings, or pick another from the list."
        ));
    }
    let last = read_last_used(&root);
    if let Some(id) = last {
        if registry.accounts.iter().any(|a| a.id == id) {
            return Ok(id);
        }
    }
    Ok(DEFAULT_ACCOUNT_ID.into())
}

pub fn list_accounts() -> Vec<Account> {
    load_or_init_registry(&data_root()).accounts
}

pub fn create_account(name: &str, color: Option<&str>) -> Result<Account, String> {
    let root = data_root();
    let mut registry = load_or_init_registry(&root);
    if registry.accounts.len() >= MAX_ACCOUNTS {
        return Err(format!("At most {MAX_ACCOUNTS} accounts."));
    }
    let name = normalize_name(name)?;
    ensure_unique_name(&registry, &name, None)?;
    let color = normalize_color(color.unwrap_or(next_palette_color(&registry)))?;
    let id = unique_id(&registry, &slugify(&name));
    let account = Account {
        id: id.clone(),
        name,
        color,
    };
    let dir = account_data_dir(&root, &id);
    fs::create_dir_all(&dir).map_err(|e| format!("Could not create account folder: {e}"))?;
    registry.accounts.push(account.clone());
    save_registry(&root, &registry)?;
    Ok(account)
}

pub fn update_account(id: &str, name: Option<&str>, color: Option<&str>) -> Result<Account, String> {
    let root = data_root();
    let mut registry = load_or_init_registry(&root);
    let idx = registry
        .accounts
        .iter()
        .position(|a| a.id == id)
        .ok_or_else(|| format!("Unknown account '{id}'."))?;
    if let Some(name) = name {
        let name = normalize_name(name)?;
        ensure_unique_name(&registry, &name, Some(id))?;
        registry.accounts[idx].name = name;
    }
    if let Some(color) = color {
        registry.accounts[idx].color = normalize_color(color)?;
    }
    let updated = registry.accounts[idx].clone();
    save_registry(&root, &registry)?;
    Ok(updated)
}

pub fn delete_account(id: &str) -> Result<(), String> {
    let root = data_root();
    let mut registry = load_or_init_registry(&root);
    if registry.accounts.len() <= 1 {
        return Err("Keep at least one account.".into());
    }
    if current_id().as_deref() == Some(id) {
        return Err("Can't delete the account this window is using. Switch first.".into());
    }
    let Some(pos) = registry.accounts.iter().position(|a| a.id == id) else {
        return Err(format!("Unknown account '{id}'."));
    };
    let dir = account_data_dir(&root, id);
    if dir != root && dir.exists() {
        fs::remove_dir_all(&dir).map_err(|e| format!("Could not remove account data: {e}"))?;
    } else if dir == root {
        // Default lives at the shared root — only drop this account's DB/logs, not
        // the registry or other accounts' folders.
        for name in ["wavetrace.db", "wavewatch.db", "towerrun.db"] {
            let _ = fs::remove_file(root.join(name));
        }
        let _ = fs::remove_dir_all(root.join("logs"));
        let _ = fs::remove_dir_all(root.join("backups"));
        let _ = fs::remove_file(root.join(LOCK_FILE));
    }
    registry.accounts.remove(pos);
    save_registry(&root, &registry)?;
    if read_last_used(&root).as_deref() == Some(id) {
        if let Some(first) = registry.accounts.first() {
            write_last_used(&root, &first.id);
        }
    }
    Ok(())
}

/// Spawn another WaveTrace process bound to `id` without closing this one.
pub fn spawn_instance(id: &str) -> Result<(), String> {
    let root = data_root();
    let registry = load_or_init_registry(&root);
    if !registry.accounts.iter().any(|a| a.id == id) {
        return Err(format!("Unknown account '{id}'."));
    }
    if current_id().as_deref() == Some(id) {
        return Err("This window is already that account.".into());
    }
    spawn_with_account(id)
}

pub fn remember_last_used(id: &str) {
    write_last_used(&data_root(), id);
}

pub fn spawn_and_replace(id: &str) -> Result<(), String> {
    let root = data_root();
    let registry = load_or_init_registry(&root);
    if !registry.accounts.iter().any(|a| a.id == id) {
        return Err(format!("Unknown account '{id}'."));
    }
    write_last_used(&root, id);
    release_lock();
    if let Err(e) = spawn_with_account(id) {
        let _ = acquire_lock(&active_data_dir());
        return Err(e);
    }
    Ok(())
}

fn release_lock() {
    if let Ok(mut slot) = INSTANCE_LOCK.lock() {
        *slot = None;
    }
}

fn spawn_with_account(id: &str) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| format!("Could not find WaveTrace: {e}"))?;
    let mut cmd = std::process::Command::new(exe);
    cmd.arg("--account").arg(id);
    cmd.env("WAVETRACE_ACCOUNT", id);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const NEW_GROUP: u32 = 0x00000200;
        cmd.creation_flags(NEW_GROUP);
    }
    cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Could not start another window: {e}"))
}

fn load_or_init_registry(root: &Path) -> Registry {
    let path = root.join(REGISTRY_FILE);
    if let Ok(text) = fs::read_to_string(&path) {
        if let Ok(mut reg) = serde_json::from_str::<Registry>(&text) {
            if reg.accounts.is_empty() {
                reg.accounts.push(default_account());
            }
            return reg;
        }
    }
    let registry = Registry::default();
    let _ = save_registry(root, &registry);
    registry
}

fn save_registry(root: &Path, registry: &Registry) -> Result<(), String> {
    fs::create_dir_all(root).map_err(|e| e.to_string())?;
    let path = root.join(REGISTRY_FILE);
    let json = serde_json::to_string_pretty(registry).map_err(|e| e.to_string())?;
    fs::write(path, json).map_err(|e| format!("Could not save accounts: {e}"))
}

fn read_last_used(root: &Path) -> Option<String> {
    fs::read_to_string(root.join(ACTIVE_FILE))
        .ok()
        .map(|s| sanitize_id(s.trim()))
        .filter(|s| !s.is_empty())
}

fn write_last_used(root: &Path, id: &str) {
    let _ = fs::write(root.join(ACTIVE_FILE), id);
}

fn acquire_lock(dir: &Path) -> Result<(), String> {
    let path = dir.join(LOCK_FILE);
    let file = lock_file(&path)?;
    *INSTANCE_LOCK.lock().map_err(|e| e.to_string())? = Some(file);
    Ok(())
}

#[cfg(windows)]
fn lock_file(path: &Path) -> Result<File, String> {
    use std::os::windows::fs::OpenOptionsExt;
    OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .share_mode(0)
        .open(path)
        .map_err(|e| {
            if e.raw_os_error() == Some(32) {
                "This account is already open in another WaveTrace window.".into()
            } else {
                format!("Could not claim this account: {e}")
            }
        })
}

#[cfg(unix)]
fn lock_file(path: &Path) -> Result<File, String> {
    use std::os::unix::io::AsRawFd;
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .open(path)
        .map_err(|e| format!("Could not claim this account: {e}"))?;
    let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if rc != 0 {
        return Err("This account is already open in another WaveTrace window.".into());
    }
    Ok(file)
}

#[cfg(not(any(windows, unix)))]
fn lock_file(path: &Path) -> Result<File, String> {
    OpenOptions::new()
        .write(true)
        .create(true)
        .open(path)
        .map_err(|e| format!("Could not claim this account: {e}"))
}

pub fn parse_hex_color(color: &str) -> Option<[u8; 4]> {
    let c = color.trim();
    if c.len() != 7 || !c.starts_with('#') {
        return None;
    }
    let r = u8::from_str_radix(&c[1..3], 16).ok()?;
    let g = u8::from_str_radix(&c[3..5], 16).ok()?;
    let b = u8::from_str_radix(&c[5..7], 16).ok()?;
    Some([r, g, b, 255])
}

pub fn normalize_color(color: &str) -> Result<String, String> {
    let c = color.trim();
    if parse_hex_color(c).is_none() {
        return Err("Color must be #RRGGBB.".into());
    }
    Ok(c.to_lowercase())
}

fn normalize_name(name: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Give the account a name.".into());
    }
    if name.chars().count() > MAX_NAME_CHARS {
        return Err(format!("Name is limited to {MAX_NAME_CHARS} characters."));
    }
    Ok(name.to_string())
}

fn ensure_unique_name(registry: &Registry, name: &str, except_id: Option<&str>) -> Result<(), String> {
    let lower = name.to_lowercase();
    if registry
        .accounts
        .iter()
        .any(|a| except_id != Some(a.id.as_str()) && a.name.to_lowercase() == lower)
    {
        return Err(format!("An account named '{name}' already exists."));
    }
    Ok(())
}

fn next_palette_color(registry: &Registry) -> &'static str {
    let used: Vec<String> = registry.accounts.iter().map(|a| a.color.to_lowercase()).collect();
    COLOR_PALETTE
        .iter()
        .copied()
        .find(|c| !used.iter().any(|u| u == c))
        .unwrap_or(COLOR_PALETTE[registry.accounts.len() % COLOR_PALETTE.len()])
}

fn unique_id(registry: &Registry, base: &str) -> String {
    if !registry.accounts.iter().any(|a| a.id == base) {
        return base.to_string();
    }
    for n in 2..1000 {
        let candidate = format!("{base}-{n}");
        if !registry.accounts.iter().any(|a| a.id == candidate) {
            return candidate;
        }
    }
    format!("{base}-{}", uuid::Uuid::new_v4().simple())
}

pub fn slugify(name: &str) -> String {
    let mut out = String::new();
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if (ch == ' ' || ch == '-' || ch == '_')
            && !out.ends_with('-') && !out.is_empty() {
                out.push('-');
            }
    }
    let slug = out.trim_matches('-').to_string();
    if slug.is_empty() {
        DEFAULT_ACCOUNT_ID.into()
    } else {
        slug
    }
}

fn sanitize_id(id: &str) -> String {
    slugify(id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex as StdMutex;

    static TEST_LOCK: StdMutex<()> = StdMutex::new(());

    fn temp_root() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("wavetrace-acct-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn default_account_uses_root_not_a_subdir() {
        let root = temp_root();
        assert_eq!(account_data_dir(&root, DEFAULT_ACCOUNT_ID), root);
        assert_eq!(
            account_data_dir(&root, "farm"),
            root.join("accounts").join("farm")
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn slugify_collapses_punctuation() {
        assert_eq!(slugify("Farm Alt"), "farm-alt");
        assert_eq!(slugify("  "), DEFAULT_ACCOUNT_ID);
        assert_eq!(slugify("T18!!!"), "t18");
    }

    #[test]
    fn palette_and_hex_validation() {
        assert!(normalize_color("#4CC2FF").is_ok());
        assert!(normalize_color("blue").is_err());
        assert_eq!(parse_hex_color("#ff00aa"), Some([255, 0, 170, 255]));
    }

    #[test]
    fn parse_account_cli() {
        let args = vec![
            "wavetrace".into(),
            "--account".into(),
            "farm".into(),
        ];
        assert_eq!(parse_account_arg(&args).as_deref(), Some("farm"));
        let args = vec!["wavetrace".into(), "--account=alt".into()];
        assert_eq!(parse_account_arg(&args).as_deref(), Some("alt"));
        assert!(parse_account_arg(&["wavetrace".into()]).is_none());
    }

    #[test]
    fn registry_round_trip_in_temp() {
        let _g = TEST_LOCK.lock().unwrap();
        let root = temp_root();
        let mut reg = Registry::default();
        reg.accounts.push(Account {
            id: "farm".into(),
            name: "Farm".into(),
            color: "#34d399".into(),
        });
        save_registry(&root, &reg).unwrap();
        let loaded = load_or_init_registry(&root);
        assert_eq!(loaded.accounts.len(), 2);
        assert_eq!(loaded.accounts[1].name, "Farm");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn lock_file_is_exclusive() {
        let root = temp_root();
        let a = lock_file(&root.join(LOCK_FILE)).expect("first lock");
        assert!(lock_file(&root.join(LOCK_FILE)).is_err());
        drop(a);
        assert!(lock_file(&root.join(LOCK_FILE)).is_ok());
        let _ = fs::remove_dir_all(root);
    }
}
