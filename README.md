# WaveTrace

Desktop companion for the idle game **The Tower**. It watches the game
window, OCRs Tier / Wave / Coin-per-minute, records a snapshot every time the
wave advances, and charts coin/min against wave for the current and past runs.

Full product spec: [Goal.md](Goal.md). Test assets: [fixtures/](fixtures/).

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

## Test

```powershell
cd src-tauri
cargo test --release       # parser, state machine, classifier, db (+ Windows OCR fixtures on Windows)
cargo test --release -- --ignored   # optional manual/debug tests only
```

### OCR regression corpus

Bundled screenshots and optional live NoxPlayer captures in `fixtures/captured/` guard
against OCR/parser regressions on Windows.

**Fixture images** under `fixtures/captured/` are committed for OCR regression tests.
Capture your own corpus locally with the commands below; reference PNGs at
`fixtures/` root stay local-only.

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

**Run corpus tests**:

```powershell
cargo test --release captured_corpus -- --nocapture
```

## Build a release bundle

```powershell
npm run tauri build
```

Outputs: `src-tauri/target/release/wavetrace.exe` plus MSI/NSIS installers under
`src-tauri/target/release/bundle/`.

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

Release builds check GitHub on startup and offer one-click updates (Settings →
**Check for updates**).

| Platform | Update format |
| -------- | ------------- |
| Windows  | NSIS installer (`.exe`) |
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

## Arch Linux

WaveTrace is not built on Windows for Linux. Use an Arch machine, VM, or the
`Release Linux` GitHub Actions workflow.

### Quick build (Arch)

```bash
git clone https://github.com/sbrants/thetower-perftracker.git
cd thetower-perftracker
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

Requires a `v0.1.0` git tag on the remote, or edit `PKGBUILD` to point at your
branch/commit.

### Runtime dependencies (Arch)

- `webkit2gtk-4.1`, `gtk3` — Tauri webview
- `tesseract`, `tesseract-data-eng` — OCR (Linux uses Tesseract instead of Windows OCR)
- `pipewire`, `libpipewire` — window capture (`xcap` on Linux)

### CI artifacts

Push a `v*` tag or run **Release** manually on GitHub Actions to publish Windows
installers, Linux AppImage, Arch binary, and `latest.json` for auto-update.


## Using the app

1. **Settings** tab: pick the emulator/game window and save.
2. Press **Start scanning**.
3. Play. A run starts when wave 1 is confirmed; snapshots are written as the
  wave advances; the run closes on the Retry screen or a wave reset.
4. **Dashboard** shows live values and the coin/min-vs-wave chart.
  **History** lists past runs with filtering, sorting, and CSV export.

Data lives in `%APPDATA%/wavetrace/wavetrace.db` (migrates from `wavewatch/` or
`towerrun/` on first launch); scanner diagnostics in
`%APPDATA%/wavetrace/logs/scanner.log`.

## Notes

- Tournament runs (`Tier N+` in game) are tagged `run_type = tournament` and
can be filtered separately in History.
- When the game shows a **total coin balance** instead of a `/min` rate, the
dashboard shows a warning banner and snapshots keep the last known rate
(see Goal.md "Game mode edge cases").
- Keep the project on a local drive: `node_modules/` and `target/` do not
survive Google Drive's virtual filesystem.

