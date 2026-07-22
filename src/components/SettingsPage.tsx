import { useEffect, useRef, useState } from "react";
import { listen, UnlistenFn } from "@tauri-apps/api/event";
import {
  api,
  AppDataInfo,
  formatCoin,
  NtfyStatusInfo,
  ScreenCaptureAccess,
  Settings,
  WindowInfo,
} from "../api";
import { downloadBase64File } from "../exportDownload";
import ScannerLogViewer from "./ScannerLogViewer";
import AppUpdater from "./AppUpdater";
import ChangelogPanel from "./ChangelogPanel";
import NotificationOption from "./NotificationOption";
import { installKindNote } from "../appDataInfo";
import { ntfyWaveMilestoneWarning } from "../ntfySettings";
import { reportUiError } from "../uiError";
import { confirmDialog } from "../confirmDialog";
import { captureDebugScreenshots } from "../debugPackage";

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
  const [ntfyRateLimit, setNtfyRateLimit] = useState<NtfyStatusInfo | null>(null);
  const [debugBusy, setDebugBusy] = useState(false);
  const [debugStatus, setDebugStatus] = useState<string | null>(null);
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

  const refreshNtfyStatus = async () => {
    try {
      const status = await api.getNtfyStatus();
      setNtfyRateLimit(status.rateLimited ? status : null);
    } catch {
      // ignore — status is optional UI sugar
    }
  };

  useEffect(() => {
    void refreshNtfyStatus();
    let unlisten: UnlistenFn | undefined;
    void listen<NtfyStatusInfo>("ntfy-rate-limited", (event) => {
      if (event.payload.rateLimited) {
        setNtfyRateLimit(event.payload);
      }
    }).then((fn) => {
      unlisten = fn;
    });
    return () => {
      void unlisten?.();
    };
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

  const generateDebugPackage = async () => {
    setDebugBusy(true);
    setDebugStatus(null);
    try {
      setDebugStatus("Capturing app screenshots…");
      const screenshots = await captureDebugScreenshots();
      setDebugStatus("Building debug package…");
      const result = await api.generateDebugPackage(screenshots);
      setDebugStatus(`Saved to ${result.path}`);
    } catch (e) {
      setDebugStatus(
        reportUiError(e, "Settings.generateDebugPackage")
      );
    } finally {
      setDebugBusy(false);
    }
  };

  const sendTestNtfy = async () => {
    setNtfyBusy(true);
    setNtfyStatus(null);
    try {
      await api.saveSettings(settings);
      await api.sendTestNtfy();
      setNtfyStatus("Test sent — check the ntfy app on your phone.");
      await refreshNtfyStatus();
    } catch (e) {
      const msg = reportUiError(e, "Settings.sendTestNtfy", { alert: false });
      setNtfyStatus(msg);
      await refreshNtfyStatus();
    } finally {
      setNtfyBusy(false);
    }
  };

  const dismissNtfyRateLimit = async () => {
    await api.clearNtfyRateLimit();
    setNtfyRateLimit(null);
  };

  const showPreview = async () => {
    try {
      setPreview(await api.previewCapture());
    } catch (e) {
      reportUiError(e, "Settings.previewCapture");
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
      setBackupStatus(reportUiError(e, "Settings.exportBackup", { alert: false }));
    } finally {
      setBackupBusy(false);
    }
  };

  const onRestoreFile = async (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    e.target.value = "";
    if (!file) return;

    const confirmed = await confirmDialog({
      title: "Restore backup?",
      message:
        "Your current database will be replaced. A copy of the current database is saved in the app data backups folder first.",
      confirmLabel: "Restore",
      danger: true,
    });
    if (!confirmed) return;

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
      setBackupStatus(reportUiError(err, "Settings.restoreBackup", { alert: false }));
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
                const msg = reportUiError(e, "Settings.probeOcr", { alert: false });
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
          Keep WaveTrace in the system tray while scanning. When minimize to tray
          is on, use <strong>Exit</strong> in the header to quit completely.
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
      </section>

      <section>
        <h3>Notifications</h3>
        <p className="muted">
          Choose which events can notify you, then pick how they are delivered
          (desktop, phone, or both).
        </p>

        <div className="notification-group">
          <h4>Run tracking</h4>
          <NotificationOption
            id="notify-run-ended"
            label="Run ended"
            description="When a run stops and final stats are saved."
            checked={settings.notify_run_ended ?? true}
            onChange={(checked) =>
              setSettings({ ...settings, notify_run_ended: checked })
            }
          />
          <NotificationOption
            id="notify-wave-every"
            label="Wave milestone"
            description="Every N waves during a run (e.g. 1,000). Leave empty to disable."
            control={
              <input
                id="notify-wave-every"
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
            }
          />
          {ntfyWaveWarning && (
            <div className="permission-callout notification-inline-callout">
              <p>{ntfyWaveWarning}</p>
            </div>
          )}
          <NotificationOption
            id="notify-coin-unavailable"
            label="Coin/min unavailable"
            description="After N seconds on the total-coins screen (same as the dashboard warning). Leave empty to disable."
            control={
              <input
                id="notify-coin-unavailable"
                type="number"
                min={0}
                step={30}
                placeholder="off"
                value={settings.notify_coin_unavailable_after_secs ?? ""}
                onChange={(e) => {
                  const raw = e.target.value.trim();
                  setSettings({
                    ...settings,
                    notify_coin_unavailable_after_secs:
                      raw === "" ? null : Math.max(1, Number.parseInt(raw, 10) || 0),
                  });
                }}
              />
            }
          />
        </div>

        <div className="notification-group">
          <h4>System</h4>
          <NotificationOption
            id="notify-system-shutdown"
            label="PC shutdown or restart"
            description="Best-effort alert when Windows is about to shut down or restart (e.g. updates). Uses your Delivery settings; won't fire on hard power-off."
            checked={settings.notify_system_shutdown ?? true}
            onChange={(checked) =>
              setSettings({ ...settings, notify_system_shutdown: checked })
            }
          />
        </div>

        <div className="notification-group">
          <h4>Scanner health</h4>
          <NotificationOption
            id="notify-window-lost"
            label="Game window not found"
            description="When WaveTrace can't see the target window during a scan."
            checked={settings.notify_window_lost ?? true}
            onChange={(checked) =>
              setSettings({ ...settings, notify_window_lost: checked })
            }
          />
        </div>

        <div className="notification-group">
          <h4>In-game popups</h4>
          <p className="muted notification-group-intro">
            Detected from OCR text while you play (mid-run banners).
          </p>
          <NotificationOption
            id="notify-research-complete"
            label="Lab research complete"
            description='When OCR sees "Research Complete:" (e.g. Starting Cash Lv.33).'
            checked={settings.notify_research_complete ?? true}
            onChange={(checked) =>
              setSettings({ ...settings, notify_research_complete: checked })
            }
          />
          <NotificationOption
            id="notify-event-mission-complete"
            label="Event mission complete"
            description='When OCR sees "EVENT MISSION COMPLETED" (e.g. Stun 50,000 enemies…).'
            checked={settings.notify_event_mission_complete ?? true}
            onChange={(checked) =>
              setSettings({ ...settings, notify_event_mission_complete: checked })
            }
          />
        </div>

        <div className="notification-group notification-delivery">
          <h4>Delivery</h4>
          <p className="muted notification-group-intro">
            Enable one or both channels. Phone alerts use a private ntfy topic —
            anyone who knows the topic can read messages, so treat it like a
            password.
          </p>
          {ntfyRateLimit?.message && (
            <div className="permission-callout notification-inline-callout">
              <p>
                <strong>ntfy rate limit (HTTP 429).</strong> {ntfyRateLimit.message}
              </p>
              <div className="row">
                <button type="button" onClick={() => void dismissNtfyRateLimit()}>
                  Dismiss
                </button>
              </div>
            </div>
          )}
          <NotificationOption
            id="notify-desktop-enabled"
            label="Desktop notifications"
            description="Show alerts in your OS notification center."
            checked={settings.notify_desktop_enabled ?? true}
            onChange={(checked) =>
              setSettings({ ...settings, notify_desktop_enabled: checked })
            }
          />
          <NotificationOption
            id="notify-ntfy-enabled"
            label="Phone alerts (ntfy)"
            description="Mirror enabled events above to the ntfy app on your phone."
            checked={settings.notify_ntfy_enabled ?? false}
            onChange={(checked) =>
              setSettings({ ...settings, notify_ntfy_enabled: checked })
            }
          />
          <NotificationOption
            id="notify-ntfy-attach"
            label="Attach game screenshot to phone alerts"
            description="JPEG capture on phone alerts: wave milestones, run ended, lab research, and event missions."
            checked={settings.notify_ntfy_attach_capture ?? true}
            disabled={!(settings.notify_ntfy_enabled ?? false)}
            onChange={(checked) =>
              setSettings({ ...settings, notify_ntfy_attach_capture: checked })
            }
          />
          <div className="notification-field-row">
            <label htmlFor="notify-ntfy-topic">
              ntfy topic or URL
              <input
                id="notify-ntfy-topic"
                type="text"
                placeholder="wavetrace-your-secret-topic"
                value={settings.notify_ntfy_topic ?? ""}
                disabled={!(settings.notify_ntfy_enabled ?? false)}
                onChange={(e) =>
                  setSettings({ ...settings, notify_ntfy_topic: e.target.value })
                }
              />
            </label>
          </div>
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
            app, subscribe to your topic, then use{" "}
            <strong>Send test notification</strong> to verify delivery (tests phone
            setup only; does not depend on the event toggles above).
          </p>
          <div className="toolbar">
            <button
              type="button"
              disabled={
                ntfyBusy ||
                !(settings.notify_ntfy_enabled ?? false) ||
                !(settings.notify_ntfy_topic ?? "").trim()
              }
              onClick={sendTestNtfy}
            >
              {ntfyBusy ? "Sending…" : "Send test notification"}
            </button>
          </div>
          {ntfyStatus && <p className="muted">{ntfyStatus}</p>}
        </div>
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
        <p className="muted">Polling interval, app log, and support tools.</p>
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

      <section>
        <h3>Support</h3>
        <p className="muted">
          Build a zip with <code>wavetrace.log</code> (recent tail), settings,
          scanner/runtime state, target-window OCR probe + capture, database summary, visible window list, and
          screenshots of the Dashboard, History, and Settings tabs when capture
          succeeds. Saves to your Downloads folder and opens Explorer with the
          file selected.
        </p>
        <div className="toolbar">
          <button
            type="button"
            disabled={debugBusy}
            onClick={generateDebugPackage}
          >
            {debugBusy ? "Generating…" : "Generate debugging package"}
          </button>
        </div>
        {debugStatus && (
          <p
            className={debugStatus.startsWith("Saved to") ? "muted" : "error"}
            role="status"
            aria-live="polite"
          >
            {debugStatus}
          </p>
        )}
      </section>
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
