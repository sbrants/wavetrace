import { useEffect, useRef, useState } from "react";
import {
  api,
  AppDataInfo,
  formatCoin,
  ScreenCaptureAccess,
  Settings,
  WindowInfo,
} from "../api";
import { downloadBase64File } from "../exportDownload";
import ScannerLogViewer from "./ScannerLogViewer";
import AppUpdater from "./AppUpdater";
import ChangelogPanel from "./ChangelogPanel";
import { installKindNote } from "../appDataInfo";
import {
  NTFY_RECOMMENDED_WAVE_EVERY_WITH_IMAGES,
  ntfyWaveMilestoneWarning,
} from "../ntfySettings";

const showDevTools = import.meta.env.DEV;
const ADVANCED_SETTINGS_KEY = "wavetrace.settings.advanced";

function loadShowAdvanced(): boolean {
  try {
    return localStorage.getItem(ADVANCED_SETTINGS_KEY) === "1";
  } catch {
    return false;
  }
}

function saveShowAdvanced(on: boolean) {
  try {
    localStorage.setItem(ADVANCED_SETTINGS_KEY, on ? "1" : "0");
  } catch {
    // ignore storage errors
  }
}

/** Default game window match per Goal.md "Window targeting". */
const TOWER_TITLE_MATCH = "The Tower";

function findTowerWindow(windows: WindowInfo[]): WindowInfo | undefined {
  const needle = TOWER_TITLE_MATCH.toLowerCase();
  return windows.find((w) => w.title.toLowerCase().includes(needle));
}

/** Preselect the game window when nothing is saved yet. */
function withDefaultWindow(settings: Settings, windows: WindowInfo[]): Settings {
  if (settings.target_window?.title_substring) {
    return settings;
  }
  const match = findTowerWindow(windows);
  if (!match) {
    return settings;
  }
  return {
    ...settings,
    target_window: {
      title_substring: TOWER_TITLE_MATCH,
      process_name: match.app_name,
      user_selected: false,
    },
  };
}

