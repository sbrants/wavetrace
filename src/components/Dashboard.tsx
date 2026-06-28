import { useEffect, useMemo, useRef, useState } from "react";
import { api, formatCoin, ScannerEvent, SnapshotRow, WaveSkipRow } from "../api";
import { snapshotsToChartData, buildChartWaveJumpMarkers } from "../chartData";
import {
  formatSkipLiveStat,
} from "../skipDisplay";
import ChartScreenshotActions from "./ChartScreenshotActions";
import CoinVsWaveChart from "./CoinVsWaveChart";

const SNAPSHOT_REFRESH_MS = 15_000;

export default function Dashboard({ event }: { event: ScannerEvent | null }) {
  const [snapshotTotal, setSnapshotTotal] = useState(0);
  const [chartSnapshots, setChartSnapshots] = useState<SnapshotRow[]>([]);
  const [skipTotal, setSkipTotal] = useState(0);
  const [chartWaveSkips, setChartWaveSkips] = useState<WaveSkipRow[]>([]);
  const [chartNormalJumps, setChartNormalJumps] = useState<number[]>([]);
  const chartRef = useRef<HTMLDivElement>(null);
  const lastFetchAtRef = useRef(0);
  const lastWaveRef = useRef<number | null>(null);
  const liveWaveRef = useRef<number | null>(null);
  liveWaveRef.current = event?.live?.wave ?? null;
  const live = event?.live ?? null;

  useEffect(() => {
    const refresh = (force = false) => {
      const wave = liveWaveRef.current;
      const now = Date.now();
      const waveChanged = wave !== null && wave !== lastWaveRef.current;
      const stale = now - lastFetchAtRef.current >= SNAPSHOT_REFRESH_MS;
      if (!force && !waveChanged && !stale && lastFetchAtRef.current > 0) {
        return;
      }
      lastFetchAtRef.current = now;
      lastWaveRef.current = wave;
      api
        .currentRunDashboard()
        .then((view) => {
          setSnapshotTotal(view.snapshot_total);
          setChartSnapshots(view.chart_snapshots);
          setSkipTotal(view.skip_total);
          setChartWaveSkips(view.chart_wave_skips);
          setChartNormalJumps(view.chart_normal_jumps);
        })
        .catch(() => {});
    };

    lastFetchAtRef.current = 0;
    lastWaveRef.current = null;
    refresh(true);

    let unlisten: (() => void) | undefined;
    void api.onScannerUpdate(() => refresh()).then((fn) => {
      unlisten = fn;
    });
    return () => unlisten?.();
  }, [event?.current_run_id]);

  const chartData = useMemo(
    () => snapshotsToChartData(chartSnapshots, { alreadySampled: true }),
    [chartSnapshots]
  );
  const skipMarkers = useMemo(
    () => buildChartWaveJumpMarkers(chartWaveSkips, chartNormalJumps),
    [chartWaveSkips, chartNormalJumps]
  );
  const lastSkipDisplay =
    live?.last_wave_delta != null
      ? live.last_skip_multiplier != null
        ? ({ kind: "multiplier", value: live.last_skip_multiplier } as const)
        : ({ kind: "delta", value: live.last_wave_delta } as const)
      : null;

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
          label="Run"
          value={
            live?.run_active
              ? live.run_type === "tournament"
                ? "Tournament"
                : "Farming"
              : "None"
          }
          badge={live?.run_type === "tournament"}
        />
      </div>

      <div className="chart-card" ref={chartRef}>
        <div className="chart-card-header">
          <div>
            <h3>Avg coin/min vs Wave (current run)</h3>
            <span className="muted">
              {snapshotTotal} snapshot{snapshotTotal === 1 ? "" : "s"} this run
              {snapshotTotal > chartSnapshots.length &&
                ` · chart shows ${chartSnapshots.length} sampled snapshot points`}
              {skipTotal > chartWaveSkips.length &&
                ` · ${chartWaveSkips.length} of ${skipTotal} skips sampled for chart`}
              {skipMarkers.length > 0 &&
                ` · ${skipMarkers.length} wave skip${skipMarkers.length === 1 ? "" : "s"} on chart`}
            </span>
          </div>
          <ChartScreenshotActions
            targetRef={chartRef}
            disabled={chartData.length === 0}
          />
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
