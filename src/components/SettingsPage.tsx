import { useEffect, useState } from "react";
import { api, Settings, WindowInfo } from "../api";

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
      title_substring: match.title,
      process_name: match.app_name,
    },
  };
}

export default function SettingsPage() {
  const [settings, setSettings] = useState<Settings | null>(null);
  const [windows, setWindows] = useState<WindowInfo[]>([]);
  const [preview, setPreview] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);
  const [capturing, setCapturing] = useState(false);
  const [captureResult, setCaptureResult] = useState<string | null>(null);

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

      <section>
        <h3>OCR test captures</h3>
        <p className="muted">
          Saves screenshots to <code>fixtures/captured/</code> with OCR metadata
          in <code>manifest.json</code>. Keep the game visible while capturing.
        </p>
        <div className="row">
          <button
            disabled={capturing}
            onClick={async () => {
              setCapturing(true);
              setCaptureResult(null);
              try {
                const r = await api.captureFixtureBurst(80, 400);
                setCaptureResult(
                  `Saved ${r.saved} frames (${r.coin_rate_detected} with coin/min detected). See ${r.captured_dir}`
                );
              } catch (e) {
                alert(e);
              } finally {
                setCapturing(false);
              }
            }}
          >
            {capturing ? "Capturing…" : "Capture 80 test frames"}
          </button>
        </div>
        {captureResult && <p className="muted">{captureResult}</p>}
      </section>

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

      <div className="toolbar">
        <button className="primary" onClick={save}>
          Save settings
        </button>
        {saved && <span className="saved">Saved ✓</span>}
      </div>
    </div>
  );
}
