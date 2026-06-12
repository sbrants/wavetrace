# TowerRun Performance Tracker

Desktop companion app for the idle game **The Tower**. It watches the game
window, OCRs Tier / Wave / Coin-per-minute, records a snapshot every time the
wave advances, and charts coin/min against wave for the current and past runs.

Full product spec: [Goal.md](Goal.md). Test assets: [fixtures/](fixtures/).

## Stack

- **Tauri 2** — Rust native shell + embedded webview
- **Rust backend** — window capture ([xcap](https://crates.io/crates/xcap)), Windows built-in OCR
(Windows.Media.Ocr), template matching ([imageproc](https://crates.io/crates/imageproc)), SQLite ([rusqlite](https://crates.io/crates/rusqlite))
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
cargo test                 # parser, state machine, classifier, db, anchors
cargo test -- --ignored    # manual OCR check against fixtures/ images
```

## Build a release bundle

```powershell
npm run tauri build
```

## Using the app

1. **Settings** tab: pick the emulator/game window and save.
2. Press **Start scanning**.
3. Play. A run starts when wave 1 is confirmed; snapshots are written as the
  wave advances; the run closes on the Retry screen or a wave reset.
4. **Dashboard** shows live values and the coin/min-vs-wave chart.
  **History** lists past runs with filtering, sorting, and CSV export.

Data lives in `%APPDATA%/towerrun/towerrun.db`; scanner diagnostics in
`%APPDATA%/towerrun/logs/scanner.log`.

## Notes

- Tournament runs (`Tier N+` in game) are tagged `run_type = tournament` and
can be filtered separately in History.
- When the game shows a **total coin balance** instead of a `/min` rate, the
dashboard shows a warning banner and snapshots keep the last known rate
(see Goal.md "Game mode edge cases").
- Keep the project on a local drive: `node_modules/` and `target/` do not
survive Google Drive's virtual filesystem.

