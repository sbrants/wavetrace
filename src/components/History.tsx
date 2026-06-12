import { useCallback, useEffect, useState } from "react";
import {
  LineChart,
  Line,
  XAxis,
  YAxis,
  Tooltip,
  CartesianGrid,
  ResponsiveContainer,
} from "recharts";
import { api, formatCoin, RunFilter, RunRow, SnapshotRow } from "../api";

type SortKey = "started_at" | "final_wave" | "peak_tier" | "avg_coin_per_minute";

export default function History() {
  const [runs, setRuns] = useState<RunRow[]>([]);
  const [filter, setFilter] = useState<RunFilter>({});
  const [sortKey, setSortKey] = useState<SortKey>("started_at");
  const [sortAsc, setSortAsc] = useState(false);
  const [selected, setSelected] = useState<RunRow | null>(null);
  const [checked, setChecked] = useState<Set<string>>(new Set());
  const [snapshots, setSnapshots] = useState<SnapshotRow[]>([]);

  const reload = useCallback(() => {
    api.listRuns(filter).then(setRuns).catch(() => {});
  }, [filter]);

  useEffect(reload, [reload]);

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
    if (checked.size === sorted.length) {
      setChecked(new Set());
    } else {
      setChecked(new Set(sorted.map((r) => r.id)));
    }
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

  const exportCsv = async () => {
    try {
      const path = await api.exportCsv();
      alert(`Exported to:\n${path}`);
    } catch (e) {
      alert(e);
    }
  };

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
          <option value="normal">Normal</option>
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
                checked={sorted.length > 0 && checked.size === sorted.length}
                onChange={toggleAll}
                aria-label="Select all runs"
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
          {sorted.map((r) => (
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
                  "normal"
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
          {sorted.length === 0 && (
            <tr>
              <td colSpan={9} className="muted">
                No runs recorded yet.
              </td>
            </tr>
          )}
        </tbody>
      </table>
      </div>

      {selected && (
        <div className="chart-card">
          <h3>
            Run {new Date(selected.started_at).toLocaleString()} — coin/min vs
            wave
          </h3>
          <ResponsiveContainer width="100%" height={300}>
            <LineChart
              data={snapshots
                .filter((s) => s.coin_per_minute !== null)
                .map((s) => ({ wave: s.wave, coin: s.coin_per_minute }))}
            >
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
