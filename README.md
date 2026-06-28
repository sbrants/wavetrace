# WaveTrace

[![CI](https://github.com/sbrants/wavetrace/actions/workflows/ci.yml/badge.svg)](https://github.com/sbrants/wavetrace/actions/workflows/ci.yml)
[![Release](https://github.com/sbrants/wavetrace/actions/workflows/release.yml/badge.svg)](https://github.com/sbrants/wavetrace/actions/workflows/release.yml)
[![License: MIT](https://img.shields.io/github/license/sbrants/wavetrace)](LICENSE)
[![Latest release](https://img.shields.io/github/v/release/sbrants/wavetrace)](https://github.com/sbrants/wavetrace/releases)
[![Microsoft Store](https://img.shields.io/badge/Microsoft_Store-9P9M9DHX1L76-blue)](https://apps.microsoft.com/detail/9P9M9DHX1L76)

Desktop companion for the idle game **The Tower**. It watches the game window,
OCRs Tier / Wave / Coin-per-minute, records a snapshot every time the wave
advances, detects **wave skips**, and charts coin/min against wave for the
current and past runs.

## Quick start

| | |
| --- | --- |
| **Install** | [GitHub Releases](https://github.com/sbrants/wavetrace/releases) — Windows (NSIS), macOS (DMG), or Linux (AppImage). [Microsoft Store](https://apps.microsoft.com/detail/9P9M9DHX1L76) for MSIX. |
| **Develop** | `npm install` then `npm run tauri dev` (Rust, Node 18+, MSVC on Windows — see [Prerequisites](#prerequisites-windows-10)) |
| **Contribute** | [CONTRIBUTING.md](CONTRIBUTING.md) · [good first issues](https://github.com/sbrants/wavetrace/issues?q=is%3Aissue+is%3Aopen+label%3A%22good+first+issue%22) · [Code of Conduct](CODE_OF_CONDUCT.md) |
| **Docs** | [Goal.md](Goal.md) · [PRIVACY.md](PRIVACY.md) · OCR corpus [`fixtures/captured/manifest.json`](fixtures/captured/manifest.json) |

## Stack

- **Tauri 2** — Rust native shell + embedded webview
- **Rust backend** — window capture ([xcap](https://crates.io/crates/xcap)), Windows built-in OCR
  (Windows.Media.Ocr), SQLite ([rusqlite](https://crates.io/crates/rusqlite))
- **React + TypeScript + Vite** frontend, charts via Recharts

## Prerequisites (Windows 10+)

- Rust toolchain (`rustup`, MSVC)
- Node.js 18+ and npm
- Visual Studio C++ Build Tools
- WebView2 runtime (preinstalled on Windows 11 / recent Windows 10)

## Develop

```powershell
npm install
npm run tauri dev
```

Debug builds use an **orange-bordered** taskbar/tray icon and window title **WaveTrace (Dev)** so they are easy to tell apart from release installs.

## Contributing

Contributions are welcome. See [CONTRIBUTING.md](CONTRIBUTING.md) for setup, testing, and pull request guidelines. By participating, you agree to the [Code of Conduct](CODE_OF_CONDUCT.md). The project is licensed under the [MIT License](LICENSE). Security reports: [SECURITY.md](SECURITY.md).

## Test

```powershell
cd src-tauri
cargo test --release       # parser, state machine, classifier, db (+ captured corpus on Windows)
cargo test --release -- --ignored   # optional manual/debug tests only
```

### OCR regression corpus

Live window captures in `fixtures/captured/` guard against OCR/parser regressions on
Windows. `fixtures/captured/manifest.json` is the source of truth; optional `expect`
labels on entries enable strict checks.

**Capture live frames** (Settings → “Capture 80 test frames”, or CLI):

```powershell
cd src-tauri
cargo run --example capture_fixtures -- --count 50 --label-detected
cargo run --example capture_fixtures -- --prune-misses   # drop frames with no /min before commit
cargo test --release captured_corpus -- --nocapture
```

**Re-run OCR** on saved PNGs after parser/classify changes:

```powershell
cargo run --example reanalyze_corpus
```

**Backfill labels** on live captures for review:

```powershell
cargo run --example label_corpus
```

Reference game-mode PNGs at `fixtures/` root are committed for OCR regression
(`fixtures/reference.json`).

## Install (end users)

| Channel | How to get it | Updates |
| ------- | ------------- | ------- |
| **GitHub Releases** | Download the NSIS `.exe` (Windows), `.dmg` (macOS), or AppImage (Linux) from [releases](https://github.com/sbrants/wavetrace/releases). | In-app updater on Windows, macOS & Linux (GitHub `latest.json`) |
| **Microsoft Store** | Search for WaveTrace or open the [Store listing](https://apps.microsoft.com/detail/9P9M9DHX1L76) | Microsoft Store (Settings explains this; no GitHub in-app updater) |
| **Arch Linux** | `makepkg` from `packaging/arch/` or install from AUR if published | Package manager |
| **macOS (dev)** | `./scripts/build-macos.sh` on a Mac with Homebrew | Rebuild from source |

WaveTrace is local-only: no account, no cloud sync. See [PRIVACY.md](PRIVACY.md).

## Build a release bundle

```powershell
npm run tauri build
```

Outputs: `src-tauri/target/release/wavetrace.exe` plus MSI/NSIS installers under
`src-tauri/target/release/bundle/`.

### Microsoft Store (MSIX)

Package for Partner Center upload:

```powershell
npm run tauri:store:build
```

Output: `microsoft-store/out/Meringue.WaveTrace_<version>_x64.msix`. Store builds set
`VITE_STORE_DISTRIBUTION` so the GitHub auto-updater is disabled; Settings shows
Store-specific update guidance instead. Updates go through the Store. Full checklist:
[microsoft-store/README.md](microsoft-store/README.md).

On each `v*` tag push, the [Release workflow](.github/workflows/release.yml) also builds an
unsigned MSIX and attaches it to the GitHub release (no extra secrets required).

### Signed release (Microsoft Trusted Signing)

Unsigned builds show Windows SmartScreen warnings. To sign installers with
[Azure Artifact Signing](https://learn.microsoft.com/en-us/azure/trusted-signing/quickstart):

1. **Azure setup** (one-time): register `Microsoft.CodeSigning`, create an
   Artifact Signing account, complete identity validation (individual or org),
   create a **Public Trust** certificate profile, and an **App Registration**
   with a client secret. Grant the app **Trusted Signing Certificate Manager**
   (or equivalent signing role) on the account.
2. **Install signing CLI**: `powershell -File scripts/setup-trusted-signing.ps1`
3. **Configure secrets**: copy `.env.signing.example` → `.env.signing` and fill:
   `AZURE_CLIENT_ID`, `AZURE_CLIENT_SECRET`, `AZURE_TENANT_ID`,
   `AZURE_TRUSTED_SIGNING_ENDPOINT`, `AZURE_TRUSTED_SIGNING_ACCOUNT_NAME`,
   `AZURE_CERTIFICATE_PROFILE_NAME`
4. **Build signed**: `npm run tauri:build:signed`

Regular `npm run tauri build` stays unsigned (no Azure credentials required).

### Auto-update

**GitHub / direct-download builds** check GitHub on startup and offer one-click updates
(Settings → **Check for updates**). **Microsoft Store builds** skip the GitHub updater
and show that updates are delivered through the Store.

| Platform | Update format |
| -------- | ------------- |
| Windows (GitHub) | NSIS installer (`.exe`) |
| Windows (Store) | Microsoft Store |
| macOS | `.app.tar.gz` in-app (first install via DMG) |
| Linux    | AppImage (works on Ubuntu, Arch, etc.) |
| Arch pacman/AUR | Use your package manager — in-app updater targets AppImage |

**One-time setup** (repo maintainer):

1. Generate an updater keypair: `powershell -File scripts/setup-updater-signing.ps1`
2. Add GitHub secret **`TAURI_SIGNING_PRIVATE_KEY`** — paste the full contents of
   `%USERPROFILE%\.tauri\wavetrace.key` (not the `.pub` file). **Required** for the
   Release workflow; builds fail at the signing step if this secret is missing.
3. Optional password: **`TAURI_SIGNING_PRIVATE_KEY_PASSWORD`**
4. Push a `v*` tag — the **Release** workflow publishes installers, `latest.json`,
   and `.sig` files for the updater

The public key is embedded in `src-tauri/tauri.conf.json`. This is separate from
Azure Trusted Signing (SmartScreen); both are used on Windows release builds.

Local signed Windows builds can set `TAURI_SIGNING_PRIVATE_KEY_PATH` in
`.env.signing` so updater artifacts are signed alongside the installer.

## macOS

WaveTrace on macOS uses Tesseract OCR (same path as Linux). CI builds **two DMGs** per
release: `WaveTrace_<version>_macos_aarch64.dmg` (Apple Silicon) and
`WaveTrace_<version>_macos_x86_64.dmg` (Intel).

### Prerequisites (macOS 10.15+)

- Xcode Command Line Tools (`xcode-select --install`)
- Node.js 18+, Rust (`rustup`)
- Homebrew: `brew install tesseract dylibbundler`

### Build locally

```bash
git clone https://github.com/sbrants/wavetrace.git
cd wavetrace
./scripts/build-macos.sh
```

Output: `src-tauri/target/<arch>-apple-darwin/release/bundle/macos/WaveTrace_*_macos_*.dmg`

Grant **Screen Recording** in System Settings → Privacy & Security on first launch.

WaveTrace DMGs are ad-hoc signed, not notarized, so macOS quarantines downloads.
If the app is blocked after launch, open **System Settings → Privacy & Security**,
scroll down, and click **Open Anyway** next to the blocked WaveTrace app. As a
terminal fallback after copying it to Applications, clear the quarantine flag:
`xattr -dr com.apple.quarantine /Applications/WaveTrace.app`. Developer ID
notarization is planned.

## Arch Linux

WaveTrace is not built on Windows for Linux. Use an Arch machine, VM, or the
**Release** GitHub Actions workflow.

### Quick build (Arch)

```bash
git clone https://github.com/sbrants/wavetrace.git
cd wavetrace
./scripts/build-arch.sh
```

Outputs:

- `src-tauri/target/release/wavetrace`
- `src-tauri/target/release/bundle/appimage/*.AppImage` (portable)
- `src-tauri/target/release/bundle/deb/` (reference layout for packaging)

### Arch package (pacman)

```bash
cd packaging/arch
makepkg -si
```

Requires a git tag matching `pkgver` in `PKGBUILD` (currently `v0.2.24`), or edit
`PKGBUILD` to point at your branch/commit.

### Runtime dependencies (Arch)

- `webkit2gtk-4.1`, `gtk3` — Tauri webview
- `tesseract`, `tesseract-data-eng` — OCR (Linux uses Tesseract instead of Windows OCR)
- `pipewire`, `libpipewire` — window capture (`xcap` on Linux)

### CI artifacts

Push a `v*` tag or run **Release** manually on GitHub Actions to publish Windows
installers, macOS DMGs (Apple Silicon + Intel), Linux AppImage, Arch binary, and
`latest.json` for auto-update.

## Using the app

1. **Settings** tab: pick the emulator/game window and save. Use **Probe OCR** to
   verify capture before scanning.
2. Press **New run** (or **Resume run** to continue the last open run) in the header.
3. Play. Snapshots are written as the wave advances; the run closes on the Retry
   screen or a wave reset.
4. Press **Stop** when you are done scanning.
5. **Dashboard** shows live values (including **Wave jump** — `1` during normal play,
   or `×N` / a larger number when a skip was detected) and the coin/min-vs-wave chart
   with a **Wave jump** line on a second axis.
6. **History** lists past runs with filtering, sorting, CSV/ODS export, chart
   screenshots, recorded-skip selection/deletion, and a **Skip vs coin/min** analytics panel
   per selected run.
7. **Settings** also includes backup/restore, system tray and notification options,
   the scanner log viewer (Advanced), update checks, and an embedded changelog.

### System tray (optional)

With **Minimize to tray when the window is closed** enabled (default), closing the
window hides WaveTrace while scanning can continue. Use **Exit** in the header (or
**Quit** in the tray menu) to fully close the app.

### Backup & restore

Settings → **Backup & restore** exports your full local database (runs, snapshots,
wave skips, settings) as a zip. Stop the scanner first. Restore replaces the database and keeps
a safety copy of the previous file under `%APPDATA%\wavetrace\backups\`.

Data lives in `%APPDATA%/wavetrace/wavetrace.db` on Windows (migrates from
`wavewatch/` or `towerrun/` on first launch), `~/Library/Application Support/wavetrace/`
on macOS, or the XDG data dir on Linux; scanner diagnostics in `logs/scanner.log`
under that folder (rotates at 20 MiB per file, up to ~200 MiB total).

## Notes

- Tournament runs (`Tier N+` in game) are tagged `run_type = tournament` and
  can be filtered separately in History.
- When the game shows a **total coin balance** instead of a `/min` rate, the
  dashboard shows a warning banner and snapshots keep the last known rate
  (see Goal.md "Game mode edge cases").
- **Skips vs jumps** — every wave advance is a **jump** (usually `1`). A **skip** is the
  in-game **Wave Skipped!** upgrade; those are stored for History and analytics. The chart
  plots `+1` jumps between consecutive snapshots; larger jumps only when a skip was
  recorded (so scanner downtime does not draw false spikes). Details:
  [Goal.md — Skips vs jumps](Goal.md#skips-vs-jumps).
- **Wave skips** — when the game shows **Wave Skipped!**, WaveTrace records the
  event in the database. Resume after **Stop** re-syncs from the database so waves
  played while stopped are not counted as skips.
- Offline skip/coin analysis against your local DB:
  `python scripts/analyze_skip_coin.py` (optional; History has in-app analytics).
  Analysis filters (0.1T floor, 3× ratio cap): [Goal.md — Skip vs coin/min analytics](Goal.md#skip-vs-coinmin-analytics).
  Uses `wavetrace.db` under `%APPDATA%\wavetrace\` (legacy `towerrun.db` if present).
- Keep the project on a local drive: `node_modules/` and `target/` do not
  survive Google Drive's virtual filesystem.