export default function SettingsPage() {
  const [settings, setSettings] = useState<Settings | null>(null);
  const [windows, setWindows] = useState<WindowInfo[]>([]);
  const [screenAccess, setScreenAccess] =
    useState<ScreenCaptureAccess>("not_required");
  const [preview, setPreview] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);
  const [probe, setProbe] = useState<Awaited<ReturnType<typeof api.probeOcr>> | null>(
    null
  );
  const [probing, setProbing] = useState(false);
  const [probeError, setProbeError] = useState<string | null>(null);
  const [probeStatus, setProbeStatus] = useState<string | null>(null);
  const [probeElapsed, setProbeElapsed] = useState(0);
  const [showAdvanced, setShowAdvanced] = useState(loadShowAdvanced);
  const [backupStatus, setBackupStatus] = useState<string | null>(null);
  const [backupBusy, setBackupBusy] = useState(false);
  const [ntfyStatus, setNtfyStatus] = useState<string | null>(null);
  const [ntfyBusy, setNtfyBusy] = useState(false);
  const [appData, setAppData] = useState<AppDataInfo | null>(null);
  const restoreInputRef = useRef<HTMLInputElement>(null);

  const load = async () => {
    const [loadedSettings, listedWindows, access, dataPaths] = await Promise.all([
      api.getSettings(),
      api.listWindows(),
      api.screenCaptureAccess(),
      api.getAppDataInfo(),
    ]);
    setWindows(listedWindows);
    setScreenAccess(access);
    setAppData(dataPaths);
    setSettings(withDefaultWindow(loadedSettings, listedWindows));
  };

  const recheckScreenAccess = async () => {
    const access = await api.requestScreenCaptureAccess();
    setScreenAccess(access);
    await load();
  };

  useEffect(() => {
    load();
  }, []);

  useEffect(() => {
    if (!probing) {
      return;
    }
    const started = Date.now();
    setProbeElapsed(0);
    const id = window.setInterval(() => {
      setProbeElapsed(Math.floor((Date.now() - started) / 1000));
    }, 500);
    return () => window.clearInterval(id);
  }, [probing]);

  if (!settings) return <p className="muted">Loading…</p>;

  const ntfyWaveWarning = ntfyWaveMilestoneWarning(settings);

  const save = async () => {
    await api.saveSettings(settings);
    setSaved(true);
    setTimeout(() => setSaved(false), 1500);
  };

  const sendTestNtfy = async () => {
    setNtfyBusy(true);
    setNtfyStatus(null);
    try {
      await api.saveSettings(settings);
      await api.sendTestNtfy();
      setNtfyStatus("Test sent — check the ntfy app on your phone.");
    } catch (e) {
      setNtfyStatus(String(e));
    } finally {
      setNtfyBusy(false);
    }
  };

  const showPreview = async () => {
    try {
      setPreview(await api.previewCapture());
    } catch (e) {
      alert(e);
    }
  };

  const exportBackup = async () => {
    setBackupBusy(true);
    setBackupStatus(null);
    try {
      const running = await api.scannerRunning();
      if (running) {
        setBackupStatus("Stop the scanner before backing up.");
        return;
      }
      const result = await api.exportBackup();
      downloadBase64File(
        result.data_base64,
        result.filename,
        "application/zip"
      );
      setBackupStatus(
        `Backup saved (${result.run_count} runs, ${result.snapshot_count} snapshots).`
      );
    } catch (e) {
      setBackupStatus(String(e));
    } finally {
      setBackupBusy(false);
    }
  };

  const onRestoreFile = async (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    e.target.value = "";
    if (!file) return;

    const ok = window.confirm(
      "Restore from this backup? Your current database will be replaced. " +
        "A copy of the current database is saved in the app data backups folder first."
    );
    if (!ok) return;

    setBackupBusy(true);
    setBackupStatus(null);
    try {
      const running = await api.scannerRunning();
      if (running) {
        setBackupStatus("Stop the scanner before restoring.");
        return;
      }
      const bytes = await file.arrayBuffer();
      const dataBase64 = btoa(
        Array.from(new Uint8Array(bytes), (b) => String.fromCharCode(b)).join("")
      );
      const result = await api.restoreBackup(dataBase64);
      await load();
      const when = result.backup_created_at
        ? ` from ${new Date(result.backup_created_at).toLocaleString()}`
        : "";
      setBackupStatus(
        `Restored${when}: ${result.run_count} runs, ${result.snapshot_count} snapshots.` +
          (result.safety_copy_path
            ? ` Previous data saved to backups folder.`
            : "")
      );
    } catch (err) {
      setBackupStatus(String(err));
    } finally {
      setBackupBusy(false);
    }
  };

  return (
    <div className="settings">
      <section>
        <h3>Target window</h3>
        {screenAccess === "denied" && (
          <div className="permission-callout">
            <p>
              <strong>macOS needs Screen Recording permission.</strong> Without
              it, WaveTrace can't read window titles or capture the game, so the
              list below stays empty.
            </p>
            <ol>
              <li>
                Open <strong>System Settings → Privacy &amp; Security → Screen
                Recording</strong>.
              </li>
              <li>
                Enable <strong>WaveTrace</strong> (add it with <strong>+</strong>{" "}
                if it isn't listed).
              </li>
              <li>Quit and reopen WaveTrace.</li>
            </ol>
            <p className="muted">
              After an auto-update, macOS may keep a stale Screen Recording
              entry for the previous WaveTrace build. If permission still shows
              as missing, remove WaveTrace from this list and add it again, or
              run <code>tccutil reset ScreenCapture com.wavetrace.app</code> in
              Terminal and grant permission again on next launch.
            </p>
            <div className="row">
              <button onClick={() => api.openScreenRecordingSettings()}>
                Open Screen Recording settings
              </button>
              <button onClick={recheckScreenAccess}>Recheck</button>
            </div>
          </div>
        )}
        <div className="row">
          <label htmlFor="target-window-select">
            Game window
            <select
              id="target-window-select"
              value={settings.target_window?.title_substring ?? ""}
            onChange={(e) => {
              const title = e.target.value;
              const match = windows.find((w) => w.title === title);
              setSettings({
                ...settings,
                target_window: title
                  ? {
                      title_substring: title,
                      process_name: match?.app_name ?? "",
                      user_selected: true,
                    }
                  : null,
              });
            }}
          >
            <option value="">— pick a window —</option>
            {windows.map((w, i) => (
              <option key={i} value={w.title}>
                {w.title} {w.app_name ? `(${w.app_name})` : ""}
              </option>
            ))}
          </select>
          </label>
          <button onClick={() => load()}>Refresh list</button>
        </div>
        <p className="muted">
          Pick a window from the list to target that window by name. The title
          substring field is for flexible matching when auto-detecting (e.g. first
          run without a saved choice).
        </p>
        <div className="row">
          <label htmlFor="target-title-substring">
            Title substring
            <input
              id="target-title-substring"
              type="text"
              value={settings.target_window?.title_substring ?? ""}
              placeholder="Title substring (e.g. The Tower, BlueStacks)"
              onChange={(e) =>
                setSettings({
                  ...settings,
                  target_window: {
                    title_substring: e.target.value,
                    process_name: settings.target_window?.process_name ?? "",
                    user_selected: false,
                  },
                })
              }
            />
          </label>
        </div>
        <div className="row">
          <button onClick={showPreview}>Preview window</button>
        </div>
        {preview && (
          <img
            className="preview"
            src={`data:image/png;base64,${preview}`}
            alt="capture preview"
          />
        )}
      </section>

      {showDevTools && (
      <section>
        <h3>OCR diagnostic</h3>
        <p className="muted">
          Runs Windows OCR on the full capture. Coin uses the second line containing
          /min (first is usually cash). Tier/Wave are parsed from lines containing
          those words. Uses the built-in Windows 10+ OCR engine (no extra install).
        </p>
        <div className="row">
          <button
            disabled={probing}
            onClick={async () => {
              setProbing(true);
              setProbe(null);
              setProbeError(null);
              setProbeStatus("Preparing…");
              try {
                if (await api.scannerRunning()) {
                  setProbeStatus("Stopping scanner so OCR can run…");
                  await api.stopScanner();
                  await new Promise((r) => setTimeout(r, 800));
                }
                setProbeStatus("Capturing window and running OCR (usually 5–15s)…");
                setProbe(await api.probeOcr());
                setProbeStatus(null);
              } catch (e) {
                const msg = String(e);
                setProbeError(msg);
                setProbeStatus(null);
              } finally {
                setProbing(false);
              }
            }}
          >
            {probing ? `Testing… ${probeElapsed}s` : "Test OCR now"}
          </button>
        </div>
        {probing && probeStatus && (
          <p className="muted">{probeStatus}</p>
        )}
        {probeError && (
          <p className="error">{probeError}</p>
        )}
        {probe && (
          <div className="ocr-probe">
            <p>
              <strong>Window:</strong>{" "}
              {probe.window_found
                ? `${probe.width}×${probe.height} (match "${probe.target_substring}")`
                : `not found (looking for "${probe.target_substring}")`}
              {" · "}
              <strong>{probe.elapsed_ms}ms</strong>
            </p>
            <p>
              <strong>Tier:</strong> {probe.tier ?? "—"} · <strong>Wave:</strong>{" "}
              {probe.wave ?? "—"} · <strong>Coin/min:</strong>{" "}
              {probe.coin_per_minute != null
                ? formatCoin(probe.coin_per_minute)
                : probe.coin_status}
            </p>
            <p className="muted">
              /min lines: {JSON.stringify(probe.coin_lines)}
              <br />
              all OCR text ({probe.all_lines?.length ?? probe.tier_wave_lines.length}{" "}
              lines):
            </p>
            <pre className="ocr-dump">
              {(probe.all_lines ?? probe.tier_wave_lines).join("\n")}
            </pre>
            {probe.preview_png_base64 && (
              <img
                className="preview"
                src={`data:image/png;base64,${probe.preview_png_base64}`}
                alt="OCR probe capture"
              />
            )}
          </div>
        )}
      </section>
      )}

      <section>
        <h3>Background</h3>
        <p className="muted">
          Keep WaveTrace in the system tray while scanning. Desktop notifications
          stay local; optional ntfy can mirror the same events to your phone.
          When minimize to tray is on, use <strong>Exit</strong> in the header to quit completely.
        </p>
        <label className="checkbox-inline">
          <input
            type="checkbox"
            checked={settings.minimize_to_tray ?? true}
            onChange={(e) =>
              setSettings({ ...settings, minimize_to_tray: e.target.checked })
            }
          />
          Minimize to tray when the window is closed
        </label>
        <label className="checkbox-inline">
          <input
            type="checkbox"
            checked={settings.notify_run_ended ?? true}
            onChange={(e) =>
              setSettings({ ...settings, notify_run_ended: e.target.checked })
            }
          />
          Notify when a run ends
        </label>
        <label className="checkbox-inline">
          <input
            type="checkbox"
            checked={settings.notify_window_lost ?? true}
            onChange={(e) =>
              setSettings({ ...settings, notify_window_lost: e.target.checked })
            }
          />
          Notify when the game window is not found
        </label>
        <div className="row">
          <label>
            Wave milestone (every N waves, optional)
            <input
              type="number"
              min={0}
              step={100}
              placeholder="off"
              value={settings.notify_wave_every ?? ""}
              onChange={(e) => {
                const raw = e.target.value.trim();
                setSettings({
                  ...settings,
                  notify_wave_every:
                    raw === "" ? null : Math.max(1, Number.parseInt(raw, 10) || 0),
                });
              }}
            />
          </label>
        </div>
        {ntfyWaveWarning && (
          <div className="permission-callout">
            <p>{ntfyWaveWarning}</p>
          </div>
        )}

        <h4>Phone alerts (ntfy)</h4>
        <p className="muted">
          Install the free{" "}
          <a
            href="https://ntfy.sh"
            onClick={(e) => {
              e.preventDefault();
              void api.openExternalUrl("https://ntfy.sh");
            }}
          >
            ntfy
          </a>{" "}
          app, subscribe to a hard-to-guess topic, then enter that topic below.
          Anyone who knows the topic can read messages, so treat it like a password.
          The toggles above also control what is sent to your phone. With screenshots
          enabled, use wave milestones of{" "}
          {NTFY_RECOMMENDED_WAVE_EVERY_WITH_IMAGES.toLocaleString()}+ to stay within
          ntfy.sh attachment limits.
        </p>
        <label className="checkbox-inline">
          <input
            type="checkbox"
            checked={settings.notify_ntfy_enabled ?? false}
            onChange={(e) =>
              setSettings({ ...settings, notify_ntfy_enabled: e.target.checked })
            }
          />
          Send notifications to ntfy
        </label>
        <label className="checkbox-inline">
          <input
            type="checkbox"
            checked={settings.notify_ntfy_attach_capture ?? true}
            disabled={!(settings.notify_ntfy_enabled ?? false)}
            onChange={(e) =>
              setSettings({
                ...settings,
                notify_ntfy_attach_capture: e.target.checked,
              })
            }
          />
          Attach game screenshot to ntfy (wave milestones and run ended)
        </label>
        <div className="row">
          <label>
            ntfy topic or URL
            <input
              type="text"
              placeholder="wavetrace-your-secret-topic"
              value={settings.notify_ntfy_topic ?? ""}
              onChange={(e) =>
                setSettings({ ...settings, notify_ntfy_topic: e.target.value })
              }
            />
          </label>
        </div>
        <div className="toolbar">
          <button
            type="button"
            disabled={ntfyBusy || !(settings.notify_ntfy_topic ?? "").trim()}
            onClick={sendTestNtfy}
          >
            {ntfyBusy ? "Sending…" : "Send test notification"}
          </button>
        </div>
        {ntfyStatus && <p className="muted">{ntfyStatus}</p>}
      </section>

      <section>
        <h3>Backup &amp; restore</h3>
        <p className="muted">
          Save or restore your full local database (runs, snapshots, and settings).
          Stop the scanner first. Backups are zip files you can copy to another PC or
          external drive.
        </p>
        {appData && (
          <p className="muted">
            {installKindNote(appData.install_kind)}
            <br />
            Database: <code>{appData.database_path}</code>
            <br />
            Backups folder: <code>{appData.backups_dir}</code>
          </p>
        )}
        <div className="toolbar">
          <button disabled={backupBusy} onClick={exportBackup}>
            Back up now…
          </button>
          <button
            disabled={backupBusy}
            className="danger"
            onClick={() => restoreInputRef.current?.click()}
          >
            Restore from file…
          </button>
          <label htmlFor="restore-backup-file" className="visually-hidden">
            Restore database from zip backup
          </label>
          <input
            id="restore-backup-file"
            ref={restoreInputRef}
            type="file"
            accept=".zip,application/zip"
            hidden
            onChange={onRestoreFile}
          />
        </div>
        {backupStatus && (
          <p className="muted" role="status" aria-live="polite">
            {backupStatus}
          </p>
        )}
      </section>

      <AppUpdater />

      <ChangelogPanel />

      <section className="settings-advanced-toggle">
        <label className="checkbox-inline">
          Show advanced settings
          <input
            type="checkbox"
            checked={showAdvanced}
            onChange={(e) => {
              const on = e.target.checked;
              setShowAdvanced(on);
              saveShowAdvanced(on);
            }}
          />
        </label>
        <p className="muted">Polling interval and scanner log.</p>
      </section>

      {showAdvanced && (
        <>
      <section>
        <h3>Polling</h3>
        <div className="row">
          <label>
            Interval (ms)
            <input
              type="number"
              min={250}
              step={250}
              value={settings.poll_interval_ms}
              onChange={(e) =>
                setSettings({
                  ...settings,
                  poll_interval_ms: Number(e.target.value),
                })
              }
            />
          </label>
        </div>
      </section>

      <ScannerLogViewer appData={appData} />
        </>
      )}

      <div className="toolbar">
        <button className="primary" onClick={save}>
          Save settings
        </button>
        {saved && (
          <span className="saved" role="status" aria-live="polite">
            Saved ✓
          </span>
        )}
      </div>
    </div>
  );
}
