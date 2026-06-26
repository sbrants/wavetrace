import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api, formatCoin, RunFilter, RunRow, SnapshotRow, WaveSkipRow } from "../api";
import {
  buildCompareChartDataByProgress,
  buildCompareChartDataByWave,
  CompareXAxis,
  snapshotsToChartData,
  waveSkipsToMarkers,
} from "../chartData";
import { downloadBase64File, downloadTextFile } from "../exportDownload";
import ChartScreenshotActions from "./ChartScreenshotActions";
import CoinVsWaveChart, { ChartLineConfig } from "./CoinVsWaveChart";
import SkipCoinAnalytics from "./SkipCoinAnalytics";

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
  const [dateFrom, setDateFrom] = useState("");
  const [dateTo, setDateTo] = useState("");
  const [sortKey, setSortKey] = useState<SortKey>("started_at");
  const [sortAsc, setSortAsc] = useState(false);
  const [selected, setSelected] = useState<RunRow | null>(null);
  const [checked, setChecked] = useState<Set<string>>(new Set());
  const [snapshots, setSnapshots] = useState<SnapshotRow[]>([]);
  const [waveSkips, setWaveSkips] = useState<WaveSkipRow[]>([]);
  const [compareRuns, setCompareRuns] = useState<RunRow[]>([]);
  const [compareSnapshots, setCompareSnapshots] = useState<
    Record<string, SnapshotRow[]>
  >({});
  const [compareWaveSkips, setCompareWaveSkips] = useState<
    Record<string, WaveSkipRow[]>
  >({});
  const [compareLoading, setCompareLoading] = useState(false);
  const [page, setPage] = useState(1);
  const [pageSize, setPageSize] = useState<number>(5);
  const [jumpPage, setJumpPage] = useState("");
  const [compareXAxis, setCompareXAxis] = useState<CompareXAxis>("wave");
  const [selectedSnapshotIds, setSelectedSnapshotIds] = useState<Set<string>>(
    new Set()
  );
  const [selectedWaveSkipIds, setSelectedWaveSkipIds] = useState<Set<string>>(
    new Set()
  );
  const [exportStatus, setExportStatus] = useState<string | null>(null);
  const chartRef = useRef<HTMLDivElement>(null);
  const compareChartRef = useRef<HTMLDivElement>(null);
  const snapshotRowRefs = useRef<Map<number, HTMLTableRowElement>>(new Map());

  const listFilter = useCallback((): RunFilter => {
    const next: RunFilter = { ...filter };
    if (dateFrom) {
      next.date_from = localDateToIsoStart(dateFrom);
    } else {
      delete next.date_from;
    }
    if (dateTo) {
      next.date_to = localDateToIsoEnd(dateTo);
    } else {
      delete next.date_to;
    }
    return next;
  }, [filter, dateFrom, dateTo]);

  const reload = useCallback(() => {
    api.listRuns(listFilter()).then(setRuns).catch(() => {});
  }, [listFilter]);

  useEffect(reload, [reload]);

  useEffect(() => {
    setPage(1);
  }, [filter, dateFrom, dateTo, pageSize]);

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
    const runId = selected?.id ?? null;
    if (runId) {
      Promise.all([api.runSnapshots(runId), api.runWaveSkips(runId)])
        .then(([snaps, skips]) => {
          setSnapshots(snaps);
          setWaveSkips(skips);
        })
        .catch(() => {});
    } else {
      setSnapshots([]);
      setWaveSkips([]);
    }
    setSelectedSnapshotIds(new Set());
    setSelectedWaveSkipIds(new Set());
    snapshotRowRefs.current.clear();
  }, [selected?.id]);

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
        ids.map(async (id) => {
          const [snaps, skips] = await Promise.all([
            api.runSnapshots(id),
            api.runWaveSkips(id),
          ]);
          return [id, { snaps, skips }] as const;
        })
      );
      setCompareSnapshots(
        Object.fromEntries(entries.map(([id, { snaps }]) => [id, snaps]))
      );
      setCompareWaveSkips(
        Object.fromEntries(entries.map(([id, { skips }]) => [id, skips]))
      );
      setCompareRuns(runsToCompare);
    } catch (e) {
      alert(String(e));
    } finally {
      setCompareLoading(false);
    }
  };

  const compareRunIdsKey = compareRuns.map((r) => r.id).join(",");
  const compareRunIds = compareRunIdsKey ? compareRunIdsKey.split(",") : [];
  const hasOngoingCompareRun = compareRuns.some((r) => !r.ended_at);

  const refreshCompare = useCallback(async () => {
    const ids = compareRunIdsKey ? compareRunIdsKey.split(",") : [];
    if (ids.length < 2) return;
    try {
      const activeFilter = listFilter();
      const [entries, updatedRuns] = await Promise.all([
        Promise.all(
          ids.map(async (id) => {
            const [snaps, skips] = await Promise.all([
              api.runSnapshots(id),
              api.runWaveSkips(id),
            ]);
            return [id, { snaps, skips }] as const;
          })
        ),
        api.listRuns(activeFilter),
      ]);
      setCompareSnapshots(
        Object.fromEntries(entries.map(([id, { snaps }]) => [id, snaps]))
      );
      setCompareWaveSkips(
        Object.fromEntries(entries.map(([id, { skips }]) => [id, skips]))
      );
      setCompareRuns(
        ids
          .map((id) => updatedRuns.find((r) => r.id === id))
          .filter((r): r is RunRow => r != null)
      );
      setRuns(updatedRuns);
    } catch {
      /* keep last chart */
    }
  }, [compareRunIdsKey, listFilter]);

  useEffect(() => {
    if (compareRunIds.length < 2 || !hasOngoingCompareRun) return;
    void refreshCompare();
    const id = window.setInterval(() => void refreshCompare(), 2000);
    return () => window.clearInterval(id);
  }, [compareRunIdsKey, hasOngoingCompareRun, refreshCompare]);

  useEffect(() => {
    if (compareRunIds.length < 2 || !hasOngoingCompareRun) return;
    const ids = compareRunIdsKey.split(",");
    let unlisten: (() => void) | undefined;
    void api
      .onScannerUpdate((e) => {
        if (e.current_run_id && ids.includes(e.current_run_id)) {
          void refreshCompare();
        }
      })
      .then((fn) => {
        unlisten = fn;
      });
    return () => unlisten?.();
  }, [compareRunIdsKey, hasOngoingCompareRun, refreshCompare]);

  const clearCompare = () => {
    setCompareRuns([]);
    setCompareSnapshots({});
    setCompareWaveSkips({});
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
      const updated = await api.listRuns(listFilter());
      setRuns(updated);
      const combined = updated.find((r) => r.id === newId) ?? null;
      setSelected(combined);
    } catch (e) {
      alert(String(e));
    }
  };

  const flashExport = (message: string) => {
    setExportStatus(message);
    window.setTimeout(() => setExportStatus(null), 2000);
  };

  const exportCsv = async () => {
    try {
      const result = await api.exportCsv(listFilter());
      downloadTextFile(result.content, result.filename);
      flashExport(
        `Downloaded ${result.snapshot_count} snapshot${result.snapshot_count === 1 ? "" : "s"} ✓`
      );
    } catch (e) {
      alert(e);
    }
  };

  const exportWorkbook = async () => {
    try {
      const result = await api.exportWorkbook(listFilter());
      downloadBase64File(
        result.data_base64,
        result.filename,
        "application/vnd.oasis.opendocument.spreadsheet"
      );
      flashExport(
        `Downloaded ${result.run_count} run${result.run_count === 1 ? "" : "s"} ✓`
      );
    } catch (e) {
      alert(e);
    }
  };

  const snapshotByWave = useMemo(() => {
    const map = new Map<number, SnapshotRow>();
    for (const s of snapshots) {
      map.set(s.wave, s);
    }
    return map;
  }, [snapshots]);

  const selectedWaves = useMemo(
    () =>
      snapshots
        .filter((s) => selectedSnapshotIds.has(s.id))
        .map((s) => s.wave),
    [snapshots, selectedSnapshotIds]
  );

  const allSnapshotsChecked =
    snapshots.length > 0 &&
    snapshots.every((s) => selectedSnapshotIds.has(s.id));

  const toggleSnapshotId = useCallback((id: string, wave: number) => {
    setSelectedSnapshotIds((prev) => {
      const next = new Set(prev);
      if (next.has(id)) {
        next.delete(id);
      } else {
        next.add(id);
        snapshotRowRefs.current.get(wave)?.scrollIntoView({
          block: "nearest",
          behavior: "smooth",
        });
      }
      return next;
    });
  }, []);

  const toggleSnapshotWave = useCallback(
    (wave: number) => {
      const snap = snapshotByWave.get(wave);
      if (snap) toggleSnapshotId(snap.id, wave);
    },
    [snapshotByWave, toggleSnapshotId]
  );

  const selectSnapshotWaves = useCallback(
    (waves: number[], additive: boolean) => {
      const waveSet = new Set(waves);
      setSelectedSnapshotIds((prev) => {
        const next = additive ? new Set(prev) : new Set<string>();
        for (const snap of snapshots) {
          if (waveSet.has(snap.wave)) {
            next.add(snap.id);
          }
        }
        return next;
      });
    },
    [snapshots]
  );

  const toggleAllSnapshots = () => {
    if (allSnapshotsChecked) {
      setSelectedSnapshotIds(new Set());
      return;
    }
    setSelectedSnapshotIds(new Set(snapshots.map((s) => s.id)));
  };

  const clearAllSelections = () => {
    setSelectedSnapshotIds(new Set());
    setSelectedWaveSkipIds(new Set());
  };

  const toggleWaveSkipId = useCallback((id: string) => {
    setSelectedWaveSkipIds((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }, []);

  const allWaveSkipsChecked =
    waveSkips.length > 0 &&
    waveSkips.every((s) => selectedWaveSkipIds.has(s.id));

  const toggleAllWaveSkips = () => {
    if (allWaveSkipsChecked) {
      setSelectedWaveSkipIds(new Set());
      return;
    }
    setSelectedWaveSkipIds(new Set(waveSkips.map((s) => s.id)));
  };

  const ongoingRunNote = () =>
    selected && !selected.ended_at
      ? "\n\nThis run is still open — stop the scanner first or deleted waves may be recorded again."
      : "";

  const selectedRunId = selected?.id ?? null;
  const hasOngoingSelectedRun =
    selected != null && !selected.ended_at && compareRuns.length < 2;

  const refreshSelectedRun = useCallback(async () => {
    if (!selectedRunId) return;
    try {
      const activeFilter = listFilter();
      const [snaps, skips, updatedRuns] = await Promise.all([
        api.runSnapshots(selectedRunId),
        api.runWaveSkips(selectedRunId),
        api.listRuns(activeFilter),
      ]);
      setSnapshots(snaps);
      setWaveSkips(skips);
      setSelectedSnapshotIds((prev) => {
        if (prev.size === 0) return prev;
        const valid = new Set(snaps.map((s) => s.id));
        let changed = false;
        const next = new Set<string>();
        for (const id of prev) {
          if (valid.has(id)) next.add(id);
          else changed = true;
        }
        return changed ? next : prev;
      });
      setSelectedWaveSkipIds((prev) => {
        if (prev.size === 0) return prev;
        const valid = new Set(skips.map((s) => s.id));
        let changed = false;
        const next = new Set<string>();
        for (const id of prev) {
          if (valid.has(id)) next.add(id);
          else changed = true;
        }
        return changed ? next : prev;
      });
      setRuns(updatedRuns);
      setSelected(updatedRuns.find((r) => r.id === selectedRunId) ?? null);
    } catch {
      /* keep last chart */
    }
  }, [selectedRunId, listFilter]);

  useEffect(() => {
    if (!selectedRunId || !hasOngoingSelectedRun) return;
    void refreshSelectedRun();
    const id = window.setInterval(() => void refreshSelectedRun(), 2000);
    return () => window.clearInterval(id);
  }, [selectedRunId, hasOngoingSelectedRun, refreshSelectedRun]);

  useEffect(() => {
    if (!selectedRunId || !hasOngoingSelectedRun) return;
    let unlisten: (() => void) | undefined;
    void api
      .onScannerUpdate((e) => {
        if (e.current_run_id === selectedRunId) {
          void refreshSelectedRun();
        }
      })
      .then((fn) => {
        unlisten = fn;
      });
    return () => unlisten?.();
  }, [selectedRunId, hasOngoingSelectedRun, refreshSelectedRun]);

  const deleteSelectedSnapshots = async () => {
    if (!selected || selectedSnapshotIds.size === 0) return;
    const n = selectedSnapshotIds.size;
    if (
      !confirm(
        `Delete ${n} snapshot${n === 1 ? "" : "s"}?${ongoingRunNote()}`
      )
    ) {
      return;
    }
    try {
      await api.deleteSnapshots([...selectedSnapshotIds]);
      setSelectedSnapshotIds(new Set());
      await refreshSelectedRun();
    } catch (e) {
      alert(String(e));
    }
  };

  const deleteSelectedWaveSkips = async () => {
    if (!selected || selectedWaveSkipIds.size === 0) return;
    const n = selectedWaveSkipIds.size;
    if (
      !confirm(
        `Delete ${n} wave skip record${n === 1 ? "" : "s"}? Coin/min snapshots are kept.${ongoingRunNote()}`
      )
    ) {
      return;
    }
    try {
      await api.deleteWaveSkips([...selectedWaveSkipIds]);
      setSelectedWaveSkipIds(new Set());
      await refreshSelectedRun();
    } catch (e) {
      alert(String(e));
    }
  };

  const deleteWaveSkip = async (skip: WaveSkipRow) => {
    if (!selected) return;
    if (
      !confirm(
        `Delete wave skip at wave ${skip.at_wave} (×${skip.skipped_count})?${ongoingRunNote()}`
      )
    ) {
      return;
    }
    try {
      await api.deleteWaveSkip(skip.id);
      setSelectedWaveSkipIds((prev) => {
        if (!prev.has(skip.id)) return prev;
        const next = new Set(prev);
        next.delete(skip.id);
        return next;
      });
      await refreshSelectedRun();
    } catch (e) {
      alert(String(e));
    }
  };

  const deleteSnapshot = async (snap: SnapshotRow) => {
    if (!selected) return;
    if (
      !confirm(
        `Delete snapshot for wave ${snap.wave} (${formatCoin(snap.coin_per_minute)})?${ongoingRunNote()}`
      )
    ) {
      return;
    }
    try {
      await api.deleteSnapshot(snap.id);
      setSelectedSnapshotIds((prev) => {
        if (!prev.has(snap.id)) return prev;
        const next = new Set(prev);
        next.delete(snap.id);
        return next;
      });
      await refreshSelectedRun();
    } catch (e) {
      alert(String(e));
    }
  };

  const chartData = snapshotsToChartData(snapshots);
  const skipMarkers = waveSkipsToMarkers(waveSkips);

  const compareChartData =
    compareXAxis === "wave"
      ? buildCompareChartDataByWave(compareRunIds, compareSnapshots)
      : buildCompareChartDataByProgress(compareRunIds, compareSnapshots);

  const compareSkipMarkers = compareRunIds.map((id) =>
    waveSkipsToMarkers(compareWaveSkips[id] ?? [])
  );

  const compareLines: ChartLineConfig[] = compareRuns.map((r, i) => ({
    dataKey: `coin_${i}`,
    name: runShortLabel(r),
    stroke: COMPARE_COLORS[i % COMPARE_COLORS.length],
  }));

  const goToPage = (raw: string) => {
    const n = Number.parseInt(raw, 10);
    if (!Number.isFinite(n) || n < 1 || n > totalPages) return;
    setPage(n);
    setJumpPage("");
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
        <label className="date-filter">
          From
          <input
            type="date"
            value={dateFrom}
            max={dateTo || undefined}
            onChange={(e) => setDateFrom(e.target.value)}
            aria-label="Filter runs from date"
          />
        </label>
        <label className="date-filter">
          To
          <input
            type="date"
            value={dateTo}
            min={dateFrom || undefined}
            onChange={(e) => setDateTo(e.target.value)}
            aria-label="Filter runs to date"
          />
        </label>
        {(dateFrom || dateTo) && (
          <button
            type="button"
            onClick={() => {
              setDateFrom("");
              setDateTo("");
            }}
          >
            Clear dates
          </button>
        )}
        <button onClick={reload}>Refresh</button>
        <button onClick={exportCsv}>Export CSV</button>
        <button onClick={exportWorkbook}>Export ODS</button>
        {exportStatus && (
          <span className="chart-action-status">{exportStatus}</span>
        )}
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
            <label className="page-jump-label">
              Go to
              <input
                type="number"
                className="page-jump-input"
                min={1}
                max={totalPages}
                value={jumpPage}
                placeholder={String(safePage)}
                onChange={(e) => setJumpPage(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter") goToPage(jumpPage);
                }}
                aria-label="Jump to page"
              />
            </label>
            <button
              type="button"
              disabled={
                !jumpPage ||
                Number.parseInt(jumpPage, 10) < 1 ||
                Number.parseInt(jumpPage, 10) > totalPages
              }
              onClick={() => goToPage(jumpPage)}
            >
              Go
            </button>
          </div>
        </div>
      )}

      {compareRuns.length >= 2 && (
        <div className="chart-card compare-card" ref={compareChartRef}>
          <div className="chart-card-header">
            <h3>
              Compare {compareRuns.length} runs — coin/min vs{" "}
              {compareXAxis === "wave" ? "wave" : "snapshot #"}
              {hasOngoingCompareRun && (
                <span className="muted compare-live"> · live</span>
              )}
            </h3>
            <div className="chart-card-actions">
              <select
                className="compare-axis-select"
                value={compareXAxis}
                onChange={(e) =>
                  setCompareXAxis(e.target.value as CompareXAxis)
                }
                aria-label="Compare chart X axis"
              >
                <option value="wave">Absolute wave</option>
                <option value="progress">Snapshot progress</option>
              </select>
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
          <CoinVsWaveChart
            mode="compare"
            data={compareChartData}
            lines={compareLines}
            waveSkipsByLine={compareSkipMarkers}
            xAxis={compareXAxis}
            height={320}
          />
        </div>
      )}

      {selected && compareRuns.length < 2 && (
        <>
          <div className="chart-card" ref={chartRef}>
            <div className="chart-card-header">
              <h3>
                Run {new Date(selected.started_at).toLocaleString()} — avg coin/min vs
                wave
                {skipMarkers.length > 0 && (
                  <span className="muted">
                    {" "}
                    · {skipMarkers.length} wave skip
                    {skipMarkers.length === 1 ? "" : "s"} (right axis)
                  </span>
                )}
                {hasOngoingSelectedRun && (
                  <span className="muted compare-live"> · live</span>
                )}
              </h3>
              <ChartScreenshotActions
                targetRef={chartRef}
                disabled={chartData.length === 0}
              />
            </div>
            <CoinVsWaveChart
              mode="single"
              data={chartData}
              waveSkips={skipMarkers}
              height={300}
              selectedWaves={selectedWaves}
              selectedSkipIds={[...selectedWaveSkipIds]}
              onPointClick={toggleSnapshotWave}
              onSkipClick={(id) => toggleWaveSkipId(id)}
              onSelectWaves={selectSnapshotWaves}
            />
          </div>

          {waveSkips.length > 0 && (
            <SkipCoinAnalytics snapshots={snapshots} waveSkips={waveSkips} />
          )}

          <div className="snapshot-panel">
            <div className="snapshot-panel-header">
              <h3>
                Snapshots ({snapshots.length}
                {selectedSnapshotIds.size > 0
                  ? ` · ${selectedSnapshotIds.size} selected`
                  : ""}
                )
              </h3>
              <div className="snapshot-panel-actions">
                <span className="muted">
                  Click coin points or skip points on the chart. Drag a rectangle
                  to select coin/min snapshots. Shift+drag adds to the selection.
                </span>
                <button
                  type="button"
                  disabled={
                    selectedSnapshotIds.size === 0 &&
                    selectedWaveSkipIds.size === 0
                  }
                  onClick={clearAllSelections}
                >
                  Clear selection
                </button>
                <button
                  type="button"
                  className="danger"
                  disabled={selectedSnapshotIds.size === 0}
                  onClick={deleteSelectedSnapshots}
                >
                  Delete snapshots ({selectedSnapshotIds.size})
                </button>
              </div>
            </div>
            <div className="snapshot-table-wrap">
              <table className="snapshot-table">
                <thead>
                  <tr>
                    <th className="check-col">
                      <input
                        type="checkbox"
                        checked={allSnapshotsChecked}
                        onChange={toggleAllSnapshots}
                        aria-label="Select all snapshots"
                      />
                    </th>
                    <th>Wave</th>
                    <th>Tier</th>
                    <th>Coin/min</th>
                    <th>Recorded</th>
                    <th />
                  </tr>
                </thead>
                <tbody>
                  {snapshots.map((s) => (
                    <tr
                      key={s.id}
                      ref={(el) => {
                        if (el) snapshotRowRefs.current.set(s.wave, el);
                        else snapshotRowRefs.current.delete(s.wave);
                      }}
                      className={
                        selectedSnapshotIds.has(s.id) ? "snapshot-selected" : ""
                      }
                      onClick={() => toggleSnapshotId(s.id, s.wave)}
                    >
                      <td
                        className="check-col"
                        onClick={(e) => e.stopPropagation()}
                      >
                        <input
                          type="checkbox"
                          checked={selectedSnapshotIds.has(s.id)}
                          onChange={() => toggleSnapshotId(s.id, s.wave)}
                          aria-label={`Select wave ${s.wave}`}
                        />
                      </td>
                      <td>{s.wave}</td>
                      <td>{s.tier ?? "—"}</td>
                      <td>{formatCoin(s.coin_per_minute)}</td>
                      <td>{new Date(s.recorded_at).toLocaleString()}</td>
                      <td
                        className="snapshot-actions"
                        onClick={(e) => e.stopPropagation()}
                      >
                        <button
                          type="button"
                          className="danger"
                          onClick={() => deleteSnapshot(s)}
                        >
                          Delete
                        </button>
                      </td>
                    </tr>
                  ))}
                  {snapshots.length === 0 && (
                    <tr>
                      <td colSpan={6} className="muted">
                        No snapshots in this run.
                      </td>
                    </tr>
                  )}
                </tbody>
              </table>
            </div>
          </div>

          {waveSkips.length > 0 && (
            <div className="snapshot-panel">
              <div className="snapshot-panel-header">
                <h3>
                  Wave skips ({waveSkips.length}
                  {selectedWaveSkipIds.size > 0
                    ? ` · ${selectedWaveSkipIds.size} selected`
                    : ""}
                  )
                </h3>
                <div className="snapshot-panel-actions">
                  <button
                    type="button"
                    className="danger"
                    disabled={selectedWaveSkipIds.size === 0}
                    onClick={deleteSelectedWaveSkips}
                  >
                    Delete wave skips ({selectedWaveSkipIds.size})
                  </button>
                </div>
              </div>
              <div className="snapshot-table-wrap">
                <table className="snapshot-table">
                  <thead>
                    <tr>
                      <th className="check-col">
                        <input
                          type="checkbox"
                          checked={allWaveSkipsChecked}
                          onChange={toggleAllWaveSkips}
                          aria-label="Select all wave skips"
                        />
                      </th>
                      <th>Wave</th>
                      <th>Skipped</th>
                      <th>Coin/min</th>
                      <th>Recorded</th>
                      <th />
                    </tr>
                  </thead>
                  <tbody>
                    {waveSkips.map((s) => (
                      <tr
                        key={s.id}
                        className={
                          selectedWaveSkipIds.has(s.id) ? "snapshot-selected" : ""
                        }
                        onClick={() => toggleWaveSkipId(s.id)}
                      >
                        <td
                          className="check-col"
                          onClick={(e) => e.stopPropagation()}
                        >
                          <input
                            type="checkbox"
                            checked={selectedWaveSkipIds.has(s.id)}
                            onChange={() => toggleWaveSkipId(s.id)}
                            aria-label={`Select wave skip at wave ${s.at_wave}`}
                          />
                        </td>
                        <td>{s.at_wave}</td>
                        <td>×{s.skipped_count}</td>
                        <td>{formatCoin(s.coin_per_minute)}</td>
                        <td>{new Date(s.recorded_at).toLocaleString()}</td>
                        <td
                          className="snapshot-actions"
                          onClick={(e) => e.stopPropagation()}
                        >
                          <button
                            type="button"
                            className="danger"
                            onClick={() => deleteWaveSkip(s)}
                          >
                            Delete
                          </button>
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            </div>
          )}
        </>
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

/** Local calendar date (YYYY-MM-DD) → UTC ISO start of that local day. */
function localDateToIsoStart(date: string): string {
  return new Date(`${date}T00:00:00`).toISOString();
}

/** Local calendar date (YYYY-MM-DD) → UTC ISO end of that local day. */
function localDateToIsoEnd(date: string): string {
  return new Date(`${date}T23:59:59.999`).toISOString();
}
