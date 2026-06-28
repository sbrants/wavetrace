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
  run_type: "farming" | "tournament" | null;
  total_coin_warning: boolean;
  last_skip_multiplier: number | null;
  last_wave_delta: number | null;
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

/** macOS gates window titles + capture behind Screen Recording; other OSes don't. */
export type ScreenCaptureAccess = "granted" | "denied" | "not_required";

export interface Settings {
  target_window: { title_substring: string; process_name: string } | null;
  poll_interval_ms: number;
  minimize_to_tray: boolean;
  notify_run_ended: boolean;
  notify_window_lost: boolean;
  notify_wave_every: number | null;
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
  id: string;
  wave: number;
  tier: number | null;
  coin_per_minute: number | null;
  recorded_at: string;
}

export interface WaveSkipRow {
  id: string;
  at_wave: number;
  skipped_count: number;
  skip_multiplier: number | null;
  coin_per_minute: number | null;
  recorded_at: string;
}

/** Chart-safe payload (snapshots and skips downsampled independently on the Rust side). */
export interface DashboardRunView {
  snapshot_total: number;
  chart_snapshots: SnapshotRow[];
  skip_total: number;
  chart_wave_skips: WaveSkipRow[];
  chart_normal_jumps: number[];
}

export interface RunFilter {
  run_type?: string;
  min_wave?: number;
  min_tier?: number;
  date_from?: string;
  date_to?: string;
}

export interface ScannerLogView {
  path: string;
  lines: string[];
  total_lines: number;
  truncated: boolean;
  log_tail_truncated: boolean;
}

export interface CaptureBurstResult {
  saved: number;
  coin_rate_detected: number;
  manifest_path: string;
  captured_dir: string;
}

export interface OcrProbeResult {
  window_found: boolean;
  target_substring: string;
  width: number;
  height: number;
  elapsed_ms: number;
  all_lines: string[];
  coin_lines: string[];
  tier_wave_lines: string[];
  mode_lines: string[];
  tier: number | null;
  wave: number | null;
  coin_per_minute: number | null;
  coin_status: string;
  mode: string;
  preview_png_base64: string | null;
}

export type ScanStartMode = "new_run" | "resume_previous";

export interface CsvExport {
  filename: string;
  content: string;
  run_count: number;
  snapshot_count: number;
}

export interface WorkbookExport {
  filename: string;
  data_base64: string;
  run_count: number;
  snapshot_count: number;
}

export interface BackupExport {
  filename: string;
  data_base64: string;
  run_count: number;
  snapshot_count: number;
}

export interface BackupRestore {
  run_count: number;
  snapshot_count: number;
  safety_copy_path: string | null;
  backup_created_at: string | null;
  backup_app_version: string | null;
}

export const api = {
  quitApp: () => invoke<void>("quit_app"),
  listWindows: () => invoke<WindowInfo[]>("list_windows"),
  screenCaptureAccess: () =>
    invoke<ScreenCaptureAccess>("screen_capture_access"),
  requestScreenCaptureAccess: () =>
    invoke<ScreenCaptureAccess>("request_screen_capture_access"),
  openScreenRecordingSettings: () =>
    invoke<void>("open_screen_recording_settings"),
  getSettings: () => invoke<Settings>("get_settings"),
  saveSettings: (newSettings: Settings) =>
    invoke<void>("save_settings", { newSettings }),
  hasResumableRun: () => invoke<boolean>("has_resumable_run"),
  startScanner: (mode: ScanStartMode) =>
    invoke<void>("start_scanner", { mode }),
  stopScanner: () => invoke<void>("stop_scanner"),
  scannerRunning: () => invoke<boolean>("scanner_running"),
  liveState: () => invoke<LiveState>("live_state"),
  manualNewRun: () => invoke<void>("manual_new_run"),
  listRuns: (filter: RunFilter) => invoke<RunRow[]>("list_runs", { filter }),
  setRunComment: (runId: string, comment: string) =>
    invoke<void>("set_run_comment", { runId, comment }),
  deleteRuns: (runIds: string[]) =>
    invoke<number>("delete_runs", { runIds }),
  deleteSnapshot: (snapshotId: string) =>
    invoke<void>("delete_snapshot", { snapshotId }),
  deleteSnapshots: (snapshotIds: string[]) =>
    invoke<number>("delete_snapshots", { snapshotIds }),
  deleteWaveSkips: (waveSkipIds: string[]) =>
    invoke<number>("delete_wave_skips", { waveSkipIds }),
  deleteWaveSkip: (waveSkipId: string) =>
    invoke<void>("delete_wave_skip", { waveSkipId }),
  combineRuns: (runIds: string[]) =>
    invoke<string>("combine_runs", { runIds }),
  runSnapshots: (runId: string) =>
    invoke<SnapshotRow[]>("run_snapshots", { runId }),
  currentRunSnapshots: () => invoke<SnapshotRow[]>("current_run_snapshots"),
  currentRunDashboard: () => invoke<DashboardRunView>("current_run_dashboard"),
  runDashboardData: (runId: string) =>
    invoke<DashboardRunView>("run_dashboard_data", { runId }),
  runWaveSkips: (runId: string) =>
    invoke<WaveSkipRow[]>("run_wave_skips", { runId }),
  currentRunWaveSkips: () => invoke<WaveSkipRow[]>("current_run_wave_skips"),
  exportCsv: (filter: RunFilter) =>
    invoke<CsvExport>("export_csv", { filter }),
  exportWorkbook: (filter: RunFilter) =>
    invoke<WorkbookExport>("export_workbook", { filter }),
  exportBackup: () => invoke<BackupExport>("export_backup"),
  restoreBackup: (dataBase64: string) =>
    invoke<BackupRestore>("restore_backup", { dataBase64 }),
  readScannerLog: (maxLines: number) =>
    invoke<ScannerLogView>("read_scanner_log", { maxLines }),
  previewCapture: () => invoke<string>("preview_capture"),
  probeOcr: () => invoke<OcrProbeResult>("probe_ocr"),
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
