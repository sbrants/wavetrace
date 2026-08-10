import { useCallback, useEffect, useRef, useState } from "react";
import { api, GameSaveStatus, ScanStartMode, ScannerEvent } from "./api";
import { reportUiError } from "./uiError";
import { showToast } from "./toast";
import Dashboard from "./components/Dashboard";
import History from "./components/History";
import SettingsPage from "./components/SettingsPage";
import AppUpdater from "./components/AppUpdater";
import CaptureOutageBanner from "./components/CaptureOutageBanner";
import ToastStack from "./components/ToastStack";
import ConfirmDialog from "./components/ConfirmDialog";
import { registerTabControl, type AppTab } from "./tabCapture";
import ExternalLink from "./ExternalLink";
import { DISCORD_SUPPORT_URL, DiscordIcon } from "./support";

type Tab = "dashboard" | "history" | "settings";

function scannerStatusLabel(
  running: boolean,
  status: string | undefined
): string {
  if (!running) return "Scanner stopped";
  switch (status) {
    case "scanning":
      return "scanning";
    case "starting":
      return "Scanner starting";
    case "window_not_found":
      return "Game window not found";
    case "window_minimized":
      return "Game window minimized";
    case "capture_stalled":
      return "Screen capture not responding";
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
  const [savePullEnabled, setSavePullEnabled] = useState(true);
  const [savePullAuto, setSavePullAuto] = useState(false);
  const [gameSaveStatus, setGameSaveStatus] = useState<GameSaveStatus | null>(
    null
  );
  const [savePullBusy, setSavePullBusy] = useState(false);
  const tabRef = useRef<Tab>(tab);
  const debugReturnTabRef = useRef<Tab | null>(null);
  const debugAwaitingTabRenderRef = useRef(false);

  useEffect(() => {
    tabRef.current = tab;
  }, [tab]);

  useEffect(() => {
    registerTabControl(setTab, () => tabRef.current);
  }, []);

  useEffect(() => {
    if (!debugAwaitingTabRenderRef.current) {
      return;
    }
    debugAwaitingTabRenderRef.current = false;
    window.requestAnimationFrame(() => {
      window.requestAnimationFrame(() => {
        window.dispatchEvent(new Event("wavetrace-debug-tab-ready"));
      });
    });
  }, [tab]);

  useEffect(() => {
    const notifyReady = () => {
      window.requestAnimationFrame(() => {
        window.requestAnimationFrame(() => {
          window.dispatchEvent(new Event("wavetrace-debug-tab-ready"));
        });
      });
    };

    const onDebugCapture = (event: Event) => {
      const detail = (event as CustomEvent<{
        phase: "start" | "switch" | "end";
        tab?: AppTab;
      }>).detail;

      if (detail.phase === "start") {
        debugReturnTabRef.current = tabRef.current;
        notifyReady();
        return;
      }
      if (detail.phase === "switch" && detail.tab) {
        // setTab is a no-op when already on that tab, so the [tab] effect
        // never fires — signal ready immediately in that case.
        if (detail.tab === tabRef.current) {
          notifyReady();
          return;
        }
        debugAwaitingTabRenderRef.current = true;
        setTab(detail.tab);
        return;
      }
      if (detail.phase === "end") {
        const restore = debugReturnTabRef.current;
        debugReturnTabRef.current = null;
        if (restore && restore !== tabRef.current) {
          debugAwaitingTabRenderRef.current = true;
          setTab(restore);
        } else {
          notifyReady();
        }
      }
    };

    window.addEventListener("wavetrace-debug-capture", onDebugCapture);
    return () =>
      window.removeEventListener("wavetrace-debug-capture", onDebugCapture);
  }, []);

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
    let cancelled = false;
    const refresh = () => {
      api
        .getSettings()
        .then((s) => {
          if (cancelled) return;
          const enabled = s.save_pull_enabled;
          const auto = s.save_pull_auto;
          setSavePullEnabled(enabled);
          setSavePullAuto(auto);
          setMinimizeToTray(s.minimize_to_tray);
          if (!enabled) {
            setGameSaveStatus(null);
            return;
          }
          return api.gameSaveStatus().then((status) => {
            if (!cancelled) setGameSaveStatus(status);
          });
        })
        .catch(() => {
          if (!cancelled) {
            setGameSaveStatus({
              ready: false,
              adbPath: null,
              deviceSerial: null,
              detail: "Could not check ADB status",
            });
          }
        });
    };
    refresh();
    const id = window.setInterval(refresh, 12_000);
    const onFocus = () => refresh();
    const onSettingsSaved = () => refresh();
    window.addEventListener("focus", onFocus);
    window.addEventListener("wavetrace-settings-saved", onSettingsSaved);
    return () => {
      cancelled = true;
      window.clearInterval(id);
      window.removeEventListener("focus", onFocus);
      window.removeEventListener("wavetrace-settings-saved", onSettingsSaved);
    };
  }, []);

  const downloadGameSave = () => {
    if (savePullBusy) return;
    setSavePullBusy(true);
    api
      .pullGameSave()
      .then((result) => {
        if (result.written) {
          showToast(
            `Saved playerInfo.dat (${result.bytes.toLocaleString()} bytes)`
          );
        } else {
          showToast("Save unchanged — file not overwritten");
        }
        api.gameSaveStatus().then(setGameSaveStatus).catch(() => {});
      })
      .catch((e) => reportUiError(e, "App.pullGameSave"))
      .finally(() => setSavePullBusy(false));
  };

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
      .catch((e) => reportUiError(e, "App.startScanner"));
  };

  const warning = scannerEvent?.live?.total_coin_warning ?? false;
  const showDownloadSave =
    savePullEnabled &&
    !savePullAuto &&
    (savePullBusy || (gameSaveStatus?.ready ?? false));

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
          <ExternalLink
            className="header-support"
            href={DISCORD_SUPPORT_URL}
            title="Ask for help in the WaveTrace Discord channel"
          >
            <DiscordIcon />
            Discord
          </ExternalLink>
          {showDownloadSave && (
            <button
              type="button"
              className="header-download-save"
              disabled={savePullBusy}
              title={
                gameSaveStatus?.deviceSerial
                  ? `Pull playerInfo.dat from ${gameSaveStatus.deviceSerial}`
                  : "Pull playerInfo.dat from the emulator"
              }
              onClick={downloadGameSave}
            >
              {savePullBusy ? "Downloading…" : "Download save"}
            </button>
          )}
        </nav>
        {warning && (
          <span
            className="header-coin-warning"
            role="status"
            aria-live="polite"
            title="Coin/min is unavailable (total coins, unreadable OCR, or similar). Snapshots keep the last known rate until /min returns."
          >
            Coin rate unavailable
          </span>
        )}
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
      <CaptureOutageBanner running={running} status={scannerEvent?.status} />

      <main>
        <div hidden={tab !== "dashboard"}>
          <Dashboard event={scannerEvent} />
        </div>
        <div hidden={tab !== "history"}>
          <History />
        </div>
        <div hidden={tab !== "settings"}>
          <SettingsPage />
        </div>
      </main>
      <ToastStack />
      <ConfirmDialog />
    </div>
  );
}
