import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { api, formatAvgGoldenComboCaret, formatCoin, formatGoldenCombo, parseOptionalCoin, ExportFileResult, RunFilter, RunRow, SnapshotRow, WaveSkipRow } from "../api";
import {
  buildCompareChartDataByWave,
  applyCompareChartSmoothing,
  compareNewerRunIndex,
  compareHasGoldenComboActivations,
  buildChartWaveJumpMarkers,
  buildWaveJumpMarkers,
  mergeCompareWithSkips,
  snapshotsToChartData,
  type LeadLagMetric,
} from "../chartData";
import ChartScreenshotActions from "./ChartScreenshotActions";
import CoinVsWaveChart, { ChartLineConfig } from "./CoinVsWaveChart";
import SkipCoinAnalytics from "./SkipCoinAnalytics";
import SortableTh from "./SortableTh";
import { formatSkipDisplay, skipDisplayFromRow, skipChartValue } from "../skipDisplay";
import { formatRunType, runTypeUsesBadge, RUN_TYPE_FILTER_OPTIONS } from "../runType";
import { reportUiError } from "../uiError";
import { confirmDialog } from "../confirmDialog";
import { setCompareSessionActive } from "../notificationCapture";
import { usePersistedBoolean } from "../persistedState";
import {
  CoinIcon,
  WaveJumpIcon,
  GcActivationIcon,
  LeadLagIcon,
  FollowRunIcon,
  PreviousRunIcon,
} from "./ChartToggleIcons";

type SortKey =
  | "started_at"
  | "duration"
  | "run_type"
  | "final_wave"
  | "peak_tier"
  | "avg_coin_per_minute"
  | "avg_golden_combo_caret"
  | "snapshot_count"
  | "comment";

/** Resolves a run's value for a sort key; "duration" is derived (ongoing runs use "now"). */
function runSortValue(r: RunRow, key: SortKey): number | string | null {
  if (key === "duration") {
    const end = r.ended_at ? +new Date(r.ended_at) : Date.now();
    return end - +new Date(r.started_at);
  }
  return r[key];
}

type CoinSortKey = "wave" | "tier" | "coin_per_minute" | "recorded_at";
type GcSortKey =
  | "wave"
  | "golden_combo_chance"
  | "golden_combo_caret"
  | "golden_combo_multiplier"
  | "recorded_at";
type SkipSortKey = "at_wave" | "skipped_count" | "coin_per_minute" | "recorded_at";

/** Sorts a copy of `items` by `getValue`, keeping nulls last regardless of direction. */
function sortByValue<T>(
  items: T[],
  getValue: (item: T) => number | string | null,
  asc: boolean
): T[] {
  return [...items].sort((a, b) => {
    const av = getValue(a);
    const bv = getValue(b);
    if (av == null && bv == null) return 0;
    if (av == null) return 1;
    if (bv == null) return -1;
    const cmp = av < bv ? -1 : av > bv ? 1 : 0;
    return asc ? cmp : -cmp;
  });
}

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

const COMPARE_SMOOTH_WINDOWS = [0, 3, 5, 10, 20, 50, 100] as const;
type CompareSmoothWindow = (typeof COMPARE_SMOOTH_WINDOWS)[number];

const LEAD_LAG_METRIC_LABEL: Record<LeadLagMetric, string> = {
  coin: "coin/min",
  gc: "GC activations",
  skip: "wave jumps",
};

const PAGE_SIZES = [5, 10, 25, 50, 100] as const;
/** Min gap between live History table refreshes on scanner-update (avoids DB thrash). */
const LIVE_REFRESH_MIN_MS = 2500;

function pruneSelectedIds(
  prev: Set<string>,
  validIds: Iterable<string>
): Set<string> {
  if (prev.size === 0) return prev;
  const valid = new Set(validIds);
  let changed = false;
  const next = new Set<string>();
  for (const id of prev) {
    if (valid.has(id)) next.add(id);
    else changed = true;
  }
  return changed ? next : prev;
}

const COMPARE_IDS_STORAGE_KEY = "wavetrace.history.compare.runIds";

function loadPersistedCompareIds(): string[] {
  try {
    const raw = localStorage.getItem(COMPARE_IDS_STORAGE_KEY);
    if (!raw) return [];
    const parsed = JSON.parse(raw);
    return Array.isArray(parsed)
      ? parsed.filter((x): x is string => typeof x === "string")
      : [];
  } catch {
    return [];
  }
}

function savePersistedCompareIds(ids: string[]): void {
  try {
    if (ids.length >= 2) {
      localStorage.setItem(COMPARE_IDS_STORAGE_KEY, JSON.stringify(ids));
    } else {
      localStorage.removeItem(COMPARE_IDS_STORAGE_KEY);
    }
  } catch {
    // localStorage unavailable — the selection just won't survive a restart.
  }
}

