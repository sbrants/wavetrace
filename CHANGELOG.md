# Changelog

All notable changes to WaveTrace are documented here.
Format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### Added

- **Contributor docs** — MIT [LICENSE](LICENSE), [CONTRIBUTING.md](CONTRIBUTING.md), [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md), [SECURITY.md](SECURITY.md), issue/PR templates, PR CI workflow, Dependabot
- **Known issues** — macOS GitHub Release DMGs currently broken ([#4](https://github.com/sbrants/wavetrace/issues/4))

### Fixed

- **Anchor-crop OCR** — `@ 3.48T`-style lines without `/min` parse as coin/min again (not total balance)
- **State machine snapshots** — wave-1 auto-start no longer clears debounced coin rate; spike misreads no longer flash on the dashboard before confirmation

### Known issues

- **macOS** — GitHub Release DMGs (Apple Silicon and Intel) are currently **broken** (launch and/or scanning). Prefer Windows or Linux until fixed ([#4](https://github.com/sbrants/wavetrace/issues/4)).

---

## [0.2.26] — 2026-06-27

### Fixed

- **WebView out-of-memory on long runs** — dashboard and History no longer load every snapshot into the chart UI on each scanner tick; snapshots are downsampled server-side (200 points), live chart refresh is throttled, and a Reload button appears if the UI renderer crashes

---

## [0.2.25] — 2026-06-26

### Added

- **Wave jump UX** — dashboard **Wave jump** stat shows `1` during normal play or `×N` / a larger value when a skip was detected; chart second axis and tooltips use **Wave jump** / **Jump** labels
- **Chart normal jumps** — plots `+1` between consecutive snapshots; larger values only when a recorded skip exists at that wave
- **`skip_multiplier` column** — stores banner `×N` separately from observed wave increment (`skipped_count`) for display vs analytics
- **`skipDisplay.ts`** — shared formatting for dashboard, chart, and History
- **Docs — skips vs jumps** — [Goal.md](Goal.md#skips-vs-jumps) and README explain wave jump vs wave skip

### Changed

- **Coin-rate warning** — compact header pill instead of a full-width banner (no layout shift); stat cards reserve hint row height for **last known** on Coin/min
- **History** — skip table column **Wave jump** shows plain numbers (banner multiplier when stored)

### Fixed

- **Capture gaps** — chart no longer plots false multi-wave jumps after scanner downtime or missed OCR

---

## [0.2.24] — 2026-06-26

### Added

- **Accessibility (phases A & B)** — focus-visible rings, live regions for scanner/warnings/updates, `aria-current` nav, sortable History headers with `aria-sort`, labeled filters and Settings fieldsets, scanner log labels; `eslint-plugin-jsx-a11y`; roadmap for phases C–E in `docs/accessibility.md`
- **Skip vs coin/min analytics** — History panel: Pearson correlation by lag, median % change after skips, breakdown by skip size (coin/min > 0.1T)
- **Offline analysis script** — `scripts/analyze_skip_coin.py` against `%APPDATA%\wavetrace\wavetrace.db`
- **Dev builds** — orange-bordered taskbar/tray icon and **WaveTrace (Dev)** window title

### Fixed

- **Intro Sprint wave skips** — trust multi-wave jumps when banner `xN` is missing or OCR'd ±1 off (e.g. x9 vs +10); fast debounce baseline; more banner typos (`Wave Skived`, etc.)

---

## [0.2.23] — 2026-06-13

### Added

- **Dashboard** — live **Waves skipped** stat (most recent skip count in the current run)
- **Scanner log rotation** — rotate `scanner.log` at 20 MiB; keep `scanner.log` + `.1`…`.9` (~200 MiB on disk)

### Fixed

- **Wave skip detection** — correlate `skipped_count` to actual wave increment; banner `×N` must match multi-wave jumps; lone banner only with `+1`
- **Resume false skips** — catch-up grace after resume; always re-sync wave from DB on resume so waves played while stopped are not counted as skips
- **CI** — retry Arch release build on flaky `crates.io` downloads (`CARGO_NET_RETRY`, up to 3 attempts)

---

## [0.2.22] — 2026-06-22

### Added

- **Wave skips** — detect in-game “Wave Skipped!” (with or without `×N`), store per run, and plot on a second Y-axis (line chart)
- **History** — select/delete wave skips separately from coin/min snapshots; clear selection; chart click/drag selection for snapshots

### Fixed

- **Single-wave skips** — latch banner across polls so `×1` skips (no multiplier) match when the wave number updates after the banner
- **Skip chart** — line returns to 0 between skip events; dots only in History edit mode

---

## [0.2.21] — 2026-06-21

### Fixed

- **CI** — TypeScript null check in History live-refresh guard (`selected` possibly null)

---

## [0.2.20] — 2026-06-21

### Added

- **History live charts** — compare view and single-run detail auto-refresh while a run is ongoing (poll + scanner events)

### Fixed

- **History selection** — snapshot picks for deletion no longer clear or jump on live refresh

---

## [0.2.19] — 2026-06-20

### Fixed

- **macOS launch crash** — rewrite `@rpath` references between bundled Frameworks dylibs to `@loader_path`; verify no Homebrew/`@rpath` deps remain at bundle time

---

## [0.2.18] — 2026-06-20

### Fixed

- **macOS launch crash** — bundle all Homebrew dylib dependencies (e.g. `libarchive`) into the app, not only Tesseract/Leptonica

---

## [0.2.17] — 2026-06-13

### Fixed

- **macOS CI** — fix bash quoting syntax error in `bundle-macos-deps.sh` (exit code 2)

---

## [0.2.16] — 2026-06-13

### Fixed

- **macOS CI** — simplify Tesseract bundling (always fetch tessdata, drop pipefail/dylib hard-fail)

---

## [0.2.15] — 2026-06-13

### Fixed

- **macOS CI** — download `eng.traineddata` when Homebrew omits it; detect app binary via Mach-O `file` probe

---

## [0.2.14] — 2026-06-13

### Fixed

- **macOS CI** — broaden Homebrew tessdata/dylib discovery and only require dylibs when the binary links them dynamically

---

## [0.2.13] — 2026-06-13

### Fixed

- **macOS CI** — rewrite Tesseract bundling without `dylibbundler` / fragile `find -perm`; manual dylib copy with safer `otool` handling

---

## [0.2.12] — 2026-06-13

### Fixed

- **macOS CI** — replace `find -perm +111` (illegal on BSD/macOS runners, exit code 2) with direct binary detection in `bundle-macos-deps.sh`

---

## [0.2.11] — 2026-06-13

### Added

- **macOS auto-update** — signed `.app.tar.gz` updater bundles for Apple Silicon and Intel; CI publishes unified `latest.json` for all platforms

### Changed

- Release CI assembles `latest.json` in a single job (avoids parallel upload races on Windows/Linux/macOS)

---

## [0.2.10] — 2026-06-19

### Fixed

- **macOS CI** — bundle Tesseract from correct Homebrew `share/tessdata` path; build Intel DMG on `macos-15-intel` instead of cross-compiling on arm64

---

## [0.2.9] — 2026-06-18

### Added

- **macOS builds** — Apple Silicon (`aarch64`) and Intel (`x86_64`) DMGs via CI; Tesseract OCR bundled in the app
- Screen Recording permission string and macOS entitlements for window capture
- `npm run tauri:macos:build` / `scripts/build-macos.sh` for local Mac builds

### Notes

- macOS DMGs are ad-hoc signed in CI; Gatekeeper may require right-click → Open until Developer ID notarization is configured
- In-app auto-update on macOS is not enabled yet (download new DMG from GitHub Releases)

---

## [0.2.8] — 2026-06-18

### Fixed

- **Microsoft Store** — Settings no longer labels Store installs as a dev build; shows Store-specific update guidance instead

---

## [0.2.7] — 2026-06-18

### Added

- **Local backup & restore** — export/import full database as a zip (Settings → Backup & restore); safety copy before restore
- **Header Exit** — quit completely when close-to-tray is enabled (next to scan controls)

---

## [0.2.6] — 2026-06-18

### Added

- **System tray** — icon with scanner status tooltip; menu for Show, New run, Resume, Stop, Quit
- **Close to tray** — closing the window hides to the tray (optional in Settings)
- **Desktop notifications** — run ended, game window lost, optional wave milestones (Settings → Background)
- [docs/future-capabilities.md](docs/future-capabilities.md) — roadmap reference for later releases

### Fixed

- Vite dev server esbuild target (`es2022`) so Recharts/d3 pre-bundling works with `tauri dev`

---

## [0.2.4] — 2026-06-15

### Added

- Reference game-mode fixtures committed (`fixtures/reference.json` + edge-case PNGs)
- `total_coin_2.png` in OCR regression suite

### Fixed

- `total_coin` detection when `/min` is absent (bare balance lines like `2.72q`)
- Corpus tests fail on missing capture PNGs instead of skipping them

### Changed

- All `fixtures/` paths removed from `.gitignore` — new fixture files are tracked automatically

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
