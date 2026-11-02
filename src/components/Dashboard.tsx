import { useEffect, useMemo, useRef, useState } from "react";
import {
  api,
  formatCoin,
  formatGoldenComboMultiplier,
  ScannerEvent,
} from "../api";
import {
  MAX_CHART_POINTS,
  snapshotsToChartData,
  buildWaveJumpMarkers,
} from "../chartData";
import {
  DATA_RESTORED_EVENT,
  EMPTY_RUN_ROWS,
  RUN_ROWS_EDITED_EVENT,
  RunRowsReader,
  type RunRows,
} from "../runRows";
import {
  formatSkipLiveStat,
} from "../skipDisplay";
import ChartScreenshotActions from "./ChartScreenshotActions";
import CoinVsWaveChart from "./CoinVsWaveChart";
import { formatRunType, runTypeUsesBadge } from "../runType";
import { usePersistedBoolean } from "../persistedState";
import { CoinIcon, WaveJumpIcon, GcActivationIcon } from "./ChartToggleIcons";

const SNAPSHOT_REFRESH_MS = 15_000;

export default function Dashboard({ event }: { event: ScannerEvent | null }) {
  const [rows, setRows] = useState<RunRows>(EMPTY_RUN_ROWS);
  const [showWaveJumps, setShowWaveJumps] = usePersistedBoolean(
    "dashboard.showWaveJumps",
    true
  );
  const [showCoinPerMinute, setShowCoinPerMinute] = usePersistedBoolean(
    "dashboard.showCoinPerMinute",
    true
  );
  const [showGcActivations, setShowGcActivations] = usePersistedBoolean(
    "dashboard.showGcActivations",
    true
  );
  const chartRef = useRef<HTMLDivElement>(null);
  const live = event?.live ?? null;
  const runId = event?.current_run_id ?? null;

  useEffect(() => {
    if (!runId) {
      setRows(EMPTY_RUN_ROWS);
      return;
    }
    let active = true;
    const reader = new RunRowsReader(runId);
    const show = (next: RunRows | null) => {
      if (active && next) setRows(next);
    };
    const reload = () => {
      reader.load().then(show).catch(() => {});
    };

    let lastFetchAt = Date.now();
    let lastWave: number | null = null;
    const refresh = (e: ScannerEvent) => {
      const wave = e.live?.wave ?? null;
      const now = Date.now();
      const waveChanged = wave !== null && wave !== lastWave;
      if (!waveChanged && now - lastFetchAt < SNAPSHOT_REFRESH_MS) return;
      lastFetchAt = now;
      lastWave = wave;
      reader.readNew().then(show).catch(() => {});
    };

    reload();
    let unlisten: (() => void) | undefined;
    void api.onScannerUpdate(refresh).then((fn) => {
      if (active) unlisten = fn;
      else fn();
    });
    window.addEventListener(RUN_ROWS_EDITED_EVENT, reload);
    window.addEventListener(DATA_RESTORED_EVENT, reload);
    return () => {
      active = false;
      unlisten?.();
      window.removeEventListener(RUN_ROWS_EDITED_EVENT, reload);
      window.removeEventListener(DATA_RESTORED_EVENT, reload);
    };
  }, [runId]);

  const chartData = useMemo(() => snapshotsToChartData(rows.snapshots), [rows.snapshots]);
  const skipMarkers = useMemo(
    () => buildWaveJumpMarkers(rows.snapshots, rows.wave_skips),
    [rows.snapshots, rows.wave_skips]
  );
  const snapshotTotal = rows.snapshots.length;
  const lastSkipDisplay =
    live?.last_wave_delta != null
      ? live.last_skip_multiplier != null
        ? ({ kind: "multiplier", value: live.last_wave_delta } as const)
        : ({ kind: "delta", value: live.last_wave_delta } as const)
      : null;

  const hasGcOnChart = chartData.some((d) => d.golden_combo_caret != null);
  const hasJumpsOnChart = skipMarkers.length > 0;

  return (
    <div className="dashboard">
      <div className="stat-cards" role="group" aria-label="Live run stats">
        <StatCard label="Tier" value={live?.tier?.toString() ?? "—"} />
        <StatCard label="Wave" value={live?.wave?.toString() ?? "—"} />
        <StatCard
          label="Coin/min"
          value={formatCoin(live?.coin_per_minute ?? null)}
          dimmed={live?.total_coin_warning ?? false}
          hint={live?.total_coin_warning ? "last known" : undefined}
          hintReserved
        />
        <StatCard
          label="Wave jump"
          value={
            lastSkipDisplay ? formatSkipLiveStat(lastSkipDisplay) : "—"
          }
          hintReserved
        />
        <StatCard
          label="GC activations"
          value={
            live?.golden_combo_caret != null
              ? `^${live.golden_combo_caret}`
              : "—"
          }
          hintReserved
        />
        <StatCard
          label="GC bonus"
          value={formatGoldenComboMultiplier(
            live?.golden_combo_multiplier ?? null
          )}
          hintReserved
        />
        <StatCard
          label="Run"
          value={
            live?.run_active ? formatRunType(live.run_type) : "None"
          }
          badge={live?.run_active && runTypeUsesBadge(live.run_type)}
        />
      </div>

      <div className="chart-card" ref={chartRef}>
        <div className="chart-card-header">
          <div>
            <h3>Coin/min vs Wave (current run)</h3>
            <span className="muted">
              {snapshotTotal} snapshot{snapshotTotal === 1 ? "" : "s"} this run
              {chartData.length === MAX_CHART_POINTS &&
                snapshotTotal > MAX_CHART_POINTS &&
                ` · chart shows ${MAX_CHART_POINTS} sampled snapshot points`}
              {" · saved automatically — every run's chart stays in History"}
            </span>
          </div>
          <div className="chart-card-actions">
            {chartData.length > 0 && (
              <button
                type="button"
                className={showCoinPerMinute ? "btn-icon active" : "btn-icon"}
                onClick={() => setShowCoinPerMinute((v) => !v)}
                aria-pressed={showCoinPerMinute}
                data-tooltip="Show coin/min series on the chart"
              >
                <CoinIcon />
                <span className="btn-icon-label">Coin/min</span>
              </button>
            )}
            {hasJumpsOnChart && (
              <button
                type="button"
                className={showWaveJumps ? "btn-icon active" : "btn-icon"}
                onClick={() => setShowWaveJumps((v) => !v)}
                aria-pressed={showWaveJumps}
                data-tooltip="Show wave jump markers on the chart"
              >
                <WaveJumpIcon />
                <span className="btn-icon-label">Wave jumps</span>
              </button>
            )}
            {hasGcOnChart && (
              <button
                type="button"
                className={showGcActivations ? "btn-icon active" : "btn-icon"}
                onClick={() => setShowGcActivations((v) => !v)}
                aria-pressed={showGcActivations}
                data-tooltip="Show Golden Combo activation (^) series on the chart"
              >
                <GcActivationIcon />
                <span className="btn-icon-label">GC activations</span>
              </button>
            )}
            <ChartScreenshotActions
              targetRef={chartRef}
              disabled={chartData.length === 0}
            />
          </div>
        </div>
        {chartData.length === 0 ? (
          <p className="muted">
            No data yet. Start a new run or resume the previous one from the header.
          </p>
        ) : (
          <CoinVsWaveChart
            mode="single"
            data={chartData}
            waveSkips={skipMarkers}
            showWaveJumps={showWaveJumps}
            showCoinPerMinute={showCoinPerMinute}
            showGoldenComboActivations={showGcActivations}
            height={320}
          />
        )}
      </div>
    </div>
  );
}

function StatCard({
  label,
  value,
  dimmed,
  hint,
  hintReserved,
  badge,
}: {
  label: string;
  value: string;
  dimmed?: boolean;
  hint?: string;
  /** Keep hint row height when hint is absent (avoids layout shift). */
  hintReserved?: boolean;
  badge?: boolean;
}) {
  const showHintRow = hintReserved || hint;

  return (
    <div className={`stat-card ${badge ? "stat-badge" : ""}`}>
      <span className="stat-label">{label}</span>
      <span className={`stat-value ${dimmed ? "dimmed" : ""}`}>{value}</span>
      {showHintRow && (
        <span
          className={`stat-hint ${hint ? "" : "stat-hint-empty"}`}
          aria-hidden={!hint}
        >
          {hint ?? (hintReserved ? "\u00a0" : "last known")}
        </span>
      )}
    </div>
  );
}
