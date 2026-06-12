import { useEffect, useState } from "react";
import { api, ScannerEvent } from "./api";
import Dashboard from "./components/Dashboard";
import History from "./components/History";
import SettingsPage from "./components/SettingsPage";

type Tab = "dashboard" | "history" | "settings";

export default function App() {
  const [tab, setTab] = useState<Tab>("dashboard");
  const [scannerEvent, setScannerEvent] = useState<ScannerEvent | null>(null);
  const [running, setRunning] = useState(false);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    api.onScannerUpdate((e) => {
      setScannerEvent(e);
      setRunning(e.status !== "stopped");
    }).then((fn) => (unlisten = fn));
    api.scannerRunning().then(setRunning);
    return () => unlisten?.();
  }, []);

  const warning = scannerEvent?.live?.total_coin_warning ?? false;

  return (
    <div className="app">
      <header>
        <h1>TowerRun</h1>
        <nav>
          {(["dashboard", "history", "settings"] as Tab[]).map((t) => (
            <button
              key={t}
              className={tab === t ? "active" : ""}
              onClick={() => setTab(t)}
            >
              {t[0].toUpperCase() + t.slice(1)}
            </button>
          ))}
        </nav>
        <div className="header-right">
          <span className={`status status-${scannerEvent?.status ?? "stopped"}`}>
            {running ? scannerEvent?.status ?? "starting…" : "stopped"}
          </span>
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
            <button
              className="primary"
              onClick={() =>
                api
                  .startScanner()
                  .then(() => {
                    setRunning(true);
                    setScannerEvent((prev) => ({
                      status: "starting",
                      live: prev?.live ?? null,
                      current_run_id: prev?.current_run_id ?? null,
                    }));
                  })
                  .catch((e) => alert(String(e)))
              }
            >
              Start scanning
            </button>
          )}
        </div>
      </header>

      {warning && (
        <div className="warning-banner">
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
