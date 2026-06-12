import { useCallback, useEffect, useRef, useState } from "react";
import {
  LineChart,
  Line,
  XAxis,
  YAxis,
  Tooltip,
  CartesianGrid,
  ResponsiveContainer,
  Legend,
} from "recharts";
import { api, formatCoin, RunFilter, RunRow, SnapshotRow } from "../api";
import ChartScreenshotActions from "./ChartScreenshotActions";

type SortKey = "started_at" | "final_wave" | "peak_tier" | "avg_coin_per_minute";

const COMPARE_COLORS = [
  "#4cc2ff",
  "#6fdd8b",
  "#e8b339",
  "#ff7eb6",
  "#b388ff",
  "#ff9f68",
  "#7ee8d6",
  "#c9a0ff",
];

const PAGE_SIZES = [5, 10, 25, 50, 100] as const;

export default function History() {
  const [runs, setRuns] = useState<RunRow[]>([]);
  const [filter, setFilter] = useState<RunFilter>({});
  const [sortKey, setSortKey] = useState<SortKey>("started_at");
  const [sortAsc, setSortAsc] = useState(false);
  const [selected, setSelected] = useState<RunRow | null>(null);
  const [checked, setChecked] = useState<Set<string>>(new Set());
  const [snapshots, setSnapshots] = useState<SnapshotRow[]>([]);
  const [compareRuns, setCompareRuns] = useState<RunRow[]>([]);
  const [compareSnapshots, setCompareSnapshots] = useState<
    Record<string, SnapshotRow[]>
  >({});
  const [compareLoading, setCompareLoading] = useState(false);
  const [page, setPage] = useState(1);
  const [pageSize, setPageSize] = useState<number>(25);
  const chartRef = useRef<HTMLDivElement>(null);
  const compareChartRef = useRef<HTMLDivElement>(null);

  const reload = useCallback(() => {
    api.listRuns(filter).then(setRuns).catch(() => {});
  }, [filter]);

  useEffect(reload, [reload]);

  useEffect(() => {
    setPage(1);
  }, [filter, pageSize]);

  const updateComment = useCallback(async (runId: string, value: string) => {
    setRuns((prev) =>
      prev.map((r) =>
        r.id === runId ? { ...r, comment: value || null } : r
      )
    );
    try {
      await api.setRunComment(runId, value);
    } catch (e) {
      alert(String(e));
      reload();
    }
  }, [reload]);

  useEffect(() => {
    if (selected) {
      api.runSnapshots(selected.id).then(setSnapshots).catch(() => {});
    } else {
      setSnapshots([]);
    }
  }, [selected]);

  const sorted = [...runs].sort((a, b) => {
    const av = a[sortKey] ?? "";
    const bv = b[sortKey] ?? "";
    const cmp = av < bv ? -1 : av > bv ? 1 : 0;
    return sortAsc ? cmp : -cmp;
  });

  const totalRuns = sorted.length;
  const totalPages = Math.max(1, Math.ceil(totalRuns / pageSize));
  const safePage = Math.min(page, totalPages);
  const pageStart = (safePage - 1) * pageSize;
  const pageRuns = sorted.slice(pageStart, pageStart + pageSize);
  const rangeStart = totalRuns === 0 ? 0 : pageStart + 1;
  const rangeEnd = Math.min(pageStart + pageSize, totalRuns);

  useEffect(() => {
    if (page !== safePage) {
      setPage(safePage);
    }
  }, [page, safePage]);

  const pageIds = pageRuns.map((r) => r.id);
  const allPageChecked =
    pageIds.length > 0 && pageIds.every((id) => checked.has(id));

  const toggleSort = (key: SortKey) => {
    if (key === sortKey) setSortAsc(!sortAsc);
    else {
      setSortKey(key);
      setSortAsc(false);
    }
  };

  const toggleChecked = (id: string) => {
    setChecked((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  const toggleAll = () => {
    setChecked((prev) => {
      const next = new Set(prev);
      if (allPageChecked) {
        for (const id of pageIds) next.delete(id);
      } else {
        for (const id of pageIds) next.add(id);
      }
      return next;
    });
  };

  const deleteSelected = async () => {
    if (checked.size === 0) return;
    const n = checked.size;
    if (!confirm(`Delete ${n} run${n === 1 ? "" : "s"} and their snapshots?`)) {
      return;
    }
    try {
      await api.deleteRuns([...checked]);
      if (selected && checked.has(selected.id)) {
        setSelected(null);
      }
      setChecked(new Set());
      reload();
    } catch (e) {
      alert(String(e));
    }
  };

  const compareSelected = async () => {
    if (checked.size < 2) return;
    const ids = [...checked];
    const runsToCompare = sorted.filter((r) => ids.includes(r.id));
    setCompareLoading(true);
    try {
      const entries = await Promise.all(
        ids.map(async (id) => [id, await api.runSnapshots(id)] as const)
      );
      setCompareSnapshots(Object.fromEntries(entries));
      setCompareRuns(runsToCompare);
    } catch (e) {
      alert(String(e));
    } finally {
      setCompareLoading(false);
    }
  };

  const clearCompare = () => {
    setCompareRuns([]);
    setCompareSnapshots({});
  };

  const combineSelected = async () => {
    if (checked.size < 2) return;
    const n = checked.size;
    if (
      !confirm(
        `Combine ${n} runs into one? Runs are ordered by start time. Waves must increase across the combined timeline. Source runs will be removed.`
      )
    ) {
      return;
    }
    try {
      const newId = await api.combineRuns([...checked]);
      setChecked(new Set());
      const updated = await api.listRuns(filter);
      setRuns(updated);
      const combined = updated.find((r) => r.id === newId) ?? null;
      setSelected(combined);
    } catch (e) {
      alert(String(e));
    }
  };

  const exportCsv = async () => {
    try {
      const path = await api.exportCsv();
      alert(`Exported to:\n${path}`);
    } catch (e) {
      alert(e);
    }
  };

  const chartData = snapshots
    .filter((s) => s.coin_per_minute !== null)
    .map((s) => ({ wave: s.wave, coin: s.coin_per_minute as number }));

  const compareChartData = buildCompareChartData(compareRuns, compareSnapshots);

  return (
    <div className="history">
      <div className="toolbar">
        <select
          value={filter.run_type ?? ""}
          onChange={(e) =>
            setFilter({ ...filter, run_type: e.target.value || undefined })
          }
        >
          <option value="">All run types</option>
          <option value="farming">Farming</option>
          <option value="tournament">Tournament</option>
        </select>
        <input
          type="number"
          placeholder="Min wave"
          onChange={(e) =>
            setFilter({
              ...filter,
              min_wave: e.target.value ? Number(e.target.value) : undefined,
            })
          }
        />
        <input
          type="number"
          placeholder="Min tier"
          onChange={(e) =>
            setFilter({
              ...filter,
              min_tier: e.target.value ? Number(e.target.value) : undefined,
            })
          }
        />
        <button onClick={reload}>Refresh</button>
        <button onClick={exportCsv}>Export CSV</button>
        <button
          disabled={checked.size < 2 || compareLoading}
          onClick={compareSelected}
        >
          {compareLoading ? "Loading…" : `Compare selected (${checked.size})`}
        </button>
        <button
          disabled={checked.size < 2}
          onClick={combineSelected}
        >
          Combine selected ({checked.size})
        </button>
        <button
          className="danger"
          disabled={checked.size === 0}
          onClick={deleteSelected}
        >
          Delete selected ({checked.size})
        </button>
      </div>

      <div className="history-table-wrap">
      <table>
        <thead>
          <tr>
            <th className="check-col">
              <input
                type="checkbox"
                checked={allPageChecked}
                onChange={toggleAll}
                aria-label="Select all runs on this page"
              />
            </th>
            <th onClick={() => toggleSort("started_at")}>Started</th>
            <th>Duration</th>
            <th>Type</th>
            <th onClick={() => toggleSort("peak_tier")}>Tier</th>
            <th onClick={() => toggleSort("final_wave")}>Final wave</th>
            <th onClick={() => toggleSort("avg_coin_per_minute")}>
              Avg coin/min
            </th>
            <th>Snapshots</th>
            <th>Comment</th>
          </tr>
        </thead>
        <tbody>
          {pageRuns.map((r) => (
            <tr
              key={r.id}
              className={selected?.id === r.id ? "selected" : ""}
              onClick={() => setSelected(r)}
            >
              <td className="check-col" onClick={(e) => e.stopPropagation()}>
                <input
                  type="checkbox"
                  checked={checked.has(r.id)}
                  onChange={() => toggleChecked(r.id)}
                  aria-label={`Select run ${r.id}`}
                />
              </td>
              <td>{new Date(r.started_at).toLocaleString()}</td>
              <td>{duration(r)}</td>
              <td>
                {r.run_type === "tournament" ? (
                  <span className="badge">tournament</span>
                ) : (
                  "farming"
                )}
              </td>
              <td>{r.peak_tier ?? "—"}</td>
              <td>{r.final_wave ?? "—"}</td>
              <td>{formatCoin(r.avg_coin_per_minute)}</td>
              <td>{r.snapshot_count}</td>
              <td className="comment-col" onClick={(e) => e.stopPropagation()}>
                <input
                  type="text"
                  className="comment-input"
                  value={r.comment ?? ""}
                  placeholder="Add comment…"
                  onChange={(e) => updateComment(r.id, e.target.value)}
                  aria-label={`Comment for run ${r.id}`}
                />
              </td>
            </tr>
          ))}
          {totalRuns === 0 && (
            <tr>
              <td colSpan={9} className="muted">
                No runs recorded yet.
              </td>
            </tr>
          )}
        </tbody>
      </table>
      </div>

      {totalRuns > 0 && (
        <div className="history-pagination">
          <span className="muted">
            Showing {rangeStart}–{rangeEnd} of {totalRuns}
          </span>
          <div className="history-pagination-controls">
            <label className="page-size-label">
              Per page
              <select
                value={pageSize}
                onChange={(e) => setPageSize(Number(e.target.value))}
              >
                {PAGE_SIZES.map((n) => (
                  <option key={n} value={n}>
                    {n}
                  </option>
                ))}
              </select>
            </label>
            <button
              disabled={safePage <= 1}
              onClick={() => setPage((p) => Math.max(1, p - 1))}
            >
              Previous
            </button>
            <span className="page-indicator">
              Page {safePage} of {totalPages}
            </span>
            <button
              disabled={safePage >= totalPages}
              onClick={() => setPage((p) => Math.min(totalPages, p + 1))}
            >
              Next
            </button>
          </div>
        </div>
      )}

      {compareRuns.length >= 2 && (
        <div className="chart-card compare-card" ref={compareChartRef}>
          <div className="chart-card-header">
            <h3>Compare {compareRuns.length} runs — coin/min vs wave</h3>
            <div className="chart-card-actions">
              <button onClick={clearCompare}>Clear comparison</button>
              <ChartScreenshotActions
                targetRef={compareChartRef}
                disabled={compareChartData.length === 0}
              />
            </div>
          </div>
          <table className="compare-summary">
            <thead>
              <tr>
                <th>Run</th>
                <th>Duration</th>
                <th>Type</th>
                <th>Peak tier</th>
                <th>Final wave</th>
                <th>Avg coin/min</th>
                <th>Snapshots</th>
              </tr>
            </thead>
            <tbody>
              {compareRuns.map((r, i) => (
                <tr key={r.id}>
                  <td>
                    <span
                      className="compare-swatch"
                      style={{ background: COMPARE_COLORS[i % COMPARE_COLORS.length] }}
                    />
                    {runShortLabel(r)}
                  </td>
                  <td>{duration(r)}</td>
                  <td>
                    {r.run_type === "tournament" ? (
                      <span className="badge">tournament</span>
                    ) : (
                      "farming"
                    )}
                  </td>
                  <td>{r.peak_tier ?? "—"}</td>
                  <td>{r.final_wave ?? "—"}</td>
                  <td>{formatCoin(r.avg_coin_per_minute)}</td>
                  <td>{r.snapshot_count}</td>
                </tr>
              ))}
            </tbody>
          </table>
          <ResponsiveContainer width="100%" height={320}>
            <LineChart data={compareChartData}>
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
              <Legend />
              {compareRuns.map((r, i) => (
                <Line
                  key={r.id}
                  type="monotone"
                  dataKey={`coin_${i}`}
                  name={runShortLabel(r)}
                  stroke={COMPARE_COLORS[i % COMPARE_COLORS.length]}
                  dot={false}
                  strokeWidth={2}
                  connectNulls
                />
              ))}
            </LineChart>
          </ResponsiveContainer>
        </div>
      )}

      {selected && compareRuns.length < 2 && (
        <div className="chart-card" ref={chartRef}>
          <div className="chart-card-header">
            <h3>
              Run {new Date(selected.started_at).toLocaleString()} — avg coin/min vs
              wave
            </h3>
            <ChartScreenshotActions
              targetRef={chartRef}
              disabled={chartData.length === 0}
            />
          </div>
          <ResponsiveContainer width="100%" height={300}>
            <LineChart data={chartData}>
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
        </div>
      )}
    </div>
  );
}

function duration(r: RunRow): string {
  if (!r.ended_at) return "ongoing";
  const ms = +new Date(r.ended_at) - +new Date(r.started_at);
  const m = Math.floor(ms / 60000);
  return m >= 60 ? `${Math.floor(m / 60)}h ${m % 60}m` : `${m}m`;
}

function runShortLabel(r: RunRow): string {
  const date = new Date(r.started_at).toLocaleString(undefined, {
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
  const wave = r.final_wave ?? "?";
  const tier = r.peak_tier ?? "?";
  return `${date} (T${tier} W${wave})`;
}

function buildCompareChartData(
  runs: RunRow[],
  snapshots: Record<string, SnapshotRow[]>
): { wave: number; [key: string]: number | null }[] {
  const waves = new Set<number>();
  for (const run of runs) {
    for (const s of snapshots[run.id] ?? []) {
      if (s.coin_per_minute !== null) {
        waves.add(s.wave);
      }
    }
  }
  return [...waves]
    .sort((a, b) => a - b)
    .map((wave) => {
      const row: { wave: number; [key: string]: number | null } = { wave };
      runs.forEach((run, i) => {
        const snap = (snapshots[run.id] ?? []).find((s) => s.wave === wave);
        row[`coin_${i}`] = snap?.coin_per_minute ?? null;
      });
      return row;
    });
}
