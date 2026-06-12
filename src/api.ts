import { invoke } from "@tauri-apps/api/core";
import { listen, UnlistenFn } from "@tauri-apps/api/event";

export type GameMode =
  | "normal"
  | "total_coin"
  | "intro_sprint"
  | "tournament"
  | "end_of_run"
  | "unknown";

export interface LiveState {
  mode: GameMode;
  tier: number | null;
  wave: number | null;
  coin_per_minute: number | null;
  run_active: boolean;
  run_type: "normal" | "tournament" | null;
  total_coin_warning: boolean;
}

export interface ScannerEvent {
  status: string;
  live: LiveState | null;
  current_run_id: string | null;
}

export interface WindowInfo {
  title: string;
  app_name: string;
}

export interface Settings {
  target_window: { title_substring: string; process_name: string } | null;
  poll_interval_ms: number;
}

export interface RunRow {
  id: string;
  started_at: string;
  ended_at: string | null;
  run_type: string;
  peak_tier: number | null;
  final_wave: number | null;
  avg_coin_per_minute: number | null;
  snapshot_count: number;
  comment: string | null;
}

export interface SnapshotRow {
  wave: number;
  tier: number | null;
  coin_per_minute: number | null;
  recorded_at: string;
}

export interface RunFilter {
  run_type?: string;
  min_wave?: number;
  min_tier?: number;
  date_from?: string;
  date_to?: string;
}

export interface CaptureBurstResult {
  saved: number;
  coin_rate_detected: number;
  manifest_path: string;
  captured_dir: string;
}

export const api = {
  listWindows: () => invoke<WindowInfo[]>("list_windows"),
  getSettings: () => invoke<Settings>("get_settings"),
  saveSettings: (newSettings: Settings) =>
    invoke<void>("save_settings", { newSettings }),
  startScanner: () => invoke<void>("start_scanner"),
  stopScanner: () => invoke<void>("stop_scanner"),
  scannerRunning: () => invoke<boolean>("scanner_running"),
  liveState: () => invoke<LiveState>("live_state"),
  manualNewRun: () => invoke<void>("manual_new_run"),
  listRuns: (filter: RunFilter) => invoke<RunRow[]>("list_runs", { filter }),
  setRunComment: (runId: string, comment: string) =>
    invoke<void>("set_run_comment", { runId, comment }),
  deleteRuns: (runIds: string[]) =>
    invoke<number>("delete_runs", { runIds }),
  runSnapshots: (runId: string) =>
    invoke<SnapshotRow[]>("run_snapshots", { runId }),
  currentRunSnapshots: () => invoke<SnapshotRow[]>("current_run_snapshots"),
  exportCsv: () => invoke<string>("export_csv"),
  previewCapture: () => invoke<string>("preview_capture"),
  captureFixtureBurst: (count: number, intervalMs: number) =>
    invoke<CaptureBurstResult>("capture_fixture_burst", { count, intervalMs }),
  onScannerUpdate: (cb: (e: ScannerEvent) => void): Promise<UnlistenFn> =>
    listen<ScannerEvent>("scanner-update", (event) => cb(event.payload)),
};

/** Format a normalized coin/min back to game-style units (1230 -> "1.23K"). */
export function formatCoin(value: number | null): string {
  if (value === null || value === undefined) return "—";
  const suffixes = ["", "K", "M", "B", "T", "q", "Q", "s", "S", "O", "N", "D"];
  let idx = 0;
  let v = value;
  while (Math.abs(v) >= 1000 && idx < suffixes.length - 1) {
    v /= 1000;
    idx++;
  }
  const num = v >= 100 ? v.toFixed(1) : v.toFixed(2);
  return `${parseFloat(num)}${suffixes[idx]}`;
}
