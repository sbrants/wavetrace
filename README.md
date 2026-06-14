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

**Capture live frames** (Settings → “Capture 80 test frames”, or CLI):

```powershell
cd src-tauri
cargo run --example capture_fixtures -- --count 30
cargo run --example capture_fixtures -- --count 30 --label-detected   # auto-set expect when all fields detected
```

**Seed reference fixtures** from `fixtures/expected.json` (keeps existing live captures):

```powershell
cargo run --example seed_captured_corpus -- --clear-seeded
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

### CI artifacts

Push a `v*` tag or run **Release Linux** manually on GitHub Actions to publish an
AppImage and an Arch-built binary artifact.


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

