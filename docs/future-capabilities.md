# Future capabilities (reference)

Ideas to revisit when planning releases. Not a commitment — prioritize based on user feedback and Store/cert constraints.

See also: [Goal.md](../Goal.md) (phases, acceptance criteria, open questions).

---

## Near term — biggest user impact

### 1. Background / occluded capture (Phase 2)

Still the main gap vs “real” companion usage. Today the emulator must stay visible. Improving capture when another window is on top (or documenting hard OS limits) would matter more than most chart tweaks.

### 2. More tracked fields (carefully)

Tier / Wave / Coin-min are the initial set. Wave skips and wave-jump charting already shipped. Good next candidates are **stable, OCR-friendly** values:

- Round / session coins (from end-of-run screen — `end_of_run` is already detected)
- Cash/min vs coin/min if the UI exposes both reliably

Avoid chasing every HUD stat until there are fixtures and mode rules like `total_coin` / `tournament`.

---

## Medium term — power users

### 3. Personal bests & run comparison

Extend existing run overlay and combine: “best coin/min at wave N”, “best run this tier”, compare two runs on the same wave axis.

### 4. Profiles per emulator/window

Saved window + poll interval per profile (“BlueStacks”, “phone mirror”) so switching setups isn’t all manual.

### 5. Cloud backup upload

Local zip backup/restore already ships. Google Drive or other cloud upload remains a possible follow-up (same bundle format).

### 6. Auto-start on login (optional)

Common for idle-game tools. **Autostart** plugin; off by default with a clear Settings toggle (Store-friendly).

### 7. OCR confidence / quality hints

Surface “low confidence” polls in the scanner log or dashboard when classification is shaky — builds trust without new game fields.

---

## Long term (Phase 3+) — only if scope should grow

### 8. Cloud sync + auth

Multi-device history, shared links, community stats. Biggest architectural shift (API, privacy policy, Store disclosure). Only worth it if users explicitly ask.

### 9. Android / iOS

Separate app, MediaProjection / ReplayKit, different OCR stack. Not an extension of the Tauri desktop app.

---

## Shipped (keep for history)

| Capability | Notes |
| --- | --- |
| System tray + scan in background | v0.2.6 |
| Desktop + ntfy notifications | run ended, window lost, wave milestones, research/event popups, shutdown |
| Local backup / restore | v0.2.7 — Settings → Backup & restore |
| macOS DMGs | v0.2.9; in-app updater v0.2.11 — Developer ID signing/notarization still open |
| Wave skips + jump charts | v0.2.22–v0.2.25 |

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

## Suggested priority

1. **Background capture** — hardest, but matches the product promise
2. **End-of-run stats capture** — builds on existing `end_of_run` work
3. macOS Developer ID signing/notarization (DMGs/updater already ship)

Defer cloud/mobile until there’s clear demand; WaveTrace’s strength is **local, focused, and trustworthy**.

---

*Captured from planning discussion, 2026-06; pruned shipped “near term” items 2026-08.*
