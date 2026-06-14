import { useEffect, useState } from "react";
import { api, formatCoin, Settings, WindowInfo } from "../api";
import ScannerLogViewer from "./ScannerLogViewer";

const showDevTools = import.meta.env.DEV;

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
  const [capturing, setCapturing] = useState(false);
  const [captureResult, setCaptureResult] = useState<string | null>(null);
  const [probe, setProbe] = useState<Awaited<ReturnType<typeof api.probeOcr>> | null>(
    null
  );
  const [probing, setProbing] = useState(false);
  const [probeError, setProbeError] = useState<string | null>(null);
  const [probeStatus, setProbeStatus] = useState<string | null>(null);
  const [probeElapsed, setProbeElapsed] = useState(0);

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

      {showDevTools && (
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
      )}

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

      <div className="toolbar">
        <button className="primary" onClick={save}>
          Save settings
        </button>
        {saved && <span className="saved">Saved ✓</span>}
      </div>
    </div>
  );
}
