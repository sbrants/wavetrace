# Future capabilities (reference)

Ideas to revisit when planning releases. Not a commitment — prioritize based on user feedback and Store/cert constraints after the initial Microsoft Store launch.

See also: [Goal.md](../Goal.md) (phases, acceptance criteria, open questions).

---

## Near term — biggest user impact

### 1. Background / occluded capture (Phase 2)

Still the main gap vs “real” companion usage. Today the emulator must stay visible. Improving capture when another window is on top (or documenting hard OS limits) would matter more than most chart tweaks.

### 2. macOS (Phase 1b) ✅ shipped (v0.2.9)

DMGs for Apple Silicon and Intel on GitHub Releases. Follow-ups: Developer ID signing/notarization, optional Apple Vision OCR vs Tesseract tuning. In-app updater shipped in v0.2.11.

### 3. System tray + “scan in background” ✅ shipped

### 4. Notifications ✅ shipped

Lightweight wins: run ended, target window lost for N minutes, optional “wave X” milestone. Local-only, no cloud. On Tauri that likely means the **notification** plugin plus a capability entry when added.

### 5. More tracked fields (carefully)

Tier / Wave / Coin-min are the initial set. **Wave skips** shipped in v0.2.22–v0.2.24 (detection, charting, skip/coin analytics). Good next candidates are **stable, OCR-friendly** values:

- Round / session coins (from end-of-run screen — `end_of_run` is already detected)
- Cash/min vs coin/min if the UI exposes both reliably

Avoid chasing every HUD stat until there are fixtures and mode rules like `total_coin` / `tournament`.

---

## Medium term — power users

### 6. Personal bests & run comparison

Extend existing run overlay and combine: “best coin/min at wave N”, “best run this tier”, compare two runs on the same wave axis.

### 7. Profiles per emulator/window

Saved window + poll interval per profile (“BlueStacks”, “phone mirror”) so switching setups isn’t all manual.

### 8. Smarter export / portable backup ✅ shipped (v0.2.7)

Zip backup of `wavetrace.db` + manifest via Settings → **Backup & restore**. Google Drive or other cloud upload remains a possible follow-up (same bundle format).

### 9. Auto-start on login (optional)

Common for idle-game tools. **Autostart** plugin; off by default with a clear Settings toggle (Store-friendly).

### 10. OCR confidence / quality hints

Surface “low confidence” polls in the scanner log or dashboard when classification is shaky — builds trust without new game fields.

---

## Long term (Phase 3+) — only if scope should grow

### 11. Cloud sync + auth

Multi-device history, shared links, community stats. Biggest architectural shift (API, privacy policy, Store disclosure). Only worth it if users explicitly ask.

### 12. Android / iOS

Separate app, MediaProjection / ReplayKit, different OCR stack. Not an extension of the Tauri desktop app.

---

## Tauri capabilities (`desktop.json`)

Current permissions: `core:default`, `core:tray:default`, `updater:default`, `process:default`, `notification:default`. Add permissions **only when a feature needs them**:

| When you build… | Likely add |
| --------------- | ---------- |
| Tray icon, minimize on close | `core:tray:default` ✅ |
| Run-end / window-lost alerts | `notification:default` ✅ |
| Start with Windows | `autostart:default` |
| “Save export as…” from UI | `dialog:default`, scoped `fs:allow-write-*` |
| Open docs / GitHub in browser | `shell:allow-open` |

Keep exports on the Rust side (as today) if you want fewer Store surface-area questions.

---

## Suggested priority after Store certification

1. ~~**Tray + notifications**~~ — done (v0.2.6)
2. ~~**Local backup / restore**~~ — done (v0.2.7)
3. ~~**macOS DMGs**~~ — done (v0.2.9); ~~macOS updater~~ — done (v0.2.11); Developer ID signing/notarization remains
4. **Background capture** — hardest, but matches the product promise
5. **End-of-run stats capture** — builds on existing `end_of_run` work

Defer cloud/mobile until there’s clear demand; WaveTrace’s strength is **local, focused, and trustworthy**.

---

*Captured from planning discussion, 2026-06.*
