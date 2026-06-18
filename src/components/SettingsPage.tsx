import { useEffect, useRef, useState } from "react";
import { api, formatCoin, Settings, WindowInfo } from "../api";
import { downloadBase64File } from "../exportDownload";
import ScannerLogViewer from "./ScannerLogViewer";
import AppUpdater from "./AppUpdater";
import ChangelogPanel from "./ChangelogPanel";

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
    },
  };
}

export default function SettingsPage() {
  const [settings, setSettings] = useState<Settings | null>(null);
  const [windows, setWindows] = useState<WindowInfo[]>([]);
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
  const restoreInputRef = useRef<HTMLInputElement>(null);

  const load = async () => {
    const [loadedSettings, listedWindows] = await Promise.all([
      api.getSettings(),
      api.listWindows(),
    ]);
    setWindows(listedWindows);
    setSettings(withDefaultWindow(loadedSettings, listedWindows));
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

  const save = async () => {
    await api.saveSettings(settings);
    setSaved(true);
    setTimeout(() => setSaved(false), 1500);
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
        <div className="row">
          <select
            value={settings.target_window?.title_substring ?? ""}
            onChange={(e) =>
              setSettings({
                ...settings,
                target_window: e.target.value
                  ? { title_substring: e.target.value, process_name: "" }
                  : null,
              })
            }
          >
            <option value="">— pick a window —</option>
            {windows.map((w, i) => (
              <option key={i} value={w.title}>
                {w.title} {w.app_name ? `(${w.app_name})` : ""}
              </option>
            ))}
          </select>
          <button onClick={() => load()}>Refresh list</button>
        </div>
        <p className="muted">
          Matching is by title substring, so the window is found again after a
          restart even if the full title changes.
        </p>
        <div className="row">
          <input
            type="text"
            value={settings.target_window?.title_substring ?? ""}
            placeholder="Title substring (e.g. The Tower, BlueStacks)"
            onChange={(e) =>
              setSettings({
                ...settings,
                target_window: {
                  title_substring: e.target.value,
                  process_name: settings.target_window?.process_name ?? "",
                },
              })
            }
          />
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
          Keep WaveTrace in the system tray while scanning. Notifications are local only.
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
      </section>

      <section>
        <h3>Backup &amp; restore</h3>
        <p className="muted">
          Save or restore your full local database (runs, snapshots, and settings).
          Stop the scanner first. Backups are zip files you can copy to another PC or
          external drive.
        </p>
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
          <input
            ref={restoreInputRef}
            type="file"
            accept=".zip,application/zip"
            hidden
            onChange={onRestoreFile}
          />
        </div>
        {backupStatus && <p className="muted">{backupStatus}</p>}
      </section>

      <section className="settings-advanced-toggle">
        <label className="checkbox-inline">
          <input
            type="checkbox"
            checked={showAdvanced}
            onChange={(e) => {
              const on = e.target.checked;
              setShowAdvanced(on);
              saveShowAdvanced(on);
            }}
          />
          Advanced
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

      <ScannerLogViewer />
        </>
      )}

      <AppUpdater />

      <ChangelogPanel />

      <div className="toolbar">
        <button className="primary" onClick={save}>
          Save settings
        </button>
        {saved && <span className="saved">Saved ✓</span>}
      </div>
    </div>
  );
}
