import { useEffect, useRef, useState } from "react";
import {
  LineChart,
  Line,
  XAxis,
  YAxis,
  Tooltip,
  CartesianGrid,
  ResponsiveContainer,
} from "recharts";
import { api, formatCoin, ScannerEvent, SnapshotRow } from "../api";
import ChartScreenshotActions from "./ChartScreenshotActions";

export default function Dashboard({ event }: { event: ScannerEvent | null }) {
  const [snapshots, setSnapshots] = useState<SnapshotRow[]>([]);
  const chartRef = useRef<HTMLDivElement>(null);
  const live = event?.live ?? null;

  useEffect(() => {
    // Refresh the chart whenever the scanner reports progress.
    api.currentRunSnapshots().then(setSnapshots).catch(() => {});
  }, [event?.current_run_id, live?.wave]);

  const data = snapshots
    .filter((s) => s.coin_per_minute !== null)
    .map((s) => ({ wave: s.wave, coin: s.coin_per_minute as number }));

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

      <div className="toolbar">
        <button onClick={() => api.manualNewRun().catch((e) => alert(e))}>
          New Run
        </button>
        <span className="muted">
          {snapshots.length} snapshot{snapshots.length === 1 ? "" : "s"} this run
        </span>
      </div>

      <div className="chart-card" ref={chartRef}>
        <div className="chart-card-header">
          <h3>Avg coin/min vs Wave (current run)</h3>
          <ChartScreenshotActions
            targetRef={chartRef}
            disabled={data.length === 0}
          />
        </div>
        {data.length === 0 ? (
          <p className="muted">
            No data yet. Start the scanner and let a run reach wave 1.
          </p>
        ) : (
          <ResponsiveContainer width="100%" height={320}>
            <LineChart data={data}>
              <CartesianGrid strokeDasharray="3 3" stroke="#2a3550" />
              <XAxis dataKey="wave" stroke="#8da2c0" />
              <YAxis
                stroke="#8da2c0"
                tickFormatter={(v: number) => formatCoin(v)}
                width={70}
              />
              <Tooltip
                formatter={(v) => formatCoin(v as number)}
                labelFormatter={(l) => `Wave ${l}`}
                contentStyle={{ background: "#16203a", border: "1px solid #2a3550" }}
              />
              <Line
                type="monotone"
                dataKey="coin"
                stroke="#4cc2ff"
                dot={false}
                strokeWidth={2}
              />
            </LineChart>
          </ResponsiveContainer>
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
