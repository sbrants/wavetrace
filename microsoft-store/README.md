# Microsoft Store packaging (WaveTrace)

Product: [Partner Center overview](https://partner.microsoft.com/en-US/dashboard/products/9P9M9DHX1L76/overview)

## Reserved identity (from Partner Center)

| Field | Value |
| ----- | ----- |
| Package/Identity/Name | `Meringue.WaveTrace` |
| Package/Identity/Publisher | `CN=90A2A573-644F-4778-87F1-C7879133F905` |
| Package/Properties/PublisherDisplayName | `Meringue` |

These values are baked into `Package.appxmanifest`. Do not change them without updating Partner Center.

## Prerequisites

- Windows 11 (or Windows 10 with Windows SDK)
- Rust + Node (same as normal Tauri build)
- **winapp CLI** (recommended): `winget install Microsoft.winappcli`
- Or **Windows SDK** (`makeappx.exe`) — `winget install Microsoft.WindowsSDK.10.0.26100`

## Build MSIX for Store upload

```powershell
cd C:\Code\TowerRunPerformance
npm run tauri:store:build
```

Output: `microsoft-store/out/Meringue.WaveTrace_<version>_x64.msix`

- Upload **unsigned** MSIX to Partner Center → **Packages**. Microsoft re-signs after certification.
- Store builds disable the GitHub auto-updater (updates go through the Store).

### Local install test (optional)

```powershell
npm run tauri:store:build:local
winapp cert install microsoft-store\devcert.pfx   # admin, once
# double-click the .msix in microsoft-store/out/
```

## Submit in Partner Center

1. **Packages** — upload the `.msix` from `microsoft-store/out/`
2. **Store listings** — description, screenshots (1366×768+), category
3. **Privacy policy** — public URL (required):  
   `https://github.com/sbrants/wavetrace/blob/main/PRIVACY.md`  
   (source: [PRIVACY.md](../PRIVACY.md) — push to `main` before submitting)
4. **Age ratings** — complete IARC questionnaire
5. **Submit for certification**

### Reviewer notes (recommended)

> WaveTrace is a companion utility for the idle game *The Tower*. The user selects their emulator or game window in Settings. The app captures that window locally, runs OCR, and stores stats in local SQLite. It does not modify the game.

## Version bumps

MSIX uses four-part versions (`0.2.3.0`). Bump `version` in `src-tauri/tauri.conf.json` and `package.json` before each Store submission; the build script syncs the manifest automatically.