export default function History() {
  const [runs, setRuns] = useState<RunRow[]>([]);
  const [filter, setFilter] = useState<RunFilter>({});
  const [dateFrom, setDateFrom] = useState("");
  const [dateTo, setDateTo] = useState("");
  const [sortKey, setSortKey] = useState<SortKey>("started_at");
  const [sortAsc, setSortAsc] = useState(false);
  const [filtersOpen, setFiltersOpen] = usePersistedBoolean(
    "history.filtersOpen",
    true
  );
  const [selected, setSelected] = useState<RunRow | null>(null);
  const [checked, setChecked] = useState<Set<string>>(new Set());
  const [snapshots, setSnapshots] = useState<SnapshotRow[]>([]);
  const [waveSkips, setWaveSkips] = useState<WaveSkipRow[]>([]);
  const [liveChartSnapshots, setLiveChartSnapshots] = useState<SnapshotRow[]>([]);
  const [liveChartWaveSkips, setLiveChartWaveSkips] = useState<WaveSkipRow[]>([]);
  const [liveChartNormalJumps, setLiveChartNormalJumps] = useState<number[]>([]);
  const [compareRuns, setCompareRuns] = useState<RunRow[]>([]);
  const [compareSnapshots, setCompareSnapshots] = useState<
    Record<string, SnapshotRow[]>
  >({});
  const [compareWaveSkips, setCompareWaveSkips] = useState<
    Record<string, WaveSkipRow[]>
  >({});
  const [compareNormalJumps, setCompareNormalJumps] = useState<
    Record<string, number[]>
  >({});
  const [compareLoading, setCompareLoading] = useState(false);
  const [page, setPage] = useState(1);
  const [pageSize, setPageSize] = useState<number>(5);
  const [jumpPage, setJumpPage] = useState("");
  const [compareShowSkips, setCompareShowSkips] = usePersistedBoolean(
    "history.compare.showSkips",
    false
  );
  const [compareShowCoin, setCompareShowCoin] = usePersistedBoolean(
    "history.compare.showCoin",
    true
  );
  const [compareShowGc, setCompareShowGc] = usePersistedBoolean(
    "history.compare.showGc",
    false
  );
  const [showWaveJumps, setShowWaveJumps] = usePersistedBoolean(
    "history.single.showWaveJumps",
    true
  );
  const [showCoinPerMinute, setShowCoinPerMinute] = usePersistedBoolean(
    "history.single.showCoinPerMinute",
    true
  );
  const [showGcActivations, setShowGcActivations] = usePersistedBoolean(
    "history.single.showGcActivations",
    true
  );
  const [coinOutlierBelow, setCoinOutlierBelow] = useState("");
  const [coinOutlierAbove, setCoinOutlierAbove] = useState("");
  const [gcOutlierBelow, setGcOutlierBelow] = useState("");
  const [gcOutlierAbove, setGcOutlierAbove] = useState("");
  const [skipOutlierBelow, setSkipOutlierBelow] = useState("");
  const [skipOutlierAbove, setSkipOutlierAbove] = useState("");
  const [compareSmoothWindow, setCompareSmoothWindow] = useState<CompareSmoothWindow>(
    10
  );
  const [compareLeadLagBand, setCompareLeadLagBand] = usePersistedBoolean(
    "history.compare.leadLagBand",
    true
  );
  const [compareFollowNewRun, setCompareFollowNewRun] = usePersistedBoolean(
    "history.compare.followNewRun",
    true
  );
  const [compareFollowKeepPrevious, setCompareFollowKeepPrevious] =
    usePersistedBoolean("history.compare.followKeepPrevious", false);
  const [chartExpanded, setChartExpanded] = useState(false);
  const [expandedChartHeight, setExpandedChartHeight] = useState<number>();
  const [chartSelectMode, setChartSelectMode] = useState(false);
  const [selectedSnapshotIds, setSelectedSnapshotIds] = useState<Set<string>>(
    new Set()
  );
  const [selectedWaveSkipIds, setSelectedWaveSkipIds] = useState<Set<string>>(
    new Set()
  );
  const [selectedGcIds, setSelectedGcIds] = useState<Set<string>>(new Set());
  const [editingGcId, setEditingGcId] = useState<string | null>(null);
  const [gcEditChance, setGcEditChance] = useState("");
  const [gcEditCaret, setGcEditCaret] = useState("");
  const [gcEditMultiplier, setGcEditMultiplier] = useState("");
  const [coinSortKey, setCoinSortKey] = useState<CoinSortKey>("wave");
  const [coinSortAsc, setCoinSortAsc] = useState(true);
  const [gcSortKey, setGcSortKey] = useState<GcSortKey>("wave");
  const [gcSortAsc, setGcSortAsc] = useState(true);
  const [skipSortKey, setSkipSortKey] = useState<SkipSortKey>("at_wave");
  const [skipSortAsc, setSkipSortAsc] = useState(true);
  const [exportStatus, setExportStatus] = useState<string | null>(null);
  const [exportBusy, setExportBusy] = useState(false);
  const [exportProgress, setExportProgress] = useState<{
    done: number;
    total: number;
  } | null>(null);
  const chartRef = useRef<HTMLDivElement>(null);
  const compareChartRef = useRef<HTMLDivElement>(null);
  const snapshotRowRefs = useRef<Map<number, HTMLTableRowElement>>(new Map());
  const waveSkipRowRefs = useRef<Map<string, HTMLTableRowElement>>(new Map());
  const gcRowRefs = useRef<Map<string, HTMLTableRowElement>>(new Map());
  const liveRefreshAtRef = useRef(0);
  /** Guards compareSelected()/restoreCompareFromIds() against out-of-order
   * responses: only the most recently issued call is allowed to update
   * compare state. refreshCompare() deliberately does NOT use this — it's a
   * background sync of whatever comparison is already active, and must
   * never be able to cancel a fresh user-initiated compareSelected() call
   * (see compareRunIdsRef instead). */
  const compareRequestRef = useRef(0);
  /** Mirrors compareRunIdsKey for refreshCompare()'s own out-of-order guard:
   * bail if the set of compared runs changed while the fetch was in flight. */
  const compareRunIdsRef = useRef("");
  const expandedChartBodyRef = useRef<HTMLDivElement>(null);
  const toolbarCompactBarRef = useRef<HTMLDivElement>(null);
  const toolbarFiltersBtnRef = useRef<HTMLButtonElement>(null);
  const toolbarActionsShadowRef = useRef<HTMLDivElement>(null);
  const [toolbarActionsCompact, setToolbarActionsCompact] = useState(false);
  const compareTitleRef = useRef<HTMLHeadingElement>(null);
  const compareActionsShadowRef = useRef<HTMLDivElement>(null);
  const [compareActionsCompact, setCompareActionsCompact] = useState(false);

  useEffect(() => {
    const active = compareRuns.length >= 2;
    setCompareSessionActive(active);
    void api.setCompareCaptureActive(active);
  }, [compareRuns]);

  // Auto-collapse if the expanded chart's data disappears from under it
  // (comparison cleared, run deselected, etc.).
  useEffect(() => {
    const chartVisible =
      compareRuns.length >= 2 || (selected != null && compareRuns.length < 2);
    if (!chartVisible) setChartExpanded(false);
  }, [compareRuns.length, selected]);

  useEffect(() => {
    if (!chartExpanded) return;
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") setChartExpanded(false);
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [chartExpanded]);

  useEffect(() => {
    if (!chartExpanded) return;
    const el = expandedChartBodyRef.current;
    if (!el) return;
    const update = () => setExpandedChartHeight(el.clientHeight);
    update();
    const observer = new ResizeObserver(update);
    observer.observe(el);
    return () => observer.disconnect();
  }, [chartExpanded]);

  // Hide the toolbar action buttons' text labels only once they'd actually
  // stop fitting — measured against a hidden always-labeled copy — rather
  // than wrapping/scrolling while there'd have been room for icon-only
  // buttons. .toolbar-compact-bar is flex-nowrap and the Filters button is
  // flex-shrink:0 (see CSS), so both their sizes are stable regardless of
  // what the actions column ends up rendering, which is what makes this
  // arithmetic reliable. clientWidth includes the bar's own left/right
  // padding, which isn't available to its children, so that has to come
  // out too.
  useLayoutEffect(() => {
    const bar = toolbarCompactBarRef.current;
    const filtersBtn = toolbarFiltersBtnRef.current;
    const shadow = toolbarActionsShadowRef.current;
    if (!bar || !filtersBtn || !shadow) return;
    const check = () => {
      const barStyle = getComputedStyle(bar);
      const barPadding =
        parseFloat(barStyle.paddingLeft) + parseFloat(barStyle.paddingRight);
      const available =
        bar.clientWidth - barPadding - filtersBtn.offsetWidth - 10;
      setToolbarActionsCompact(shadow.scrollWidth > available);
    };
    check();
    const observer = new ResizeObserver(check);
    observer.observe(bar);
    observer.observe(shadow);
    return () => observer.disconnect();
  }, []);

  // Same idea as the toolbar above: the title and the actions row share one
  // line (.chart-card-header is nowrap — see CSS), so hide the toggle
  // buttons' text labels once title + actions together stop fitting, the
  // same way the Filters button forced the toolbar's actions to compact.
  // title.scrollWidth (not offsetWidth) is used because the title itself is
  // allowed to shrink/truncate via CSS as a last resort — scrollWidth still
  // reports its true, unshrunk content width regardless of how narrow its
  // box is currently rendered.
  // Depends on compareRuns.length because the whole card (and these refs)
  // only exist in the DOM once there are 2+ compared runs — compareRuns
  // starts empty and populates asynchronously, so an empty dep array here
  // would find null refs on the one and only run and never attach the
  // ResizeObserver at all.
  useLayoutEffect(() => {
    const card = compareChartRef.current;
    const title = compareTitleRef.current;
    const shadow = compareActionsShadowRef.current;
    if (!card || !title || !shadow) return;
    const check = () => {
      const cardStyle = getComputedStyle(card);
      const cardPadding =
        parseFloat(cardStyle.paddingLeft) + parseFloat(cardStyle.paddingRight);
      const available = card.clientWidth - cardPadding - title.scrollWidth - 12;
      setCompareActionsCompact(shadow.scrollWidth > available);
    };
    check();
    const observer = new ResizeObserver(check);
    observer.observe(card);
    observer.observe(title);
    observer.observe(shadow);
    return () => observer.disconnect();
  }, [compareRuns.length]);

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

  // Refresh the runs list whenever the scanner switches to a different run
  // (a new run starting, or resuming one) so it shows up without a manual
  // refresh.
  useEffect(() => {
    let lastRunId: string | null = null;
    let unlisten: (() => void) | undefined;
    void api
      .onScannerUpdate((e) => {
        if (e.current_run_id === lastRunId) return;
        lastRunId = e.current_run_id;
        if (e.current_run_id) reload();
      })
      .then((fn) => {
        unlisten = fn;
      });
    return () => unlisten?.();
  }, [reload]);

  useEffect(() => {
    void api.getSettings().then((s) => {
      if (s.compare_capture_active) {
        setCompareSessionActive(true);
      }
    });
  }, []);

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
      reportUiError(e, "History");
      reload();
    }
  }, [reload]);

  const updateRunType = useCallback(async (runId: string, value: string) => {
    const apply = (run: RunRow) =>
      run.id === runId ? { ...run, run_type: value } : run;
    setRuns((prev) => prev.map(apply));
    setSelected((prev) => (prev ? apply(prev) : prev));
    setCompareRuns((prev) => prev.map(apply));
    try {
      await api.setRunType(runId, value);
    } catch (e) {
      reportUiError(e, "History");
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
    setLiveChartSnapshots([]);
    setLiveChartWaveSkips([]);
    setLiveChartNormalJumps([]);
    setChartSelectMode(false);
    setSelectedSnapshotIds(new Set());
    setSelectedWaveSkipIds(new Set());
    setSelectedGcIds(new Set());
    setEditingGcId(null);
    setCoinOutlierBelow("");
    setCoinOutlierAbove("");
    setGcOutlierBelow("");
    setGcOutlierAbove("");
    setSkipOutlierBelow("");
    setSkipOutlierAbove("");
    snapshotRowRefs.current.clear();
    waveSkipRowRefs.current.clear();
    gcRowRefs.current.clear();
  }, [selected?.id]);

  const sorted = sortByValue(runs, (r) => runSortValue(r, sortKey), sortAsc);

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
    const confirmed = await confirmDialog({
      title: n === 1 ? "Delete run?" : `Delete ${n} runs?`,
      message: `This permanently removes the selected run${n === 1 ? "" : "s"} and all snapshots.`,
      confirmLabel: "Delete",
      danger: true,
    });
    if (!confirmed) {
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
      reportUiError(e, "History");
    }
  };

  const compareSelected = async () => {
    if (checked.size < 2) return;
    const ids = [...checked];
    const runsToCompare = sorted.filter((r) => ids.includes(r.id));
    const requestId = ++compareRequestRef.current;
    setCompareLoading(true);
    try {
      const entries = await Promise.all(
        ids.map(async (id) => {
          const view = await api.runDashboardData(id);
          return [id, view] as const;
        })
      );
      // A newer compareSelected()/refreshCompare() call started while this one
      // was in flight (e.g. re-selecting runs and clicking Compare again before
      // the first fetch finished) — its result already won, so drop this one.
      if (compareRequestRef.current !== requestId) return;
      setCompareSnapshots(
        Object.fromEntries(entries.map(([id, view]) => [id, view.chart_snapshots]))
      );
      setCompareWaveSkips(
        Object.fromEntries(entries.map(([id, view]) => [id, view.chart_wave_skips]))
      );
      setCompareNormalJumps(
        Object.fromEntries(entries.map(([id, view]) => [id, view.chart_normal_jumps]))
      );
      setCompareRuns(runsToCompare);
      savePersistedCompareIds(runsToCompare.map((r) => r.id));
    } catch (e) {
      if (compareRequestRef.current === requestId) {
        reportUiError(e, "History");
      }
    } finally {
      // Unconditional: this flag reflects whether *this* compareSelected()
      // call is still in flight, not whether it won the race to render — a
      // refreshCompare() firing in the background (e.g. while filters are
      // being tweaked, for an ongoing compared run) also bumps
      // compareRequestRef and must not leave the Compare button stuck
      // disabled forever.
      setCompareLoading(false);
    }
  };

  const compareRunIdsKey = compareRuns.map((r) => r.id).join(",");
  const compareRunIds = compareRunIdsKey ? compareRunIdsKey.split(",") : [];
  const hasOngoingCompareRun = compareRuns.some((r) => !r.ended_at);
  compareRunIdsRef.current = compareRunIdsKey;

  const refreshCompare = useCallback(async () => {
    const idsAtStart = compareRunIdsKey;
    const ids = idsAtStart ? idsAtStart.split(",") : [];
    if (ids.length < 2) return;
    try {
      const [entries, allRuns, tableRuns] = await Promise.all([
        Promise.all(
          ids.map(async (id) => {
            const view = await api.runDashboardData(id);
            return [id, view] as const;
          })
        ),
        // Unfiltered — the compared runs must be found regardless of the
        // table's active filter (they may no longer match it).
        api.listRuns({}),
        api.listRuns(listFilter()),
      ]);
      // The set of compared runs changed (new comparison started, or
      // cleared) while this refresh was in flight — don't clobber it.
      if (compareRunIdsRef.current !== idsAtStart) return;
      setCompareSnapshots(
        Object.fromEntries(entries.map(([id, view]) => [id, view.chart_snapshots]))
      );
      setCompareWaveSkips(
        Object.fromEntries(entries.map(([id, view]) => [id, view.chart_wave_skips]))
      );
      setCompareNormalJumps(
        Object.fromEntries(entries.map(([id, view]) => [id, view.chart_normal_jumps]))
      );
      const matchedRuns = ids
        .map((id) => allRuns.find((r) => r.id === id))
        .filter((r): r is RunRow => r != null);
      setCompareRuns(matchedRuns);
      savePersistedCompareIds(matchedRuns.map((r) => r.id));
      setRuns(tableRuns);
    } catch {
      /* keep last chart */
    }
  }, [compareRunIdsKey, listFilter]);

  const restoreCompareFromIds = useCallback(
    async (ids: string[]) => {
      if (ids.length < 2) return;
      const requestId = ++compareRequestRef.current;
      setCompareLoading(true);
      try {
        const [entries, updatedRuns] = await Promise.all([
          Promise.all(
            ids.map(async (id) => {
              const view = await api.runDashboardData(id);
              return [id, view] as const;
            })
          ),
          api.listRuns(listFilter()),
        ]);
        if (compareRequestRef.current !== requestId) return;
        const matchedRuns = ids
          .map((id) => updatedRuns.find((r) => r.id === id))
          .filter((r): r is RunRow => r != null);
        if (matchedRuns.length < 2) {
          // One or more of the persisted runs no longer exists — drop the
          // stale selection rather than showing a partial comparison.
          savePersistedCompareIds([]);
          return;
        }
        setCompareSnapshots(
          Object.fromEntries(entries.map(([id, view]) => [id, view.chart_snapshots]))
        );
        setCompareWaveSkips(
          Object.fromEntries(entries.map(([id, view]) => [id, view.chart_wave_skips]))
        );
        setCompareNormalJumps(
          Object.fromEntries(entries.map(([id, view]) => [id, view.chart_normal_jumps]))
        );
        setCompareRuns(matchedRuns);
        setChecked(new Set(matchedRuns.map((r) => r.id)));
      } catch {
        // Persisted runs may have been deleted — give up quietly.
        savePersistedCompareIds([]);
      } finally {
        // Unconditional for the same reason as in compareSelected() — this
        // flag tracks whether this call is still in flight, not whether it
        // won the race to render.
        setCompareLoading(false);
      }
    },
    [listFilter]
  );

  useEffect(() => {
    const ids = loadPersistedCompareIds();
    if (ids.length >= 2) {
      void restoreCompareFromIds(ids);
    }
    // Restore only once, on mount — subsequent changes are persisted
    // explicitly wherever compareRuns is set.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    if (compareRunIds.length < 2 || !hasOngoingCompareRun) return;
    void refreshCompare();
    const id = window.setInterval(() => void refreshCompare(), 15_000);
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

  /** Re-points a 2-run comparison at a newly started run, keeping `keepId` as the
   * other side. Used by the "Follow new run" auto-swap below. */
  const followCompareToNewRun = useCallback(
    async (keepId: string, newRunId: string) => {
      const requestId = ++compareRequestRef.current;
      try {
        const [entries, allRuns] = await Promise.all([
          Promise.all(
            [keepId, newRunId].map(async (id) => {
              const view = await api.runDashboardData(id);
              return [id, view] as const;
            })
          ),
          api.listRuns({}),
        ]);
        if (compareRequestRef.current !== requestId) return;
        const matchedRuns = [keepId, newRunId]
          .map((id) => allRuns.find((r) => r.id === id))
          .filter((r): r is RunRow => r != null);
        if (matchedRuns.length < 2) {
          // The new run's row isn't queryable yet (scanner event fired just
          // ahead of the DB write) — the ongoing-run polling above will pick
          // it up once its own run_id lands in the compare set. Leave the
          // current comparison alone rather than risk clearing it.
          return;
        }
        setCompareSnapshots(
          Object.fromEntries(entries.map(([id, view]) => [id, view.chart_snapshots]))
        );
        setCompareWaveSkips(
          Object.fromEntries(entries.map(([id, view]) => [id, view.chart_wave_skips]))
        );
        setCompareNormalJumps(
          Object.fromEntries(entries.map(([id, view]) => [id, view.chart_normal_jumps]))
        );
        setCompareRuns(matchedRuns);
        setChecked(new Set(matchedRuns.map((r) => r.id)));
        savePersistedCompareIds(matchedRuns.map((r) => r.id));
      } catch {
        // Same reasoning as above: quietly skip and let the next scanner
        // update retry.
      }
    },
    []
  );

  // "Follow new run": when the live run in a 2-run comparison ends and a
  // different run starts, automatically swap it in — keeping either the
  // other (older) run in the comparison, or the run that just ended,
  // depending on compareFollowKeepPrevious.
  useEffect(() => {
    if (!compareFollowNewRun || compareRuns.length !== 2 || !hasOngoingCompareRun) {
      return;
    }
    const ids = compareRunIdsKey.split(",");
    const endedLiveId = compareRuns.find((r) => !r.ended_at)?.id;
    const keepId = compareFollowKeepPrevious
      ? endedLiveId
      : compareRuns.find((r) => r.id !== endedLiveId)?.id;
    if (!endedLiveId || !keepId) return;
    let unlisten: (() => void) | undefined;
    void api
      .onScannerUpdate((e) => {
        if (!e.current_run_id || ids.includes(e.current_run_id)) return;
        void followCompareToNewRun(keepId, e.current_run_id);
      })
      .then((fn) => {
        unlisten = fn;
      });
    return () => unlisten?.();
  }, [
    compareFollowNewRun,
    compareFollowKeepPrevious,
    compareRuns,
    compareRunIdsKey,
    hasOngoingCompareRun,
    followCompareToNewRun,
  ]);

  const clearCompare = () => {
    setCompareRuns([]);
    setCompareSnapshots({});
    setCompareWaveSkips({});
    setCompareNormalJumps({});
    savePersistedCompareIds([]);
  };

  const combineSelected = async () => {
    if (checked.size < 2) return;
    const n = checked.size;
    const confirmed = await confirmDialog({
      title: "Combine runs?",
      message:
        `Merge ${n} runs into one? Runs are ordered by start time. Waves must increase across the combined timeline. Source runs will be removed.`,
      confirmLabel: "Combine",
      danger: true,
    });
    if (!confirmed) {
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
      reportUiError(e, "History");
    }
  };

  const flashExport = (message: string) => {
    setExportStatus(message);
    window.setTimeout(() => setExportStatus(null), 2000);
  };

  const runExport = async (
    kind: "csv" | "workbook",
    invokeExport: () => Promise<ExportFileResult>,
    describe: (result: ExportFileResult) => string
  ) => {
    setExportBusy(true);
    setExportStatus("Exporting…");
    setExportProgress(null);
    const unlisten = await api.onExportProgress((e) => {
      if (e.kind === kind) setExportProgress({ done: e.done, total: e.total });
    });
    try {
      const result = await invokeExport();
      flashExport(describe(result));
    } catch (e) {
      setExportStatus(null);
      reportUiError(e, "History");
    } finally {
      unlisten();
      setExportBusy(false);
      setExportProgress(null);
    }
  };

  const exportCsv = () =>
    runExport(
      "csv",
      () => api.exportCsv(listFilter()),
      (result) =>
        `Saved ${result.snapshot_count} snapshot${result.snapshot_count === 1 ? "" : "s"} to ${result.path} ✓`
    );

  const exportWorkbook = () =>
    runExport(
      "workbook",
      () => api.exportWorkbook(listFilter()),
      (result) =>
        `Saved ${result.run_count} run${result.run_count === 1 ? "" : "s"} to ${result.path} ✓`
    );

  const gcSnapshots = useMemo(
    () => snapshots.filter(snapshotHasGoldenCombo),
    [snapshots]
  );

  const toggleCoinSort = (key: CoinSortKey) => {
    if (key === coinSortKey) setCoinSortAsc(!coinSortAsc);
    else {
      setCoinSortKey(key);
      setCoinSortAsc(true);
    }
  };

  const toggleGcSort = (key: GcSortKey) => {
    if (key === gcSortKey) setGcSortAsc(!gcSortAsc);
    else {
      setGcSortKey(key);
      setGcSortAsc(true);
    }
  };

  const toggleSkipSort = (key: SkipSortKey) => {
    if (key === skipSortKey) setSkipSortAsc(!skipSortAsc);
    else {
      setSkipSortKey(key);
      setSkipSortAsc(true);
    }
  };

  const sortedSnapshots = useMemo(
    () =>
      sortByValue(
        snapshots,
        (s) => s[coinSortKey],
        coinSortAsc
      ),
    [snapshots, coinSortKey, coinSortAsc]
  );

  const sortedGcSnapshots = useMemo(
    () =>
      sortByValue(
        gcSnapshots,
        (s) => s[gcSortKey],
        gcSortAsc
      ),
    [gcSnapshots, gcSortKey, gcSortAsc]
  );

  const sortedWaveSkips = useMemo(
    () =>
      sortByValue(
        waveSkips,
        (s) => s[skipSortKey],
        skipSortAsc
      ),
    [waveSkips, skipSortKey, skipSortAsc]
  );

  const selectedGcWaves = useMemo(
    () =>
      gcSnapshots
        .filter((s) => selectedGcIds.has(s.id))
        .map((s) => s.wave),
    [gcSnapshots, selectedGcIds]
  );

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
    setSelectedGcIds(new Set());
  };

  const toggleWaveSkipId = useCallback((id: string) => {
    setSelectedWaveSkipIds((prev) => {
      const next = new Set(prev);
      if (next.has(id)) {
        next.delete(id);
      } else {
        next.add(id);
        waveSkipRowRefs.current.get(id)?.scrollIntoView({
          block: "nearest",
          behavior: "smooth",
        });
      }
      return next;
    });
  }, []);

  const toggleGcId = useCallback((id: string) => {
    setSelectedGcIds((prev) => {
      const next = new Set(prev);
      if (next.has(id)) {
        next.delete(id);
      } else {
        next.add(id);
        gcRowRefs.current.get(id)?.scrollIntoView({
          block: "nearest",
          behavior: "smooth",
        });
      }
      return next;
    });
  }, []);

  const toggleGcWave = useCallback(
    (wave: number) => {
      const snap = snapshotByWave.get(wave);
      if (snap && snapshotHasGoldenCombo(snap)) {
        toggleGcId(snap.id);
      }
    },
    [snapshotByWave, toggleGcId]
  );

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

  const allGcChecked =
    gcSnapshots.length > 0 &&
    gcSnapshots.every((s) => selectedGcIds.has(s.id));

  const toggleAllGc = () => {
    if (allGcChecked) {
      setSelectedGcIds(new Set());
      return;
    }
    setSelectedGcIds(new Set(gcSnapshots.map((s) => s.id)));
  };

  const selectCoinOutliers = () => {
    const ids = idsOutsideBounds(
      snapshots,
      (s) => s.coin_per_minute,
      coinOutlierBelow,
      coinOutlierAbove,
      (s) => s.id,
      parseOptionalCoin
    );
    if (ids === false) {
      reportUiError(
        new Error(
          'Enter valid Below / Above coin values, e.g. "600T" or "1.5q" (leave a side blank to ignore it).'
        ),
        "History"
      );
      return;
    }
    setSelectedSnapshotIds(ids);
  };

  const selectGcOutliers = () => {
    const ids = idsOutsideBounds(
      gcSnapshots,
      (s) =>
        s.golden_combo_caret != null ? Number(s.golden_combo_caret) : null,
      gcOutlierBelow,
      gcOutlierAbove,
      (s) => s.id
    );
    if (ids === false) {
      reportUiError(
        new Error("Enter valid Below / Above numbers (leave a side blank to ignore it)."),
        "History"
      );
      return;
    }
    setSelectedGcIds(ids);
  };

  const selectSkipOutliers = () => {
    const ids = idsOutsideBounds(
      waveSkips,
      (s) => skipChartValue(skipDisplayFromRow(s)),
      skipOutlierBelow,
      skipOutlierAbove,
      (s) => s.id
    );
    if (ids === false) {
      reportUiError(
        new Error("Enter valid Below / Above numbers (leave a side blank to ignore it)."),
        "History"
      );
      return;
    }
    setSelectedWaveSkipIds(ids);
  };

  const beginEditGc = (snap: SnapshotRow) => {
    setEditingGcId(snap.id);
    setGcEditChance(
      snap.golden_combo_chance != null ? String(snap.golden_combo_chance) : ""
    );
    setGcEditCaret(
      snap.golden_combo_caret != null ? String(snap.golden_combo_caret) : ""
    );
    setGcEditMultiplier(
      snap.golden_combo_multiplier != null
        ? String(snap.golden_combo_multiplier)
        : ""
    );
  };

  const cancelEditGc = () => {
    setEditingGcId(null);
  };

  const saveEditGc = async (snap: SnapshotRow) => {
    const chance = parseOptionalNumber(gcEditChance);
    const caret = parseOptionalInt(gcEditCaret);
    const multiplier = parseOptionalNumber(gcEditMultiplier);
    if (chance === false || caret === false || multiplier === false) {
      reportUiError(
        new Error("Enter valid numbers (or leave blank to clear a field)."),
        "History"
      );
      return;
    }
    try {
      await api.updateSnapshotGoldenCombo(snap.id, {
        chance,
        caret,
        multiplier,
      });
      setEditingGcId(null);
      await refreshSelectedRun(true);
    } catch (e) {
      reportUiError(e, "History");
    }
  };

  const ongoingRunNote = () =>
    selected && !selected.ended_at
      ? "\n\nThis run is still open — stop the scanner first or deleted waves may be recorded again."
      : "";

  const selectedRunId = selected?.id ?? null;
  const hasOngoingSelectedRun =
    selected != null && !selected.ended_at && compareRuns.length < 2;

  const refreshSelectedRun = useCallback(async (reloadTable = false) => {
    if (!selectedRunId) return;
    try {
      const activeFilter = listFilter();
      if (reloadTable || !hasOngoingSelectedRun) {
        const [snaps, skips, updatedRuns] = await Promise.all([
          api.runSnapshots(selectedRunId),
          api.runWaveSkips(selectedRunId),
          api.listRuns(activeFilter),
        ]);
        setSnapshots(snaps);
        setWaveSkips(skips);
        setLiveChartSnapshots([]);
        setLiveChartWaveSkips([]);
        setLiveChartNormalJumps([]);
        setSelectedSnapshotIds((prev) =>
          pruneSelectedIds(
            prev,
            snaps.map((s) => s.id)
          )
        );
        setSelectedWaveSkipIds((prev) =>
          pruneSelectedIds(
            prev,
            skips.map((s) => s.id)
          )
        );
        setSelectedGcIds((prev) =>
          pruneSelectedIds(
            prev,
            snaps.filter(snapshotHasGoldenCombo).map((s) => s.id)
          )
        );
        setRuns(updatedRuns);
        setSelected(updatedRuns.find((r) => r.id === selectedRunId) ?? null);
        return;
      }

      // Ongoing run: keep chart on sampled dashboard payload; refresh full tables
      // so coin/min, GC, and jump rows appear as the scanner writes them.
      const [view, snaps, skips, updatedRuns] = await Promise.all([
        api.runDashboardData(selectedRunId),
        api.runSnapshots(selectedRunId),
        api.runWaveSkips(selectedRunId),
        api.listRuns(activeFilter),
      ]);
      setLiveChartSnapshots(view.chart_snapshots);
      setLiveChartWaveSkips(view.chart_wave_skips);
      setLiveChartNormalJumps(view.chart_normal_jumps);
      setSnapshots(snaps);
      setWaveSkips(skips);
      setSelectedSnapshotIds((prev) =>
        pruneSelectedIds(
          prev,
          snaps.map((s) => s.id)
        )
      );
      setSelectedWaveSkipIds((prev) =>
        pruneSelectedIds(
          prev,
          skips.map((s) => s.id)
        )
      );
      setSelectedGcIds((prev) =>
        pruneSelectedIds(
          prev,
          snaps.filter(snapshotHasGoldenCombo).map((s) => s.id)
        )
      );
      setRuns(updatedRuns);
      setSelected(updatedRuns.find((r) => r.id === selectedRunId) ?? null);
    } catch {
      /* keep last chart */
    }
  }, [selectedRunId, listFilter, hasOngoingSelectedRun]);

  useEffect(() => {
    if (!selectedRunId || !hasOngoingSelectedRun) return;
    liveRefreshAtRef.current = 0;
    void refreshSelectedRun();
    const id = window.setInterval(() => void refreshSelectedRun(), 15_000);
    return () => window.clearInterval(id);
  }, [selectedRunId, hasOngoingSelectedRun, refreshSelectedRun]);

  useEffect(() => {
    if (!selectedRunId || !hasOngoingSelectedRun) return;
    let unlisten: (() => void) | undefined;
    void api
      .onScannerUpdate((e) => {
        if (e.current_run_id !== selectedRunId) return;
        const now = Date.now();
        if (now - liveRefreshAtRef.current < LIVE_REFRESH_MIN_MS) return;
        liveRefreshAtRef.current = now;
        void refreshSelectedRun();
      })
      .then((fn) => {
        unlisten = fn;
      });
    return () => unlisten?.();
  }, [selectedRunId, hasOngoingSelectedRun, refreshSelectedRun]);

  const deleteSelectedSnapshots = async () => {
    if (!selected || selectedSnapshotIds.size === 0) return;
    const n = selectedSnapshotIds.size;
    const confirmed = await confirmDialog({
      title: n === 1 ? "Delete snapshot?" : `Delete ${n} snapshots?`,
      message: `This permanently removes the selected snapshot${n === 1 ? "" : "s"}.${ongoingRunNote()}`,
      confirmLabel: "Delete",
      danger: true,
    });
    if (!confirmed) {
      return;
    }
    try {
      await api.deleteSnapshots([...selectedSnapshotIds]);
      setSelectedSnapshotIds(new Set());
      await refreshSelectedRun(true);
    } catch (e) {
      reportUiError(e, "History");
    }
  };

  const deleteSelectedWaveSkips = async () => {
    if (!selected || selectedWaveSkipIds.size === 0) return;
    const n = selectedWaveSkipIds.size;
    const confirmed = await confirmDialog({
      title: n === 1 ? "Delete wave skip?" : `Delete ${n} wave skips?`,
      message: `This removes the selected wave skip record${n === 1 ? "" : "s"}. Coin/min snapshots are kept.${ongoingRunNote()}`,
      confirmLabel: "Delete",
      danger: true,
    });
    if (!confirmed) {
      return;
    }
    try {
      await api.deleteWaveSkips([...selectedWaveSkipIds]);
      setSelectedWaveSkipIds(new Set());
      await refreshSelectedRun(true);
    } catch (e) {
      reportUiError(e, "History");
    }
  };

  const clearSelectedGc = async () => {
    if (!selected || selectedGcIds.size === 0) return;
    const n = selectedGcIds.size;
    const confirmed = await confirmDialog({
      title: n === 1 ? "Clear GC activations?" : `Clear ${n} GC readings?`,
      message: `This clears Golden Combo fields on the selected wave${n === 1 ? "" : "s"}. Coin/min snapshots are kept.${ongoingRunNote()}`,
      confirmLabel: "Clear",
      danger: true,
    });
    if (!confirmed) {
      return;
    }
    try {
      await api.clearSnapshotGoldenCombo([...selectedGcIds]);
      setSelectedGcIds(new Set());
      setEditingGcId(null);
      await refreshSelectedRun(true);
    } catch (e) {
      reportUiError(e, "History");
    }
  };

  const clearGc = async (snap: SnapshotRow) => {
    const confirmed = await confirmDialog({
      title: "Clear GC activations?",
      message: `Clear Golden Combo on wave ${snap.wave} (${formatGoldenCombo(
        snap.golden_combo_chance,
        snap.golden_combo_caret,
        snap.golden_combo_multiplier
      )})? Coin/min is kept.${ongoingRunNote()}`,
      confirmLabel: "Clear",
      danger: true,
    });
    if (!confirmed) {
      return;
    }
    try {
      await api.clearSnapshotGoldenCombo([snap.id]);
      setSelectedGcIds((prev) => {
        const next = new Set(prev);
        next.delete(snap.id);
        return next;
      });
      if (editingGcId === snap.id) {
        setEditingGcId(null);
      }
      await refreshSelectedRun(true);
    } catch (e) {
      reportUiError(e, "History");
    }
  };

  const deleteWaveSkip = async (skip: WaveSkipRow) => {
    if (!selected) return;
    const confirmed = await confirmDialog({
      title: "Delete wave skip?",
      message: `Delete wave skip at wave ${skip.at_wave} (${formatSkipDisplay(skipDisplayFromRow(skip))})?${ongoingRunNote()}`,
      confirmLabel: "Delete",
      danger: true,
    });
    if (!confirmed) {
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
      await refreshSelectedRun(true);
    } catch (e) {
      reportUiError(e, "History");
    }
  };

  const deleteSnapshot = async (snap: SnapshotRow) => {
    if (!selected) return;
    const confirmed = await confirmDialog({
      title: "Delete snapshot?",
      message: `Delete snapshot for wave ${snap.wave} (${formatCoin(snap.coin_per_minute)})?${ongoingRunNote()}`,
      confirmLabel: "Delete",
      danger: true,
    });
    if (!confirmed) {
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
      await refreshSelectedRun(true);
    } catch (e) {
      reportUiError(e, "History");
    }
  };

  const usingLiveChart =
    hasOngoingSelectedRun && liveChartSnapshots.length > 0;
  const chartSnapshotsForDisplay = usingLiveChart
    ? liveChartSnapshots
    : snapshots;
  const chartSkipsForDisplay = usingLiveChart ? liveChartWaveSkips : waveSkips;
  const chartData = snapshotsToChartData(chartSnapshotsForDisplay, {
    alreadySampled: usingLiveChart,
  });
  const skipMarkers = usingLiveChart
    ? buildChartWaveJumpMarkers(liveChartWaveSkips, liveChartNormalJumps)
    : buildWaveJumpMarkers(chartSnapshotsForDisplay, chartSkipsForDisplay);
  const hasJumpsOnChart = skipMarkers.length > 0;
  const hasGcOnChart = chartData.some((d) => d.golden_combo_caret != null);

  const compareChartData = buildCompareChartDataByWave(
    compareRunIds,
    compareSnapshots
  );

  const compareSkipMarkers = compareRunIds.map((id) =>
    buildChartWaveJumpMarkers(
      compareWaveSkips[id] ?? [],
      compareNormalJumps[id] ?? []
    )
  );
  const hasCompareSkips = compareSkipMarkers.some((markers) => markers.length > 0);
  const hasCompareGc = compareHasGoldenComboActivations(
    compareRunIds,
    compareSnapshots
  );

  const compareChartMerged = useMemo(
    () => mergeCompareWithSkips(compareChartData, compareSkipMarkers),
    [compareChartData, compareSkipMarkers]
  );

  const compareChartDisplayData = useMemo(() => {
    if (compareSmoothWindow <= 1) {
      return compareChartMerged;
    }
    return applyCompareChartSmoothing(
      compareChartMerged,
      compareRuns.length,
      compareSmoothWindow
    );
  }, [compareChartMerged, compareRuns.length, compareSmoothWindow]);

  // Lead/lag band works off whichever comparison series is visible: coin/min
  // takes priority, then GC activations, then wave jumps.
  const compareLeadLagMetric: LeadLagMetric | null = compareShowCoin
    ? "coin"
    : compareShowGc && hasCompareGc
      ? "gc"
      : compareShowSkips && hasCompareSkips
        ? "skip"
        : null;

  const compareLeadLag = useMemo(() => {
    if (!compareLeadLagBand || compareRuns.length !== 2 || !compareLeadLagMetric) {
      return null;
    }
    const newerIndex = compareNewerRunIndex(compareRuns);
    if (newerIndex == null) {
      return null;
    }
    return {
      newerIndex,
      olderIndex: 1 - newerIndex,
      metric: compareLeadLagMetric,
    };
  }, [compareLeadLagBand, compareRuns, compareLeadLagMetric]);

  const compareLines: ChartLineConfig[] = compareRuns.map((r, i) => ({
    dataKey: `coin_${i}`,
    name: runShortLabel(r, compareSnapshots[r.id]),
    stroke: COMPARE_COLORS[i % COMPARE_COLORS.length],
  }));

  const goToPage = (raw: string) => {
    const n = Number.parseInt(raw, 10);
    if (!Number.isFinite(n) || n < 1 || n > totalPages) return;
    setPage(n);
    setJumpPage("");
  };

  const renderToolbarActionGroups = (showLabels: boolean) => (
    <>
      {checked.size > 0 && (
        <div className="toolbar-action-group">
          <span className="toolbar-selected-count">
            {checked.size} selected
          </span>
          <button
            type="button"
            className="icon-btn"
            onClick={() => setChecked(new Set())}
            data-tooltip="Clear selection"
            aria-label="Clear selection"
          >
            <CloseIcon />
          </button>
        </div>
      )}
      <div className="toolbar-action-group">
        <button
          type="button"
          className="btn-icon"
          onClick={reload}
          data-tooltip="Refresh the run list"
        >
          <RefreshIcon />
          {showLabels && <span className="btn-icon-label">Refresh</span>}
        </button>
        <button
          type="button"
          className="btn-icon"
          onClick={exportCsv}
          disabled={exportBusy}
          data-tooltip="Export CSV"
        >
          <ExportIcon />
          {showLabels && <span className="btn-icon-label">Export CSV</span>}
        </button>
        <button
          type="button"
          className="btn-icon"
          onClick={exportWorkbook}
          disabled={exportBusy}
          data-tooltip="Export ODS"
        >
          <DocumentIcon />
          {showLabels && <span className="btn-icon-label">Export ODS</span>}
        </button>
      </div>
      <div className="toolbar-action-group">
        <button
          type="button"
          className="btn-icon"
          disabled={checked.size < 2 || compareLoading}
          onClick={compareSelected}
          data-tooltip={
            compareLoading
              ? "Loading…"
              : checked.size < 2
                ? "Select 2+ runs to compare"
                : `Compare selected (${checked.size})`
          }
        >
          <CompareIcon />
          {showLabels && (
            <span className="btn-icon-label">
              Compare{checked.size >= 2 ? ` (${checked.size})` : ""}
            </span>
          )}
        </button>
        <button
          type="button"
          className="btn-icon"
          disabled={checked.size < 2}
          onClick={combineSelected}
          data-tooltip={
            checked.size < 2
              ? "Select 2+ runs to combine"
              : `Combine selected (${checked.size})`
          }
        >
          <CombineIcon />
          {showLabels && (
            <span className="btn-icon-label">
              Combine{checked.size >= 2 ? ` (${checked.size})` : ""}
            </span>
          )}
        </button>
        <button
          type="button"
          className="btn-icon danger"
          disabled={checked.size === 0}
          onClick={deleteSelected}
          data-tooltip={
            checked.size === 0
              ? "Select runs to delete"
              : `Delete selected (${checked.size})`
          }
        >
          <TrashIcon />
          {showLabels && (
            <span className="btn-icon-label">
              Delete{checked.size > 0 ? ` (${checked.size})` : ""}
            </span>
          )}
        </button>
      </div>
    </>
  );

  const renderCompareActions = (showLabels: boolean) => (
    <>
      <div className="toolbar-action-group">
        <button
          type="button"
          className={compareShowCoin ? "btn-icon active" : "btn-icon"}
          onClick={() => setCompareShowCoin((v) => !v)}
          aria-pressed={compareShowCoin}
          data-tooltip="Show coin/min series on the chart"
        >
          <CoinIcon />
          {showLabels && <span className="btn-icon-label">Coin/min</span>}
        </button>
        {hasCompareSkips && (
          <button
            type="button"
            className={compareShowSkips ? "btn-icon active" : "btn-icon"}
            onClick={() => setCompareShowSkips((v) => !v)}
            aria-pressed={compareShowSkips}
            data-tooltip="Show skip/jump markers on the chart"
          >
            <WaveJumpIcon />
            {showLabels && (
              <span className="btn-icon-label">Wave jumps</span>
            )}
          </button>
        )}
        {hasCompareGc && (
          <button
            type="button"
            className={compareShowGc ? "btn-icon active" : "btn-icon"}
            onClick={() => setCompareShowGc((v) => !v)}
            aria-pressed={compareShowGc}
            data-tooltip="Show Golden Combo activation (^) series on the chart"
          >
            <GcActivationIcon />
            {showLabels && (
              <span className="btn-icon-label">GC activations</span>
            )}
          </button>
        )}
      </div>
      <label className="compare-smooth-label">
        Smooth
        <select
          className="compare-axis-select"
          value={compareSmoothWindow}
          onChange={(e) =>
            setCompareSmoothWindow(
              Number(e.target.value) as CompareSmoothWindow
            )
          }
          aria-label="Compare chart smoothing"
        >
          {COMPARE_SMOOTH_WINDOWS.map((w) => (
            <option key={w} value={w}>
              {w === 0 ? "Off" : `${w} pts`}
            </option>
          ))}
        </select>
      </label>
      <div className="toolbar-action-group">
        <button
          type="button"
          className={compareLeadLagBand ? "btn-icon active" : "btn-icon"}
          onClick={() => setCompareLeadLagBand((v) => !v)}
          disabled={compareRuns.length !== 2 || !compareLeadLagMetric}
          aria-pressed={compareLeadLagBand}
          data-tooltip={
            compareRuns.length !== 2
              ? "Lead/lag band is available when comparing exactly 2 runs"
              : !compareLeadLagMetric
                ? "Lead/lag band needs coin/min, GC activations, or wave jumps visible"
                : "Green when the newer run is higher; red when lower"
          }
        >
          <LeadLagIcon />
          {showLabels && (
            <span className="btn-icon-label">Lead/lag band</span>
          )}
        </button>
        <button
          type="button"
          className={compareFollowNewRun ? "btn-icon active" : "btn-icon"}
          onClick={() => setCompareFollowNewRun((v) => !v)}
          aria-pressed={compareFollowNewRun}
          data-tooltip="When the live run in this comparison ends and a new run starts, automatically swap it in so the comparison keeps following your current run."
        >
          <FollowRunIcon />
          {showLabels && (
            <span className="btn-icon-label">Follow new run</span>
          )}
        </button>
        <button
          type="button"
          className={compareFollowKeepPrevious ? "btn-icon active" : "btn-icon"}
          onClick={() => setCompareFollowKeepPrevious((v) => !v)}
          disabled={!compareFollowNewRun}
          aria-pressed={compareFollowKeepPrevious}
          data-tooltip={
            !compareFollowNewRun
              ? "Enable Follow new run first"
              : "Compare the new run against the run that just ended, instead of the older reference run."
          }
        >
          <PreviousRunIcon />
          {showLabels && (
            <span className="btn-icon-label">vs. previous run</span>
          )}
        </button>
        <button
          type="button"
          className="btn-icon"
          onClick={clearCompare}
          data-tooltip="Clear comparison"
        >
          <CloseIcon />
          {showLabels && (
            <span className="btn-icon-label">Clear comparison</span>
          )}
        </button>
        <ChartScreenshotActions
          targetRef={compareChartRef}
          disabled={compareChartData.length === 0}
        />
        <button
          type="button"
          className={chartExpanded ? "btn-icon primary" : "btn-icon"}
          onClick={() => setChartExpanded((v) => !v)}
          data-tooltip={
            chartExpanded ? "Collapse chart (Esc)" : "Expand chart to fill the window"
          }
        >
          {chartExpanded ? <CollapseIcon /> : <ExpandIcon />}
          {showLabels && (
            <span className="btn-icon-label">
              {chartExpanded ? "Collapse" : "Expand"}
            </span>
          )}
        </button>
      </div>
    </>
  );

  return (
    <div className="history">
      <div className="history-toolbar">
        <div className="toolbar-compact-bar" ref={toolbarCompactBarRef}>
          <button
            type="button"
            className="toolbar-filters-btn"
            ref={toolbarFiltersBtnRef}
            aria-expanded={filtersOpen}
            aria-controls="history-filter-drawer"
            onClick={() => setFiltersOpen((v) => !v)}
          >
            <FunnelIcon />
            Filters
            {filtersOpen ? <ChevronUpIcon /> : <ChevronDownIcon />}
          </button>

          <div
            className={
              toolbarActionsCompact
                ? "toolbar-actions toolbar-actions-compact"
                : "toolbar-actions"
            }
          >
            {renderToolbarActionGroups(!toolbarActionsCompact)}
          </div>
        </div>

        {/* Off-screen, always-labeled copy used only to measure whether
            labels would fit — never shown, never reachable. Kept out of
            .toolbar-compact-bar so it can never affect that element's own
            overflow/scrollbar. */}
        <div
          className="toolbar-actions toolbar-actions-shadow"
          aria-hidden="true"
          ref={toolbarActionsShadowRef}
        >
          {renderToolbarActionGroups(true)}
        </div>

        {filtersOpen && (
          <div
            id="history-filter-drawer"
            className="filter-drawer"
            role="search"
            aria-label="Run history filters"
          >
            <label className="filter-field filter-field-grow">
              Run type
              <select
                value={filter.run_type ?? ""}
                onChange={(e) =>
                  setFilter({ ...filter, run_type: e.target.value || undefined })
                }
              >
                <option value="">All run types</option>
                {RUN_TYPE_FILTER_OPTIONS.map((option) => (
                  <option key={option.value} value={option.value}>
                    {option.label}
                  </option>
                ))}
              </select>
            </label>
            <label className="filter-field">
              Min wave
              <input
                type="number"
                placeholder="Any"
                onChange={(e) =>
                  setFilter({
                    ...filter,
                    min_wave: e.target.value ? Number(e.target.value) : undefined,
                  })
                }
              />
            </label>
            <label className="filter-field">
              Min tier
              <input
                type="number"
                placeholder="Any"
                onChange={(e) =>
                  setFilter({
                    ...filter,
                    min_tier: e.target.value ? Number(e.target.value) : undefined,
                  })
                }
              />
            </label>
            <label className="filter-field">
              From date
              <input
                type="date"
                value={dateFrom}
                max={dateTo || undefined}
                onChange={(e) => setDateFrom(e.target.value)}
              />
            </label>
            <label className="filter-field">
              To date
              <input
                type="date"
                value={dateTo}
                min={dateFrom || undefined}
                onChange={(e) => setDateTo(e.target.value)}
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
          </div>
        )}

        {exportStatus && (
          <span
            className="chart-action-status"
            role="status"
            aria-live="polite"
          >
            {exportProgress
              ? `Exporting… ${exportProgress.done}/${exportProgress.total}`
              : exportStatus}
            {exportProgress && (
              <progress
                className="export-progress-bar"
                value={exportProgress.done}
                max={exportProgress.total || 1}
              />
            )}
          </span>
        )}
      </div>

      <div className="history-table-wrap">
      <table>
        <colgroup>
          <col style={{ width: 32 }} />
          <col style={{ width: 210 }} />
          <col style={{ width: 90 }} />
          <col style={{ width: 180 }} />
          <col style={{ width: 56 }} />
          <col style={{ width: 90 }} />
          <col style={{ width: 130 }} />
          <col style={{ width: 100 }} />
          <col style={{ width: 90 }} />
          <col style={{ width: 200 }} />
        </colgroup>
        <thead>
          <tr>
            <th className="check-col" scope="col">
              <input
                type="checkbox"
                checked={allPageChecked}
                onChange={toggleAll}
                aria-label="Select all runs on this page"
              />
            </th>
            <SortableTh
              label="Started"
              active={sortKey === "started_at"}
              sortAsc={sortAsc}
              onSort={() => toggleSort("started_at")}
            />
            <SortableTh
              label="Duration"
              active={sortKey === "duration"}
              sortAsc={sortAsc}
              onSort={() => toggleSort("duration")}
            />
            <SortableTh
              label="Type"
              active={sortKey === "run_type"}
              sortAsc={sortAsc}
              onSort={() => toggleSort("run_type")}
            />
            <SortableTh
              label="Tier"
              active={sortKey === "peak_tier"}
              sortAsc={sortAsc}
              onSort={() => toggleSort("peak_tier")}
            />
            <SortableTh
              label="Final wave"
              active={sortKey === "final_wave"}
              sortAsc={sortAsc}
              onSort={() => toggleSort("final_wave")}
            />
            <SortableTh
              label="Avg coin/min"
              active={sortKey === "avg_coin_per_minute"}
              sortAsc={sortAsc}
              onSort={() => toggleSort("avg_coin_per_minute")}
            />
            <SortableTh
              label="Avg GC ^"
              active={sortKey === "avg_golden_combo_caret"}
              sortAsc={sortAsc}
              onSort={() => toggleSort("avg_golden_combo_caret")}
            />
            <SortableTh
              label="Snapshots"
              active={sortKey === "snapshot_count"}
              sortAsc={sortAsc}
              onSort={() => toggleSort("snapshot_count")}
            />
            <SortableTh
              label="Comment"
              active={sortKey === "comment"}
              sortAsc={sortAsc}
              onSort={() => toggleSort("comment")}
            />
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
              <td className="run-type-col" onClick={(e) => e.stopPropagation()}>
                <select
                  className="run-type-select"
                  value={r.run_type}
                  onChange={(e) => updateRunType(r.id, e.target.value)}
                  aria-label={`Run type for run started ${r.started_at}`}
                >
                  {RUN_TYPE_FILTER_OPTIONS.map((option) => (
                    <option key={option.value} value={option.value}>
                      {option.label}
                    </option>
                  ))}
                </select>
              </td>
              <td>{r.peak_tier ?? "—"}</td>
              <td>{r.final_wave ?? "—"}</td>
              <td>{formatCoin(r.avg_coin_per_minute)}</td>
              <td>{formatAvgGoldenComboCaret(r.avg_golden_combo_caret)}</td>
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
              <td colSpan={10} className="muted">
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
        <div
          className={
            chartExpanded
              ? "chart-card compare-card chart-card--expanded"
              : "chart-card compare-card"
          }
          ref={compareChartRef}
        >
          <div className="chart-card-header">
            <h3 ref={compareTitleRef}>
              Compare {compareRuns.length} runs — coin/min vs wave
              {hasOngoingCompareRun && (
                <span className="muted"> · live</span>
              )}
            </h3>
            <div
              className={
                compareActionsCompact
                  ? "chart-card-actions chart-card-actions-compact"
                  : "chart-card-actions"
              }
            >
              {renderCompareActions(!compareActionsCompact)}
            </div>
          </div>

          {/* Off-screen, always-labeled copy used only to measure whether
              "Clear comparison"/"Expand" would fit with their labels shown. */}
          <div
            className="chart-card-actions chart-card-actions-shadow"
            aria-hidden="true"
            ref={compareActionsShadowRef}
          >
            {renderCompareActions(true)}
          </div>
          {!chartExpanded && (
            <table className="compare-summary">
              <thead>
                <tr>
                  <th>Run</th>
                  <th>Duration</th>
                  <th>Type</th>
                  <th>Peak tier</th>
                  <th>Final wave</th>
                  <th>Avg coin/min</th>
                  <th>Avg GC ^</th>
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
                      {runShortLabel(r, compareSnapshots[r.id])}
                    </td>
                    <td>{duration(r)}</td>
                    <td>
                      {runTypeUsesBadge(r.run_type) ? (
                        <span className="badge">{formatRunType(r.run_type)}</span>
                      ) : (
                        formatRunType(r.run_type)
                      )}
                    </td>
                    <td>{liveRunStats(r, compareSnapshots[r.id]).tier ?? "—"}</td>
                    <td>{liveRunStats(r, compareSnapshots[r.id]).wave ?? "—"}</td>
                    <td>{formatCoin(r.avg_coin_per_minute)}</td>
                    <td>{formatAvgGoldenComboCaret(r.avg_golden_combo_caret)}</td>
                    <td>{r.snapshot_count}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
          <div
            className={
              chartExpanded ? "chart-card-body chart-card-body--expanded" : "chart-card-body"
            }
            ref={expandedChartBodyRef}
          >
            <CoinVsWaveChart
              mode="compare"
              data={compareChartDisplayData}
              lines={compareLines}
              waveSkipsByLine={
                compareShowSkips ? compareSkipMarkers : undefined
              }
              showCoinPerMinute={compareShowCoin}
              showGoldenComboActivations={compareShowGc}
              height={chartExpanded ? expandedChartHeight ?? 320 : 320}
              smoothWindow={compareSmoothWindow}
              leadLagBand={compareLeadLag}
            />
          </div>
          {!chartExpanded && compareSmoothWindow > 1 && (
            <p className="compare-chart-hint muted">
              Smoothing is visual only; summary stats use raw snapshot values.
              {compareLeadLag != null &&
                ` Green: newer run ahead · red: newer run behind (${LEAD_LAG_METRIC_LABEL[compareLeadLag.metric]}).`}
            </p>
          )}
          {!chartExpanded &&
            compareSmoothWindow <= 1 &&
            compareLeadLag != null && (
            <p className="compare-chart-hint muted">
              Lead/lag band ({LEAD_LAG_METRIC_LABEL[compareLeadLag.metric]}): green when
              the newer run (by start time) is higher, red when lower.
            </p>
          )}
        </div>
      )}

      {selected && compareRuns.length < 2 && (
        <>
          <div
            className={
              chartExpanded ? "chart-card chart-card--expanded" : "chart-card"
            }
            ref={chartRef}
          >
            <div className="chart-card-header">
              <h3>
                Run {new Date(selected.started_at).toLocaleString()} — coin/min &amp; GC
                activations vs wave
                {selected.avg_golden_combo_caret != null && (
                  <span className="muted">
                    {" "}
                    · avg GC ^{formatAvgGoldenComboCaret(selected.avg_golden_combo_caret)}
                  </span>
                )}
                {skipMarkers.length > 0 && (
                  <span className="muted">
                    {" "}
                    · {skipMarkers.length} wave skip
                    {skipMarkers.length === 1 ? "" : "s"} (right axis)
                  </span>
                )}
                {hasOngoingSelectedRun && (
                  <span className="muted"> · live</span>
                )}
              </h3>
              <div className="chart-card-actions">
                {chartData.length > 0 && (
                  <label
                    className="checkbox-inline"
                    title="Show coin/min series on the chart"
                  >
                    <input
                      type="checkbox"
                      checked={showCoinPerMinute}
                      onChange={(e) => setShowCoinPerMinute(e.target.checked)}
                      aria-label="Show coin/min on history chart"
                    />
                    Coin/min
                  </label>
                )}
                {hasJumpsOnChart && (
                  <label
                    className="checkbox-inline"
                    title="Show wave jump markers on the chart"
                  >
                    <input
                      type="checkbox"
                      checked={showWaveJumps}
                      onChange={(e) => setShowWaveJumps(e.target.checked)}
                      aria-label="Show wave jumps on history chart"
                    />
                    Wave jumps
                  </label>
                )}
                {hasGcOnChart && (
                  <label
                    className="checkbox-inline"
                    title="Show Golden Combo activation (^) series on the chart"
                  >
                    <input
                      type="checkbox"
                      checked={showGcActivations}
                      onChange={(e) => setShowGcActivations(e.target.checked)}
                      aria-label="Show GC activations on history chart"
                    />
                    GC activations
                  </label>
                )}
                <button
                  type="button"
                  className={chartSelectMode ? "primary" : undefined}
                  onClick={() => {
                    setChartSelectMode((on) => {
                      if (on) {
                        setSelectedSnapshotIds(new Set());
                        setSelectedWaveSkipIds(new Set());
                        setSelectedGcIds(new Set());
                      }
                      return !on;
                    });
                  }}
                >
                  {chartSelectMode ? "Exit select mode" : "Select mode"}
                </button>
                <ChartScreenshotActions
                  targetRef={chartRef}
                  disabled={chartData.length === 0}
                />
                <button
                  type="button"
                  className={chartExpanded ? "primary" : undefined}
                  onClick={() => setChartExpanded((v) => !v)}
                  title={
                    chartExpanded
                      ? "Collapse chart (Esc)"
                      : "Expand chart to fill the window"
                  }
                >
                  {chartExpanded ? "Collapse" : "Expand"}
                </button>
              </div>
            </div>
            <div
              className={
                chartExpanded
                  ? "chart-card-body chart-card-body--expanded"
                  : "chart-card-body"
              }
              ref={expandedChartBodyRef}
            >
              <CoinVsWaveChart
                mode="single"
                data={chartData}
                waveSkips={skipMarkers}
                height={chartExpanded ? expandedChartHeight ?? 300 : 300}
                showWaveJumps={showWaveJumps}
                showCoinPerMinute={showCoinPerMinute}
                showGoldenComboActivations={showGcActivations}
                selectedWaves={
                  chartSelectMode && showCoinPerMinute
                    ? selectedWaves
                    : undefined
                }
                selectedSkipIds={
                  chartSelectMode && showWaveJumps
                    ? [...selectedWaveSkipIds]
                    : undefined
                }
                selectedGcWaves={
                  chartSelectMode && showGcActivations
                    ? selectedGcWaves
                    : undefined
                }
                onPointClick={
                  chartSelectMode && showCoinPerMinute
                    ? toggleSnapshotWave
                    : undefined
                }
                onSkipClick={
                  chartSelectMode && showWaveJumps
                    ? (id) => toggleWaveSkipId(id)
                    : undefined
                }
                onGcClick={
                  chartSelectMode && showGcActivations ? toggleGcWave : undefined
                }
                onSelectWaves={
                  chartSelectMode && showCoinPerMinute
                    ? selectSnapshotWaves
                    : undefined
                }
              />
            </div>
          </div>

          {waveSkips.length > 0 && (
            <SkipCoinAnalytics snapshots={snapshots} waveSkips={waveSkips} />
          )}

          <div className="snapshot-panel">
            <div className="snapshot-panel-header">
              <h3>
                Coin per minute ({snapshots.length}
                {selectedSnapshotIds.size > 0
                  ? ` · ${selectedSnapshotIds.size} selected`
                  : ""}
                )
              </h3>
              <div className="snapshot-panel-actions">
                <OutlierQuickSelect
                  valueLabel="coin/min"
                  below={coinOutlierBelow}
                  above={coinOutlierAbove}
                  onBelowChange={setCoinOutlierBelow}
                  onAboveChange={setCoinOutlierAbove}
                  onSelect={selectCoinOutliers}
                  placeholder="e.g. 600T"
                />
                {chartSelectMode && (
                  <span className="muted">
                    Click coin, jump, or GC points on the chart. Drag a
                    rectangle to select waves. Shift+drag adds to the selection.
                  </span>
                )}
                <button
                  type="button"
                  disabled={
                    selectedSnapshotIds.size === 0 &&
                    selectedWaveSkipIds.size === 0 &&
                    selectedGcIds.size === 0
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
                  Delete coin/min ({selectedSnapshotIds.size})
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
                        aria-label="Select all coin/min snapshots"
                      />
                    </th>
                    <SortableTh
                      label="Wave"
                      active={coinSortKey === "wave"}
                      sortAsc={coinSortAsc}
                      onSort={() => toggleCoinSort("wave")}
                    />
                    <SortableTh
                      label="Tier"
                      active={coinSortKey === "tier"}
                      sortAsc={coinSortAsc}
                      onSort={() => toggleCoinSort("tier")}
                    />
                    <SortableTh
                      label="Coin/min"
                      active={coinSortKey === "coin_per_minute"}
                      sortAsc={coinSortAsc}
                      onSort={() => toggleCoinSort("coin_per_minute")}
                    />
                    <SortableTh
                      label="Recorded"
                      active={coinSortKey === "recorded_at"}
                      sortAsc={coinSortAsc}
                      onSort={() => toggleCoinSort("recorded_at")}
                    />
                    <th />
                  </tr>
                </thead>
                <tbody>
                  {sortedSnapshots.map((s) => (
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
                        No coin/min snapshots in this run.
                      </td>
                    </tr>
                  )}
                </tbody>
              </table>
            </div>
          </div>

          {gcSnapshots.length > 0 && (
            <div className="snapshot-panel">
              <div className="snapshot-panel-header">
                <h3>
                  GC activations ({gcSnapshots.length}
                  {selectedGcIds.size > 0
                    ? ` · ${selectedGcIds.size} selected`
                    : ""}
                  )
                </h3>
                <div className="snapshot-panel-actions">
                  <OutlierQuickSelect
                    valueLabel="activations"
                    below={gcOutlierBelow}
                    above={gcOutlierAbove}
                    onBelowChange={setGcOutlierBelow}
                    onAboveChange={setGcOutlierAbove}
                    onSelect={selectGcOutliers}
                  />
                  <button
                    type="button"
                    className="danger"
                    disabled={selectedGcIds.size === 0}
                    onClick={clearSelectedGc}
                  >
                    Clear GC ({selectedGcIds.size})
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
                          checked={allGcChecked}
                          onChange={toggleAllGc}
                          aria-label="Select all GC activations"
                        />
                      </th>
                      <SortableTh
                        label="Wave"
                        active={gcSortKey === "wave"}
                        sortAsc={gcSortAsc}
                        onSort={() => toggleGcSort("wave")}
                      />
                      <SortableTh
                        label="Chance %"
                        active={gcSortKey === "golden_combo_chance"}
                        sortAsc={gcSortAsc}
                        onSort={() => toggleGcSort("golden_combo_chance")}
                      />
                      <SortableTh
                        label="Activations"
                        active={gcSortKey === "golden_combo_caret"}
                        sortAsc={gcSortAsc}
                        onSort={() => toggleGcSort("golden_combo_caret")}
                      />
                      <SortableTh
                        label="Multiplier"
                        active={gcSortKey === "golden_combo_multiplier"}
                        sortAsc={gcSortAsc}
                        onSort={() => toggleGcSort("golden_combo_multiplier")}
                      />
                      <SortableTh
                        label="Recorded"
                        active={gcSortKey === "recorded_at"}
                        sortAsc={gcSortAsc}
                        onSort={() => toggleGcSort("recorded_at")}
                      />
                      <th />
                    </tr>
                  </thead>
                  <tbody>
                    {sortedGcSnapshots.map((s) => {
                      const editing = editingGcId === s.id;
                      return (
                        <tr
                          key={s.id}
                          ref={(el) => {
                            if (el) gcRowRefs.current.set(s.id, el);
                            else gcRowRefs.current.delete(s.id);
                          }}
                          className={
                            selectedGcIds.has(s.id) ? "snapshot-selected" : ""
                          }
                          onClick={() => toggleGcId(s.id)}
                        >
                          <td
                            className="check-col"
                            onClick={(e) => e.stopPropagation()}
                          >
                            <input
                              type="checkbox"
                              checked={selectedGcIds.has(s.id)}
                              onChange={() => toggleGcId(s.id)}
                              aria-label={`Select GC at wave ${s.wave}`}
                            />
                          </td>
                          <td>{s.wave}</td>
                          {editing ? (
                            <>
                              <td onClick={(e) => e.stopPropagation()}>
                                <input
                                  className="gc-edit-input"
                                  type="text"
                                  inputMode="decimal"
                                  value={gcEditChance}
                                  onChange={(e) =>
                                    setGcEditChance(e.target.value)
                                  }
                                  aria-label="Chance percent"
                                />
                              </td>
                              <td onClick={(e) => e.stopPropagation()}>
                                <input
                                  className="gc-edit-input"
                                  type="text"
                                  inputMode="numeric"
                                  value={gcEditCaret}
                                  onChange={(e) =>
                                    setGcEditCaret(e.target.value)
                                  }
                                  aria-label="Activations"
                                />
                              </td>
                              <td onClick={(e) => e.stopPropagation()}>
                                <input
                                  className="gc-edit-input"
                                  type="text"
                                  inputMode="decimal"
                                  value={gcEditMultiplier}
                                  onChange={(e) =>
                                    setGcEditMultiplier(e.target.value)
                                  }
                                  aria-label="Multiplier"
                                />
                              </td>
                            </>
                          ) : (
                            <>
                              <td>
                                {s.golden_combo_chance != null
                                  ? s.golden_combo_chance
                                  : "—"}
                              </td>
                              <td>
                                {s.golden_combo_caret != null
                                  ? `^${s.golden_combo_caret}`
                                  : "—"}
                              </td>
                              <td>
                                {s.golden_combo_multiplier != null
                                  ? `x${s.golden_combo_multiplier}`
                                  : "—"}
                              </td>
                            </>
                          )}
                          <td>{new Date(s.recorded_at).toLocaleString()}</td>
                          <td
                            className="snapshot-actions"
                            onClick={(e) => e.stopPropagation()}
                          >
                            {editing ? (
                              <>
                                <button
                                  type="button"
                                  className="primary"
                                  onClick={() => void saveEditGc(s)}
                                >
                                  Save
                                </button>
                                <button type="button" onClick={cancelEditGc}>
                                  Cancel
                                </button>
                              </>
                            ) : (
                              <>
                                <button
                                  type="button"
                                  onClick={() => beginEditGc(s)}
                                >
                                  Edit
                                </button>
                                <button
                                  type="button"
                                  className="danger"
                                  onClick={() => void clearGc(s)}
                                >
                                  Clear
                                </button>
                              </>
                            )}
                          </td>
                        </tr>
                      );
                    })}
                  </tbody>
                </table>
              </div>
            </div>
          )}

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
                  <OutlierQuickSelect
                    valueLabel="wave jump"
                    below={skipOutlierBelow}
                    above={skipOutlierAbove}
                    onBelowChange={setSkipOutlierBelow}
                    onAboveChange={setSkipOutlierAbove}
                    onSelect={selectSkipOutliers}
                  />
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
                      <SortableTh
                        label="Wave"
                        active={skipSortKey === "at_wave"}
                        sortAsc={skipSortAsc}
                        onSort={() => toggleSkipSort("at_wave")}
                      />
                      <SortableTh
                        label="Wave jump"
                        active={skipSortKey === "skipped_count"}
                        sortAsc={skipSortAsc}
                        onSort={() => toggleSkipSort("skipped_count")}
                      />
                      <SortableTh
                        label="Coin/min"
                        active={skipSortKey === "coin_per_minute"}
                        sortAsc={skipSortAsc}
                        onSort={() => toggleSkipSort("coin_per_minute")}
                      />
                      <SortableTh
                        label="Recorded"
                        active={skipSortKey === "recorded_at"}
                        sortAsc={skipSortAsc}
                        onSort={() => toggleSkipSort("recorded_at")}
                      />
                      <th />
                    </tr>
                  </thead>
                  <tbody>
                    {sortedWaveSkips.map((s) => (
                      <tr
                        key={s.id}
                        ref={(el) => {
                          if (el) waveSkipRowRefs.current.set(s.id, el);
                          else waveSkipRowRefs.current.delete(s.id);
                        }}
                        className={
                          selectedWaveSkipIds.has(s.id)
                            ? "snapshot-selected"
                            : ""
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
                        <td>{formatSkipDisplay(skipDisplayFromRow(s))}</td>
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

function snapshotHasGoldenCombo(s: SnapshotRow): boolean {
  return (
    s.golden_combo_caret != null ||
    s.golden_combo_chance != null ||
    s.golden_combo_multiplier != null
  );
}

/** Parse an optional numeric field; blank → null, invalid → false. */
function parseOptionalNumber(raw: string): number | null | false {
  const t = raw.trim();
  if (!t) return null;
  const n = Number(t);
  if (!Number.isFinite(n)) return false;
  return n;
}

/** Parse an optional non-negative integer; blank → null, invalid → false. */
function parseOptionalInt(raw: string): number | null | false {
  const n = parseOptionalNumber(raw);
  if (n === false || n === null) return n;
  if (!Number.isInteger(n) || n < 0) return false;
  return n;
}

/**
 * Select row ids where value is strictly below `below` and/or strictly above `above`.
 * Blank bound is ignored. Returns false if either bound is invalid.
 */
function idsOutsideBounds<T>(
  rows: T[],
  getValue: (row: T) => number | null | undefined,
  belowRaw: string,
  aboveRaw: string,
  getId: (row: T) => string,
  parse: (raw: string) => number | null | false = parseOptionalNumber
): Set<string> | false {
  const below = parse(belowRaw);
  const above = parse(aboveRaw);
  if (below === false || above === false) {
    return false;
  }
  if (below == null && above == null) {
    return new Set();
  }
  const ids = new Set<string>();
  for (const row of rows) {
    const v = getValue(row);
    if (v == null || !Number.isFinite(v)) {
      continue;
    }
    if (below != null && v < below) {
      ids.add(getId(row));
      continue;
    }
    if (above != null && v > above) {
      ids.add(getId(row));
    }
  }
  return ids;
}

function FunnelIcon() {
  return (
    <svg viewBox="0 0 24 24" width="14" height="14" aria-hidden="true">
      <path
        d="M4 5h16l-6 8v6l-4-2v-4z"
        fill="none"
        stroke="currentColor"
        strokeWidth="2"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  );
}

function ChevronDownIcon() {
  return (
    <svg viewBox="0 0 24 24" width="12" height="12" aria-hidden="true">
      <path
        d="m6 9 6 6 6-6"
        fill="none"
        stroke="currentColor"
        strokeWidth="2"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  );
}

function ChevronUpIcon() {
  return (
    <svg viewBox="0 0 24 24" width="12" height="12" aria-hidden="true">
      <path
        d="m6 15 6-6 6 6"
        fill="none"
        stroke="currentColor"
        strokeWidth="2"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  );
}

function CloseIcon() {
  return (
    <svg viewBox="0 0 24 24" width="14" height="14" aria-hidden="true">
      <path
        d="M18 6 6 18M6 6l12 12"
        fill="none"
        stroke="currentColor"
        strokeWidth="2"
        strokeLinecap="round"
      />
    </svg>
  );
}

function RefreshIcon() {
  return (
    <svg viewBox="0 0 24 24" width="16" height="16" aria-hidden="true">
      <path
        d="M21 8a9 9 0 0 0-15-3.36L3 8"
        fill="none"
        stroke="currentColor"
        strokeWidth="2"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
      <path
        d="M3 3v5h5"
        fill="none"
        stroke="currentColor"
        strokeWidth="2"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
      <path
        d="M3 16a9 9 0 0 0 15 3.36L21 16"
        fill="none"
        stroke="currentColor"
        strokeWidth="2"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
      <path
        d="M16 16h5v5"
        fill="none"
        stroke="currentColor"
        strokeWidth="2"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  );
}

function ExportIcon() {
  return (
    <svg viewBox="0 0 24 24" width="16" height="16" aria-hidden="true">
      <path
        d="M12 3v12"
        fill="none"
        stroke="currentColor"
        strokeWidth="2"
        strokeLinecap="round"
      />
      <path
        d="M7 10l5 5 5-5"
        fill="none"
        stroke="currentColor"
        strokeWidth="2"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
      <path
        d="M5 21h14"
        fill="none"
        stroke="currentColor"
        strokeWidth="2"
        strokeLinecap="round"
      />
    </svg>
  );
}

function DocumentIcon() {
  return (
    <svg viewBox="0 0 24 24" width="16" height="16" aria-hidden="true">
      <rect
        x="4"
        y="3"
        width="16"
        height="18"
        rx="2"
        fill="none"
        stroke="currentColor"
        strokeWidth="2"
      />
      <path
        d="M8 8h8M8 12h8M8 16h5"
        fill="none"
        stroke="currentColor"
        strokeWidth="2"
        strokeLinecap="round"
      />
    </svg>
  );
}

function CompareIcon() {
  return (
    <svg viewBox="0 0 24 24" width="16" height="16" aria-hidden="true">
      <circle
        cx="9"
        cy="12"
        r="6"
        fill="none"
        stroke="currentColor"
        strokeWidth="2"
      />
      <circle
        cx="15"
        cy="12"
        r="6"
        fill="none"
        stroke="currentColor"
        strokeWidth="2"
      />
    </svg>
  );
}

function CombineIcon() {
  return (
    <svg viewBox="0 0 24 24" width="16" height="16" aria-hidden="true">
      <path
        d="M7 4v6a5 5 0 0 0 5 5 5 5 0 0 0 5-5V4"
        fill="none"
        stroke="currentColor"
        strokeWidth="2"
        strokeLinecap="round"
      />
      <path
        d="M12 15v5"
        fill="none"
        stroke="currentColor"
        strokeWidth="2"
        strokeLinecap="round"
      />
    </svg>
  );
}

function ExpandIcon() {
  return (
    <svg viewBox="0 0 24 24" width="16" height="16" aria-hidden="true">
      <path
        d="M15 3h6v6M9 21H3v-6M21 3l-7 7M3 21l7-7"
        fill="none"
        stroke="currentColor"
        strokeWidth="2"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  );
}

function CollapseIcon() {
  return (
    <svg viewBox="0 0 24 24" width="16" height="16" aria-hidden="true">
      <path
        d="M4 14h6v6M20 10h-6V4M14 10l7-7M3 21l7-7"
        fill="none"
        stroke="currentColor"
        strokeWidth="2"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  );
}

function TrashIcon() {
  return (
    <svg viewBox="0 0 24 24" width="16" height="16" aria-hidden="true">
      <path
        d="M4 7h16M9 7V5a1 1 0 0 1 1-1h4a1 1 0 0 1 1 1v2m2 0-.8 12.1a2 2 0 0 1-2 1.9H9.8a2 2 0 0 1-2-1.9L7 7"
        fill="none"
        stroke="currentColor"
        strokeWidth="2"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  );
}

function OutlierQuickSelect({
  valueLabel,
  below,
  above,
  onBelowChange,
  onAboveChange,
  onSelect,
  placeholder = "—",
}: {
  valueLabel: string;
  below: string;
  above: string;
  onBelowChange: (value: string) => void;
  onAboveChange: (value: string) => void;
  onSelect: () => void;
  placeholder?: string;
}) {
  return (
    <div
      className="outlier-quick-select"
      title={`Select rows where ${valueLabel} is below and/or above the bounds`}
    >
      <label className="outlier-quick-label">
        Below
        <input
          className="gc-edit-input outlier-quick-input"
          type="text"
          inputMode="decimal"
          value={below}
          onChange={(e) => onBelowChange(e.target.value)}
          aria-label={`Select when ${valueLabel} is below`}
          placeholder={placeholder}
        />
      </label>
      <label className="outlier-quick-label">
        Above
        <input
          className="gc-edit-input outlier-quick-input"
          type="text"
          inputMode="decimal"
          value={above}
          onChange={(e) => onAboveChange(e.target.value)}
          aria-label={`Select when ${valueLabel} is above`}
          placeholder={placeholder}
        />
      </label>
      <button type="button" onClick={onSelect}>
        Select
      </button>
    </div>
  );
}

// An ongoing run's `final_wave`/`peak_tier` columns are only synced when it
// ends, so fall back to the live-loaded chart snapshots while it's running.
function liveRunStats(
  r: RunRow,
  liveSnapshots?: SnapshotRow[]
): { wave: number | null; tier: number | null } {
  if (r.ended_at || !liveSnapshots || liveSnapshots.length === 0) {
    return { wave: r.final_wave, tier: r.peak_tier };
  }
  const wave = Math.max(...liveSnapshots.map((s) => s.wave));
  const tiers = liveSnapshots
    .map((s) => s.tier)
    .filter((t): t is number => t != null);
  return { wave, tier: tiers.length > 0 ? Math.max(...tiers) : null };
}

function runShortLabel(r: RunRow, liveSnapshots?: SnapshotRow[]): string {
  const date = new Date(r.started_at).toLocaleString(undefined, {
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
  const { wave, tier } = liveRunStats(r, liveSnapshots);
  return `${date} (T${tier ?? "?"} W${wave ?? "?"})`;
}

/** Local calendar date (YYYY-MM-DD) → UTC ISO start of that local day. */
function localDateToIsoStart(date: string): string {
  return new Date(`${date}T00:00:00`).toISOString();
}

/** Local calendar date (YYYY-MM-DD) → UTC ISO end of that local day. */
function localDateToIsoEnd(date: string): string {
  return new Date(`${date}T23:59:59.999`).toISOString();
}
