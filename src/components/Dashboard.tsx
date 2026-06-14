import { useEffect, useRef, useState } from "react";
import { api, formatCoin, ScannerEvent, SnapshotRow } from "../api";
import { snapshotsToChartData } from "../chartData";
import ChartScreenshotActions from "./ChartScreenshotActions";
import CoinVsWaveChart from "./CoinVsWaveChart";

export default function Dashboard({ event }: { event: ScannerEvent | null }) {
  const [snapshots, setSnapshots] = useState<SnapshotRow[]>([]);
  const chartRef = useRef<HTMLDivElement>(null);
  const live = event?.live ?? null;

  useEffect(() => {
    api.currentRunSnapshots().then(setSnapshots).catch(() => {});
  }, [event?.current_run_id, live?.wave]);

  const chartData = snapshotsToChartData(snapshots);

  return (
    <div className="dashboard">
      <div className="stat-cards">
        <StatCard label="Tier" value={live?.tier?.toString() ?? "—"} />
        <StatCard label="Wave" value={live?.wave?.toString() ?? "—"} />
        <StatCard
          label="Coin/min"
          value={formatCoin(live?.coin_per_minute ?? null)}
          dimmed={live?.total_coin_warning ?? false}
          hint={live?.total_coin_warning ? "last known" : undefined}
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
              {snapshots.length} snapshot{snapshots.length === 1 ? "" : "s"} this run
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
          <CoinVsWaveChart mode="single" data={chartData} height={320} />
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
  badge,
}: {
  label: string;
  value: string;
  dimmed?: boolean;
  hint?: string;
  badge?: boolean;
}) {
  return (
    <div className={`stat-card ${badge ? "stat-badge" : ""}`}>
      <span className="stat-label">{label}</span>
      <span className={`stat-value ${dimmed ? "dimmed" : ""}`}>{value}</span>
      {hint && <span className="stat-hint">{hint}</span>}
    </div>
  );
}
