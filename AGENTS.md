# AGENTS.md

## Cursor Cloud specific instructions

WaveTrace is a **Tauri 2 desktop app** (Rust backend + React/TypeScript/Vite frontend)
that tracks the idle game *The Tower* via window capture + OCR. There is a single
product; on Linux it uses Tesseract OCR (Windows uses the built-in Windows OCR).

### Environment already provided by the VM snapshot

- **System libraries** for the Tauri/Rust build are preinstalled via `apt` (webkit2gtk,
  gtk3, ayatana-appindicator, librsvg, tesseract, pipewire, gbm, …). The authoritative
  list is the `Install Linux dependencies` step in `.github/workflows/ci.yml`.
- **Rust toolchain**: the default stable toolchain is recent (≥ 1.85). This matters —
  several transitive crates require Rust **edition 2024**, so an old toolchain (e.g. the
  1.83 that some base images ship) fails with `feature edition2024 is required`. If a
  build fails that way, run `rustup update stable && rustup default stable`.
- Node 20+ is fine (CI uses Node 20).

### Running / building / testing

Standard commands live in `README.md` and `package.json`; the key ones:

| Task | Command | Notes |
| --- | --- | --- |
| Frontend deps | `npm install` | Run from repo root. |
| Lint | `npm run lint` | ESLint on `src/`. 2 pre-existing `react-hooks/exhaustive-deps` warnings in `History.tsx` are expected (good-first-issue #2). |
| Frontend build | `npm run build` | `tsc` typecheck + Vite build. |
| Rust tests | `cd src-tauri && cargo test --release` | 98 tests (parser, state machine, db, …). The Windows-only OCR `captured_corpus` tests do not run on Linux. |
| Run app (dev) | `npm run tauri dev` | Launches the GUI window. |

### Non-obvious gotchas

- **`npm run tauri dev`** runs Vite (port 1420) and then compiles the Rust shell. The
  **first** debug compile is heavy (~1.5 min, 730+ crates) before the window appears;
  subsequent runs are fast. Run it in a long-lived tmux session, not a one-shot command.
- The app is a **GUI**; a display is available at `DISPLAY=:1`. `libEGL warning: DRI3 …`
  lines on startup are harmless (software rendering) — the window still works.
- **"New run" requires a configured target window.** With no game/emulator running the
  app shows `No target window configured…`. For smoke-testing on the VM, pick any window
  from Settings → *Target window*, or seed it directly in the SQLite DB:
  `~/.local/share/wavetrace/wavetrace.db` →
  `INSERT INTO settings (key, value) VALUES ('target_window', '{"title_substring":"<some window title>","process_name":""}');`
  Once set, **New run → Scanning → History → Stop** works end-to-end (OCR metrics stay
  empty without a real game, which is expected).
- Local app data (DB, logs) lives under `~/.local/share/wavetrace/` on Linux.
