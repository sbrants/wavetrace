# WaveTrace Privacy Policy

**Effective date:** 15 June 2026  
**Publisher:** Meringue  
**App:** WaveTrace — automatic per-wave tracker for *The Tower*

This policy describes how WaveTrace handles information when you use the desktop application distributed via the Microsoft Store, GitHub Releases, or other channels.

## Summary

WaveTrace is a **local-only** companion app. It watches a **window you choose** (for example an emulator running *The Tower*), reads on-screen game stats with OCR, and saves run history on **your computer**. WaveTrace does **not** require an account, does **not** sell data, and does **not** upload your gameplay history to our servers.

## Information the app processes

### Data you provide or configure

- **Target window** — a window title substring and optional process name you pick in Settings so the app knows which window to watch.
- **Run comments** — optional notes you type in History.
- **Settings** — polling interval and UI preferences (for example the “Advanced” checkbox).

### Data collected automatically (on your device)

When scanning is enabled, WaveTrace periodically:

1. **Captures a screenshot** of the selected window only (not your full desktop).
2. **Runs OCR** on that image to extract game stats (tier, wave, coin/minute) and detect game mode.
3. **Writes snapshots** to a local SQLite database when the wave advances.

Stored fields include: run start/end times, run type (farming/tournament), peak tier, final wave, per-wave tier, coin/minute, and timestamps.

### Diagnostic logs

WaveTrace may write technical logs locally (for example OCR timing and errors) under your app data folder. These stay on your device unless you copy them elsewhere.

## Where data is stored

All app data is stored **locally** on your PC, typically:

- **Windows:** `%APPDATA%\wavetrace\`
  - `wavetrace.db` — run and snapshot database
  - `logs\scanner.log` — optional diagnostic log

Data is not synced to a cloud service operated by Meringue.

## Network use

| Channel | Network activity |
| ------- | ---------------- |
| **Microsoft Store build** | No gameplay data is sent to us. Updates are delivered by the Microsoft Store. |
| **Direct download (GitHub Releases)** | The app may check `https://github.com/sbrants/wavetrace` for optional software updates (version metadata and, if you accept, an installer download). No gameplay database or screenshots are transmitted. |

WaveTrace does **not** include analytics, advertising, or third-party tracking SDKs.

## What we do not collect

- No user accounts or passwords
- No payment information (the app is free)
- No address book, contacts, or files outside the window you selected
- No deliberate collection of data for advertising or profiling

**Note:** Screenshots may incidentally include whatever is visible in the chosen game window (including text you have on screen). Only capture a window you are comfortable processing locally.

## How you can control your data

- **Stop scanning** — ends new captures immediately.
- **Delete runs or individual snapshots** — in History.
- **Export** — CSV/ODS exports are files you save where you choose.
- **Remove all local data** — delete the `%APPDATA%\wavetrace\` folder while the app is closed.

## Children

WaveTrace is not directed at children under 13. We do not knowingly collect personal information from children.

## Legal bases and your rights

Because processing happens on your device for your own use, Meringue does not operate a central database of your gameplay. If you contact us about privacy, we generally cannot access or delete data on your PC—you can delete local files as described above.

Depending on where you live, you may have rights regarding personal data (for example access, correction, or erasure). To exercise them or ask questions, contact us using the details below.

## Changes

We may update this policy when the app changes. The effective date at the top will be revised. Continued use after an update means you accept the revised policy.

## Contact

- **Issues / privacy requests:** [github.com/sbrants/wavetrace/issues](https://github.com/sbrants/wavetrace/issues)
- **Source & releases:** [github.com/sbrants/wavetrace](https://github.com/sbrants/wavetrace)
