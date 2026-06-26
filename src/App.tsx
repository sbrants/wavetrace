import { useCallback, useEffect, useState } from "react";
import { api, ScanStartMode, ScannerEvent } from "./api";
import Dashboard from "./components/Dashboard";
import History from "./components/History";
import SettingsPage from "./components/SettingsPage";
import AppUpdater from "./components/AppUpdater";

type Tab = "dashboard" | "history" | "settings";

function scannerStatusLabel(
  running: boolean,
  status: string | undefined
): string {
  if (!running) return "Scanner stopped";
  switch (status) {
    case "scanning":
      return "Scanner scanning";
    case "starting":
      return "Scanner starting";
    case "window_not_found":
      return "Game window not found";
    case "ocr_error":
      return "Scanner OCR error";
    case "stopped":
      return "Scanner stopped";
    default:
      return "Scanner active";
  }
}

export default function App() {
  const [tab, setTab] = useState<Tab>("dashboard");
  const [scannerEvent, setScannerEvent] = useState<ScannerEvent | null>(null);
  const [running, setRunning] = useState(false);
  const [canResume, setCanResume] = useState(false);
  const [minimizeToTray, setMinimizeToTray] = useState(true);

  const refreshCanResume = useCallback(() => {
    api.hasResumableRun().then(setCanResume).catch(() => setCanResume(false));
  }, []);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    api.onScannerUpdate((e) => {
      setScannerEvent(e);
      setRunning(e.status !== "stopped");
      if (e.status === "stopped") {
        refreshCanResume();
      }
    }).then((fn) => (unlisten = fn));
    api.scannerRunning().then(setRunning);
    refreshCanResume();
    return () => unlisten?.();
  }, [refreshCanResume]);

  useEffect(() => {
    api
      .getSettings()
      .then((s) => setMinimizeToTray(s.minimize_to_tray ?? true))
      .catch(() => setMinimizeToTray(true));
  }, [tab]);

  const startScanning = (mode: ScanStartMode) => {
    api
      .startScanner(mode)
      .then(() => {
        setRunning(true);
        setScannerEvent((prev) => ({
          status: "starting",
          live: prev?.live ?? null,
          current_run_id: prev?.current_run_id ?? null,
        }));
      })
      .catch((e) => alert(String(e)));
  };

  const warning = scannerEvent?.live?.total_coin_warning ?? false;

  return (
    <div className="app">
      <header>
        <h1>WaveTrace</h1>
        <nav>
          {(["dashboard", "history", "settings"] as Tab[]).map((t) => (
            <button
              key={t}
              className={tab === t ? "active" : ""}
              aria-current={tab === t ? "page" : undefined}
              onClick={() => setTab(t)}
            >
              {t[0].toUpperCase() + t.slice(1)}
            </button>
          ))}
        </nav>
        <div className="header-right">
          <span
            className={`status status-${scannerEvent?.status ?? "stopped"}`}
            role="status"
            aria-live="polite"
            aria-atomic="true"
          >
            {scannerStatusLabel(running, scannerEvent?.status)}
          </span>
          <div className="header-actions">
            {running ? (
              <button
                onClick={() => {
                  setRunning(false);
                  setScannerEvent((prev) => ({
                    status: "stopped",
                    live: prev?.live ?? null,
                    current_run_id: prev?.current_run_id ?? null,
                  }));
                  api.stopScanner();
                }}
              >
                Stop
              </button>
            ) : (
              <>
                <button
                  className="primary"
                  onClick={() => startScanning("new_run")}
                >
                  New run
                </button>
                <button
                  disabled={!canResume}
                  aria-describedby={!canResume ? "resume-run-hint" : undefined}
                  title={
                    canResume
                      ? "Continue the last open run"
                      : "No open run to resume"
                  }
                  onClick={() => startScanning("resume_previous")}
                >
                  Resume run
                </button>
                {!canResume && (
                  <span id="resume-run-hint" className="visually-hidden">
                    No open run to resume
                  </span>
                )}
              </>
            )}
            {minimizeToTray && (
              <button
                type="button"
                className="danger"
                title="Close the app completely (window close keeps running in the tray)"
                onClick={() => api.quitApp()}
              >
                Exit
              </button>
            )}
          </div>
        </div>
      </header>

      <AppUpdater autoCheck variant="banner" />

      {warning && (
        <div className="warning-banner" role="alert">
          Coin rate unavailable — the game is showing total coins, not
          coins/min. Snapshots keep the last known rate until /min returns.
        </div>
      )}

      <main>
        {tab === "dashboard" && <Dashboard event={scannerEvent} />}
        {tab === "history" && <History />}
        {tab === "settings" && <SettingsPage />}
      </main>
    </div>
  );
}
