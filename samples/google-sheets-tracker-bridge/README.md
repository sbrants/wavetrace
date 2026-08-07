# Google Sheets + tracker-bridge sample

Sidebar Apps Script that pulls `playerInfo.dat` through local **tracker-bridge** (WebSocket), then stores the file in Drive and logs a row on a `TowerSave` sheet.

## Why a sidebar?

`UrlFetchApp` runs on Google’s servers and **cannot** reach `127.0.0.1`. The sidebar HTML runs in your browser and can open `ws://127.0.0.1:43781`.

## Setup

1. Run `npx tracker-bridge` on the PC with the emulator.
2. In the spreadsheet: **Extensions → Apps Script**.
3. Create two files:
   - `Code.gs` — paste from [`Code.gs`](Code.gs)
   - `Sidebar` (HTML) — paste from [`Sidebar.html`](Sidebar.html) (Apps Script file name must be `Sidebar` to match `createHtmlOutputFromFile('Sidebar')`)
4. Save, close the script editor, reload the sheet.
5. Menu **Tower Save → Open sidebar**.
6. Status should say **Ready**. Click **Test Apps Script** (must succeed), then **Check bridge**, then **Pull save**.
7. Allow local network access if Chrome asks.

If buttons still do nothing, the HTML file name in Apps Script must be exactly `Sidebar` (not `Sidebar.html`), and you must replace the whole file contents then close/reopen the sidebar.

If **Test Apps Script** works but **Check bridge** fails, the sidebar iframe is likely blocking `ws://127.0.0.1` (CSP / local network). Workarounds: pull with WaveTrace **Download save**, or upload the `.dat` into Drive manually; a Chrome extension can also bridge localhost when the sidebar cannot.

## Optional

- Set **Extra ADB port** (e.g. `62001` for MuMu) to match your emulator.
- Check **Prefer USB phone** for physical devices (same flag as tracker-bridge `preferPhysicalDevice`).

## WaveTrace note

WaveTrace can also pull the save locally (**Download save** in the header) without tracker-bridge. This sample is for Sheets workflows that still use the bridge.
