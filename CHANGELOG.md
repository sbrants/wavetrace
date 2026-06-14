# Changelog

All notable changes to WaveTrace are documented here.
Format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

---

## [0.2.3] — 2026-06-14

### Added

- OCR regression corpus in `fixtures/captured/` (live captures + `manifest.json` labels)

### Changed

- GitHub repo renamed to [`sbrants/wavetrace`](https://github.com/sbrants/wavetrace); updater endpoint updated
- Settings: polling interval and scanner log behind **Advanced** checkbox
- README refresh (repo links, corpus workflow, current UI)

### Removed

- Legacy `fixtures/expected.json` and seeded-corpus tooling

### Fixed

- Slimmer Rust deps: drop unused `tray-icon`, PNG-only `image`, `pollster` instead of `futures`
- Minor clippy cleanups

---

## [0.2.2] — 2026-06-14

### Fixed

- Release CI: correct Arch updater config path for `tauri build`

---

## [0.2.1] — 2026-06-14

### Added

- Embedded changelog in Settings (bundled from `CHANGELOG.md`)

### Fixed

- Release CI: run `npm ci` before `tauri-action` (Tauri CLI was missing on runners)
- Release CI: Arch job skips updater signing (raw binary only; AppImage handles Linux updates)

---

## [0.2.0] — 2026-06-14

### Added

- In-app auto-update (Windows NSIS, Linux AppImage) via GitHub Releases
- Settings → **Check for updates**; startup banner when a newer version exists
- Unified **Release** GitHub Actions workflow with signed `latest.json`

---

## [0.1.2] — 2026-06-14

First successful **Linux release** on GitHub Actions.

### Added

- **Linux downloads** on GitHub Releases: AppImage (Ubuntu 24.04 build) and Arch `x86_64` binary
- Arch packaging: `packaging/arch/PKGBUILD` and `scripts/build-arch.sh`

### Fixed

- AppImage CI: use Ubuntu 24.04 (PipeWire 1.0+ required by `xcap` / `libspa`)
- AppImage CI: add `lld` and `libgbm-dev` for Linux link step
- Arch CI: install `clang` so `bindgen` can find `libclang`

---

## [0.1.1] — 2026-06-14

### Fixed

- Linux CI: install `libpipewire-0.3-dev` and `libspa-0.2-dev` (required by `xcap` on Linux)

---

## [0.1.0] — 2026-06-14

First public release of **WaveTrace** — automatic per-wave tracker for *The Tower*.

### Added

- **Scanner** — watches the game/emulator window, OCRs Tier / Wave / Coin-per-minute, records a snapshot on each wave advance
- **Dashboard** — live HUD values and coin/min vs wave chart
- **History** — past runs with sorting, tier/wave/run-type filters, and **date range** filter
- **Resume run** — continue the last open run after stopping the scanner
- **Outlier cleanup** — delete bad snapshots from History (OCR glitches or manual mistakes)
- **Export** — filtered **CSV** (snapshots) and **ODS workbook** (runs + snapshots tables)
- **Chart screenshots** — copy or download PNG from Dashboard and History charts
- **Scanner log viewer** in Settings — tail `%APPDATA%/wavetrace/logs/scanner.log`
- **Run types** — farming vs tournament (`Tier N+`) tagging and filtering
- **Game mode warnings** — banner when the game shows total coin balance instead of `/min`
- **OCR regression corpus** — bundled and live-captured fixtures under `fixtures/captured/`
- **Linux OCR** — Tesseract on non-Windows (Windows uses built-in `Windows.Media.Ocr`)
- **Rebrand** to WaveTrace with oscilloscope icon; app data migrates from `towerrun/` → `wavewatch/` → `wavetrace/`

### Changed

- OCR pipeline uses full-frame scan + line classification (removed anchor/region templates)
- Settings UI simplified (removed in-app OCR test capture)

### Fixed

- Release webview loads correctly (`base: "./"` in Vite config for Tauri bundles)
