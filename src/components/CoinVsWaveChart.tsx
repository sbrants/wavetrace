import { useCallback, useMemo, useRef, useState, type MouseEvent } from "react";
import {
  ComposedChart,
  LineChart,
  Line,
  XAxis,
  YAxis,
  Tooltip,
  CartesianGrid,
  ResponsiveContainer,
  Legend,
  ReferenceArea,
  Customized,
} from "recharts";
import { formatCoin } from "../api";
import type { CoinChartPoint, CompareChartRow, LeadLagPolygon, WaveSkipMarker } from "../chartData";
import { buildLeadLagPolygons } from "../chartData";

export type ChartLineConfig = {
  dataKey: string;
  name?: string;
  stroke: string;
};

const SKIP_AXIS_MAX = 20;
const SKIP_DOT_R = 3;
const COIN_LINE_COLOR = "#4cc2ff";
const GC_LINE_COLOR = "#6ec6a0";
const SKIP_LINE_COLOR = "#e8b339";

function createSkipDot(
  color: string,
  selectedIds: Set<string>,
  onSkipClick?: (id: string, wave: number) => void
) {
  return (dotProps: {
    cx?: number;
    cy?: number;
    payload?: SingleChartRow;
  }) => {
    const { cx, cy, payload } = dotProps;
    if (cx == null || cy == null || !payload || payload.skip_count <= 0) {
      return <g />;
    }
    const skipId = payload.skip_id;
    const clickable = !!skipId && !!onSkipClick;
    const selected = skipId ? selectedIds.has(skipId) : false;
    return (
      <g
        style={{ cursor: clickable ? "pointer" : undefined }}
        onMouseDown={(e) => {
          if (!clickable) return;
          e.stopPropagation();
          e.preventDefault();
        }}
        onClick={(e) => {
          if (!clickable || !skipId) return;
          e.stopPropagation();
          e.nativeEvent.stopImmediatePropagation();
          onSkipClick?.(skipId, payload.wave);
        }}
      >
        {/* Hit target above the coin line; keep large for easy select-mode clicks. */}
        <circle cx={cx} cy={cy} r={14} fill="transparent" />
        <circle
          cx={cx}
          cy={cy}
          r={selected ? SKIP_DOT_R + 2 : SKIP_DOT_R}
          fill={selected ? "#fff" : color}
          stroke={selected ? color : "#fff"}
          strokeWidth={selected ? 2 : 1}
          style={{ pointerEvents: "none" }}
        />
      </g>
    );
  };
}

function createGcDot(
  color: string,
  selectedWaves: Set<number>,
  onGcClick?: (wave: number) => void
) {
  return (dotProps: {
    cx?: number;
    cy?: number;
    payload?: SingleChartRow;
  }) => {
    const { cx, cy, payload } = dotProps;
    if (
      cx == null ||
      cy == null ||
      !payload ||
      payload.golden_combo_caret == null
    ) {
      return <g />;
    }
    const wave = payload.wave;
    const clickable = !!onGcClick;
    const selected = selectedWaves.has(wave);
    return (
      <g
        style={{ cursor: clickable ? "pointer" : undefined }}
        onMouseDown={(e) => {
          if (!clickable) return;
          e.stopPropagation();
          e.preventDefault();
        }}
        onClick={(e) => {
          if (!clickable) return;
          e.stopPropagation();
          e.nativeEvent.stopImmediatePropagation();
          onGcClick?.(wave);
        }}
      >
        <circle cx={cx} cy={cy} r={14} fill="transparent" />
        <circle
          cx={cx}
          cy={cy}
          r={selected ? SKIP_DOT_R + 2 : SKIP_DOT_R}
          fill={selected ? "#fff" : color}
          stroke={selected ? color : "#fff"}
          strokeWidth={selected ? 2 : 1}
          style={{ pointerEvents: "none" }}
        />
      </g>
    );
  };
}

type SingleChartRow = {
  wave: number;
  coin: number | null;
  golden_combo_caret: number | null;
  skip_count: number;
  skip_id: string | null;
  skip_tooltip: string;
};

function mergeSingleChartData(
  data: CoinChartPoint[],
  waveSkips: WaveSkipMarker[]
): SingleChartRow[] {
  const pointByWave = new Map(data.map((d) => [d.wave, d]));
  const skipByWave = new Map(waveSkips.map((s) => [s.wave, s]));
  const waves = new Set([...pointByWave.keys(), ...skipByWave.keys()]);
  return [...waves].sort((a, b) => a - b).map((wave) => {
    const point = pointByWave.get(wave);
    const skip = skipByWave.get(wave);
    return {
      wave,
      coin: point?.coin ?? null,
      golden_combo_caret: point?.golden_combo_caret ?? null,
      skip_count: skip?.skip_count ?? 0,
      skip_id: skip?.id ?? null,
      skip_tooltip: skip?.skip_tooltip ?? "",
    };
  });
}

function toSingleChartRows(data: CoinChartPoint[]): SingleChartRow[] {
  return data.map((d) => ({
    wave: d.wave,
    coin: d.coin,
    golden_combo_caret: d.golden_combo_caret,
    skip_count: 0,
    skip_id: null,
    skip_tooltip: "",
  }));
}

function mergeCompareWithSkips(
  rows: CompareChartRow[],
  waveSkipsByLine: WaveSkipMarker[][]
): CompareChartRow[] {
  const byX = new Map<number, CompareChartRow>();
  for (const row of rows) {
    byX.set(row.x, { ...row });
  }
  const lineCount = waveSkipsByLine.length;
  for (const row of byX.values()) {
    for (let i = 0; i < lineCount; i++) {
      if (row[`skip_${i}`] == null) {
        row[`skip_${i}`] = 0;
      }
    }
  }
  waveSkipsByLine.forEach((skips, i) => {
    for (const s of skips) {
      const row = byX.get(s.wave) ?? { x: s.wave };
      for (let j = 0; j < lineCount; j++) {
        if (row[`skip_${j}`] == null) {
          row[`skip_${j}`] = 0;
        }
      }
      row[`skip_${i}`] = s.skip_count;
      row[`skip_tip_${i}`] = s.skip_tooltip;
      byX.set(s.wave, row);
    }
  });
  return [...byX.values()].sort((a, b) => a.x - b.x);
}

function waveDomain(
  data: CoinChartPoint[],
  skips: WaveSkipMarker[]
): [number, number] {
  const waves = [...data.map((d) => d.wave), ...skips.map((s) => s.wave)];
  if (waves.length === 0) return [0, 1];
  return [Math.min(...waves), Math.max(...waves)];
}

type PlotOffset = {
  left: number;
  top: number;
  width: number;
  height: number;
};

type SelectionBox = {
  waveMin: number;
  waveMax: number;
  coinMin: number;
  coinMax: number;
};

type ChartMouseState = {
  activeLabel?: string | number;
  chartX?: number;
  chartY?: number;
  activePayload?: Array<{ payload?: SingleChartRow | CoinChartPoint }>;
};

type SingleProps = {
  mode: "single";
  data: CoinChartPoint[];
  waveSkips?: WaveSkipMarker[];
  waveSkipColor?: string;
  /** When false, hide wave-jump markers (default true). */
  showWaveJumps?: boolean;
  /** When false, hide the coin/min series (default true). */
  showCoinPerMinute?: boolean;
  /** When false, hide Golden Combo activation series (default true). */
  showGoldenComboActivations?: boolean;
  height?: number;
  onPointClick?: (wave: number) => void;
  onSelectWaves?: (waves: number[], additive: boolean) => void;
  selectedWaves?: number[];
  onSkipClick?: (id: string, wave: number) => void;
  selectedSkipIds?: string[];
  onGcClick?: (wave: number) => void;
  selectedGcWaves?: number[];
};

type CompareProps = {
  mode: "compare";
  data: CompareChartRow[];
  lines: ChartLineConfig[];
  waveSkipsByLine?: WaveSkipMarker[][];
  /** When false, hide coin/min series for each run (default true). */
  showCoinPerMinute?: boolean;
  /** When true, plot `gc_N` activation series on a right axis. */
  showGoldenComboActivations?: boolean;
  height?: number;
  smoothWindow?: number;
  leadLagBand?: { newerIndex: number; olderIndex: number } | null;
};

export type CoinVsWaveChartProps = SingleProps | CompareProps;

function clamp(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, value));
}

const LEAD_LAG_AHEAD = "rgba(61, 186, 110, 0.22)";
const LEAD_LAG_BEHIND = "rgba(232, 93, 93, 0.22)";

type AxisScale = {
  scale?: (value: number) => number;
  bandwidth?: () => number;
};

function leadLagPolygonsToPixels(
  polygons: LeadLagPolygon[],
  xScale: AxisScale["scale"],
  yScale: AxisScale["scale"]
): { fill: string; d: string }[] {
  if (!xScale || !yScale) {
    return [];
  }
  return polygons.map((poly) => {
    const [p0, p1] = poly.ring;
    const x0 = xScale(p0.x);
    const x1 = xScale(p1.x);
    const d = [
      `M ${x0} ${yScale(p0.newer)}`,
      `L ${x1} ${yScale(p1.newer)}`,
      `L ${x1} ${yScale(p1.older)}`,
      `L ${x0} ${yScale(p0.older)}`,
      "Z",
    ].join(" ");
    return {
      fill: poly.tone === "ahead" ? LEAD_LAG_AHEAD : LEAD_LAG_BEHIND,
      d,
    };
  });
}

function CompareLeadLagLayer({
  polygons,
  xAxisMap,
  yAxisMap,
}: {
  polygons: LeadLagPolygon[];
  xAxisMap?: Record<string, { scale?: AxisScale["scale"] }>;
  yAxisMap?: Record<string, { scale?: AxisScale["scale"] }>;
}) {
  const xAxis = xAxisMap ? Object.values(xAxisMap)[0] : undefined;
  const yAxis = yAxisMap?.coin;
  const paths = leadLagPolygonsToPixels(
    polygons,
    xAxis?.scale,
    yAxis?.scale
  );
  if (paths.length === 0) {
    return null;
  }
  return (
    <g className="compare-lead-lag-band" aria-hidden>
      {paths.map((path, i) => (
        <path key={i} d={path.d} fill={path.fill} stroke="none" />
      ))}
    </g>
  );
}

function compareCoinValue(
  row: CompareChartRow | undefined,
  lineIndex: number
): number | null {
  if (!row) {
    return null;
  }
  const v = row[`coin_${lineIndex}`];
  return typeof v === "number" && Number.isFinite(v) ? v : null;
}

function compareCoinRaw(
  row: CompareChartRow | undefined,
  lineIndex: number
): number | null {
  if (!row) {
    return null;
  }
  const raw = row[`coin_${lineIndex}_raw`];
  if (typeof raw === "number" && Number.isFinite(raw)) {
    return raw;
  }
  return compareCoinValue(row, lineIndex);
}

function formatCompareDelta(
  newer: number,
  older: number
): string {
  if (older === 0) {
    return "";
  }
  const pct = ((newer - older) / Math.abs(older)) * 100;
  const sign = pct >= 0 ? "+" : "";
  return `${sign}${pct.toFixed(1)}% newer vs older`;
}

function SingleRunChart({
  data,
  waveSkips = [],
  waveSkipColor = SKIP_LINE_COLOR,
  showWaveJumps = true,
  showCoinPerMinute = true,
  showGoldenComboActivations = true,
  height,
  onPointClick,
  onSelectWaves,
  selectedWaves = [],
  onSkipClick,
  selectedSkipIds = [],
  onGcClick,
  selectedGcWaves = [],
}: SingleProps) {
  const layoutRef = useRef<PlotOffset | null>(null);
  const dragRef = useRef<{ wave: number; coin: number } | null>(null);
  const draggedRef = useRef(false);
  const suppressClickRef = useRef(false);
  const [selectionBox, setSelectionBox] = useState<SelectionBox | null>(null);
  const selectedSet = new Set(selectedWaves);
  const selectedSkipSet = new Set(selectedSkipIds);
  const selectedGcSet = new Set(selectedGcWaves);
  const selectable = !!onSelectWaves;

  const bounds = useMemo(() => {
    const waves = data.map((d) => d.wave);
    const coins = data
      .map((d) => d.coin)
      .filter((c): c is number => c != null && Number.isFinite(c));
    return {
      waveMin: Math.min(...waves),
      waveMax: Math.max(...waves),
      coinMin: coins.length ? Math.min(...coins) : 0,
      coinMax: coins.length ? Math.max(...coins) : 1,
    };
  }, [data]);

  const pointerToData = useCallback(
    (chartX?: number, chartY?: number): { wave: number; coin: number } | null => {
      const layout = layoutRef.current;
      if (!layout || chartX == null || chartY == null) return null;
      const { left, top, width, height: plotHeight } = layout;
      if (width <= 0 || plotHeight <= 0) return null;

      const waveSpan = bounds.waveMax - bounds.waveMin || 1;
      const coinSpan = bounds.coinMax - bounds.coinMin || 1;
      const tx = clamp((chartX - left) / width, 0, 1);
      const ty = clamp((chartY - top) / plotHeight, 0, 1);
      return {
        wave: bounds.waveMin + tx * waveSpan,
        coin: bounds.coinMax - ty * coinSpan,
      };
    },
    [bounds]
  );

  const updateSelectionBox = useCallback(
    (start: { wave: number; coin: number }, end: { wave: number; coin: number }) => {
      setSelectionBox({
        waveMin: Math.min(start.wave, end.wave),
        waveMax: Math.max(start.wave, end.wave),
        coinMin: Math.min(start.coin, end.coin),
        coinMax: Math.max(start.coin, end.coin),
      });
    },
    []
  );

  const wavesInBox = useCallback(
    (box: SelectionBox): number[] =>
      data
        .filter((point) => {
          if (point.coin == null) return false;
          return (
            point.wave >= box.waveMin &&
            point.wave <= box.waveMax &&
            point.coin >= box.coinMin &&
            point.coin <= box.coinMax
          );
        })
        .map((point) => point.wave),
    [data]
  );

  const handleMouseDown = (state: ChartMouseState, event: MouseEvent) => {
    if (!selectable) return;
    const target = event.target as SVGElement;
    const tag = target.tagName?.toLowerCase();
    if (tag === "circle") return;
    const point = pointerToData(state.chartX, state.chartY);
    if (!point) return;
    dragRef.current = point;
    draggedRef.current = false;
    updateSelectionBox(point, point);
  };

  const handleMouseMove = (state: ChartMouseState) => {
    const start = dragRef.current;
    if (!start || !selectable) return;
    const point = pointerToData(state.chartX, state.chartY);
    if (!point) return;
    if (
      Math.abs(point.wave - start.wave) > 0.01 ||
      Math.abs(point.coin - start.coin) > 0.01
    ) {
      draggedRef.current = true;
    }
    updateSelectionBox(start, point);
  };

  const finishDrag = (additive: boolean) => {
    const box = selectionBox;
    const wasDrag = draggedRef.current;
    dragRef.current = null;
    setSelectionBox(null);
    draggedRef.current = false;
    if (!wasDrag || !box || !onSelectWaves) return;
    suppressClickRef.current = true;
    onSelectWaves(wavesInBox(box), additive);
  };

  const handleMouseUp = (_state: ChartMouseState, event: MouseEvent) => {
    if (!dragRef.current) return;
    finishDrag(event.shiftKey);
  };

  const handleMouseLeave = () => {
    if (!dragRef.current) return;
    finishDrag(false);
  };

  const handleChartClick = () => {
    // Selection is handled only by per-series dots. Chart-level clicks used to
    // always pick coin/min and stole GC / jump clicks in select mode.
    if (suppressClickRef.current) {
      suppressClickRef.current = false;
    }
  };

  const visibleSkips = showWaveJumps ? waveSkips : [];
  const hasSkips = visibleSkips.length > 0;
  const hasGc =
    showGoldenComboActivations &&
    data.some((d) => d.golden_combo_caret != null);
  const chartData = useMemo(
    () =>
      hasSkips
        ? mergeSingleChartData(data, visibleSkips)
        : toSingleChartRows(data),
    [data, visibleSkips, hasSkips]
  );
  const gcDomain = useMemo((): [number, number] | undefined => {
    if (!hasGc) return undefined;
    const values = data
      .map((d) => d.golden_combo_caret)
      .filter((v): v is number => v != null && Number.isFinite(v));
    if (values.length === 0) return undefined;
    const max = Math.max(...values);
    return [0, Math.max(1, Math.ceil(max * 1.05))];
  }, [data, hasGc]);

  if (data.length === 0) {
    return null;
  }

  const Chart = hasSkips || hasGc ? ComposedChart : LineChart;
  const xDomain = waveDomain(data, visibleSkips);
  const rightAxes = (hasSkips ? 1 : 0) + (hasGc ? 1 : 0);
  const rightMargin = rightAxes === 0 ? 12 : rightAxes === 1 ? 44 : 80;
  // Keep a hidden coin axis so marquee selection / reference areas still map.
  const coinAxisHidden = !showCoinPerMinute;

  return (
    <ResponsiveContainer
      width="100%"
      height={height}
      className={selectable ? "chart-marquee" : undefined}
    >
      <Chart
        data={chartData}
        margin={{ top: 8, right: rightMargin, bottom: 8, left: 4 }}
        onMouseDown={handleMouseDown}
        onMouseMove={handleMouseMove}
        onMouseUp={handleMouseUp}
        onMouseLeave={handleMouseLeave}
        onClick={handleChartClick}
      >
        <Customized
          component={(props: { offset?: PlotOffset }) => {
            if (props.offset) {
              layoutRef.current = props.offset;
            }
            return null;
          }}
        />
        <CartesianGrid strokeDasharray="3 3" stroke="#2a3550" />
        <XAxis
          dataKey="wave"
          stroke="#8da2c0"
          type="number"
          domain={xDomain}
          allowDataOverflow
        />
        <YAxis
          yAxisId="coin"
          stroke={COIN_LINE_COLOR}
          tickFormatter={(v: number) => formatCoin(v)}
          width={70}
          hide={coinAxisHidden}
        />
        {hasSkips && (
          <YAxis
            yAxisId="skip"
            orientation="right"
            stroke={waveSkipColor}
            width={36}
            allowDecimals={false}
            domain={[0, SKIP_AXIS_MAX]}
            tickCount={6}
            label={{
              value: "Wave jump",
              angle: 90,
              position: "insideRight",
              fill: waveSkipColor,
              fontSize: 11,
            }}
          />
        )}
        {hasGc && (
          <YAxis
            yAxisId="gc"
            orientation="right"
            stroke={GC_LINE_COLOR}
            width={40}
            allowDecimals={false}
            domain={gcDomain}
            label={{
              value: "GC ^",
              angle: 90,
              position: "insideRight",
              fill: GC_LINE_COLOR,
              fontSize: 11,
            }}
          />
        )}
        <Tooltip
          cursor={
            onPointClick || onSkipClick || onGcClick || selectable
              ? false
              : undefined
          }
          formatter={(v, name, item) => {
            if (String(name).toLowerCase().includes("jump")) {
              const row = (item as { payload?: SingleChartRow })?.payload;
              const value = row?.skip_tooltip?.trim() || String(v ?? "");
              return [value, "Jump"];
            }
            if (String(name).toLowerCase().includes("activation")) {
              return [v == null ? "—" : String(v), "GC activations"];
            }
            return [formatCoin(v as number), name];
          }}
          labelFormatter={(l) => `Wave ${l}`}
          contentStyle={{ background: "#16203a", border: "1px solid #2a3550" }}
        />
        {(hasSkips || hasGc) && <Legend />}
        {selectionBox && (
          <ReferenceArea
            yAxisId="coin"
            x1={selectionBox.waveMin}
            x2={selectionBox.waveMax}
            y1={selectionBox.coinMin}
            y2={selectionBox.coinMax}
            stroke="#e8b339"
            fill="#e8b339"
            fillOpacity={0.2}
            ifOverflow="extendDomain"
          />
        )}
        {/* Coin under GC/skip so their select-mode hit targets stay on top. */}
        {showCoinPerMinute && (
          <Line
            yAxisId="coin"
            type="monotone"
            dataKey="coin"
            name="Coin/min"
            stroke={COIN_LINE_COLOR}
            strokeWidth={2}
            connectNulls
            isAnimationActive={false}
            dot={(dotProps) => {
              const { cx, cy, payload } = dotProps;
              const row = payload as SingleChartRow | CoinChartPoint;
              if (cx == null || cy == null || row.coin == null) {
                return <g key={row.wave} />;
              }
              const wave = row.wave;
              const selected = selectedSet.has(wave);
              const showDots = !!onPointClick || selectable;
              if (!showDots && !selected) {
                return <g key={wave} />;
              }
              return (
                <g
                  key={wave}
                  style={{ cursor: showDots ? "pointer" : undefined }}
                  onMouseDown={(e) => {
                    if (!showDots) return;
                    e.stopPropagation();
                    e.preventDefault();
                  }}
                  onClick={(e) => {
                    if (!showDots) return;
                    e.stopPropagation();
                    e.nativeEvent.stopImmediatePropagation();
                    onPointClick?.(wave);
                  }}
                >
                  <circle cx={cx} cy={cy} r={14} fill="transparent" />
                  <circle
                    cx={cx}
                    cy={cy}
                    r={selected ? 7 : 4}
                    fill={selected ? "#e8b339" : "#16203a"}
                    stroke={selected ? "#fff" : COIN_LINE_COLOR}
                    strokeWidth={2}
                    style={{ pointerEvents: "none" }}
                  />
                </g>
              );
            }}
            activeDot={
              onPointClick || selectable
                ? {
                    r: 7,
                    fill: "#e8b339",
                    stroke: "#fff",
                    strokeWidth: 2,
                    cursor: "pointer",
                  }
                : false
            }
          />
        )}
        {hasSkips && (
          <Line
            yAxisId="skip"
            type="monotone"
            dataKey="skip_count"
            name="Jump"
            stroke={waveSkipColor}
            strokeWidth={1.5}
            isAnimationActive={false}
            dot={
              onSkipClick
                ? createSkipDot(waveSkipColor, selectedSkipSet, onSkipClick)
                : false
            }
            activeDot={false}
          />
        )}
        {hasGc && (
          <Line
            yAxisId="gc"
            type="monotone"
            dataKey="golden_combo_caret"
            name="GC activations"
            stroke={GC_LINE_COLOR}
            strokeWidth={2}
            connectNulls
            isAnimationActive={false}
            dot={
              onGcClick
                ? createGcDot(GC_LINE_COLOR, selectedGcSet, onGcClick)
                : false
            }
            activeDot={false}
          />
        )}
      </Chart>
    </ResponsiveContainer>
  );
}

export default function CoinVsWaveChart(props: CoinVsWaveChartProps) {
  const height = props.height ?? 320;

  if (props.mode === "single") {
    return <SingleRunChart {...props} height={height} />;
  }

  if (props.data.length === 0) {
    return null;
  }

  const smoothWindow = props.smoothWindow ?? 0;
  const showCoinPerMinute = props.showCoinPerMinute !== false;
  const leadLag = showCoinPerMinute ? props.leadLagBand ?? null : null;
  const leadLagPolygons =
    leadLag != null
      ? buildLeadLagPolygons(
          props.data,
          leadLag.newerIndex,
          leadLag.olderIndex
        )
      : [];

  const flatSkips = props.waveSkipsByLine?.flat() ?? [];
  const hasSkips = flatSkips.length > 0;
  const showGc = props.showGoldenComboActivations === true;
  const hasGc =
    showGc &&
    props.lines.some((_, i) =>
      props.data.some((row) => {
        const v = row[`gc_${i}`];
        return typeof v === "number" && Number.isFinite(v);
      })
    );
  const gcDomain: [number, number] | undefined = (() => {
    if (!hasGc) return undefined;
    let max = 0;
    for (const row of props.data) {
      for (let i = 0; i < props.lines.length; i++) {
        const v = row[`gc_${i}`];
        if (typeof v === "number" && Number.isFinite(v) && v > max) {
          max = v;
        }
      }
    }
    return [0, Math.max(1, Math.ceil(max * 1.05))];
  })();
  const chartData = hasSkips
    ? mergeCompareWithSkips(props.data, props.waveSkipsByLine ?? [])
    : props.data;
  const Chart = hasSkips || hasGc ? ComposedChart : LineChart;
  const rightAxes = (hasSkips ? 1 : 0) + (hasGc ? 1 : 0);
  const rightMargin = rightAxes === 0 ? 12 : rightAxes === 1 ? 44 : 80;
  const xDomain: [number, number] | undefined = hasSkips
    ? [
        Math.min(
          ...props.data.map((d) => d.x),
          ...flatSkips.map((s) => s.wave)
        ),
        Math.max(
          ...props.data.map((d) => d.x),
          ...flatSkips.map((s) => s.wave)
        ),
      ]
    : undefined;

  return (
    <ResponsiveContainer width="100%" height={height}>
      <Chart data={chartData} margin={{ top: 8, right: rightMargin, bottom: 8, left: 4 }}>
        <CartesianGrid strokeDasharray="3 3" stroke="#2a3550" />
        <XAxis
          dataKey="x"
          stroke="#8da2c0"
          type="number"
          domain={xDomain}
          allowDataOverflow
        />
        <YAxis
          yAxisId="coin"
          stroke={COIN_LINE_COLOR}
          tickFormatter={(v: number) => formatCoin(v)}
          width={70}
          hide={!showCoinPerMinute}
        />
        {hasSkips && (
          <YAxis
            yAxisId="skip"
            orientation="right"
            stroke={SKIP_LINE_COLOR}
            width={36}
            allowDecimals={false}
            domain={[0, SKIP_AXIS_MAX]}
            tickCount={6}
            label={{
              value: "Wave jump",
              angle: 90,
              position: "insideRight",
              fill: SKIP_LINE_COLOR,
              fontSize: 11,
            }}
          />
        )}
        {hasGc && (
          <YAxis
            yAxisId="gc"
            orientation="right"
            stroke={GC_LINE_COLOR}
            width={40}
            allowDecimals={false}
            domain={gcDomain}
            label={{
              value: "GC ^",
              angle: 90,
              position: "insideRight",
              fill: GC_LINE_COLOR,
              fontSize: 11,
            }}
          />
        )}
        <Tooltip
          formatter={(v, name, item) => {
            if (String(name).toLowerCase().includes("jump")) {
              const dataKey = String(
                (item as { dataKey?: string })?.dataKey ?? ""
              );
              const match = /^skip_(\d+)$/.exec(dataKey);
              const row = (item as { payload?: CompareChartRow })?.payload;
              const tip =
                match && row
                  ? String(row[`skip_tip_${match[1]}`] ?? "")
                  : "";
              const value = tip.trim() || String(v ?? "");
              return [value, name];
            }
            if (String(name).toLowerCase().includes("gc ^")) {
              return [v == null ? "—" : String(v), name];
            }
            const dataKey = String((item as { dataKey?: string })?.dataKey ?? "");
            const coinMatch = /^coin_(\d+)$/.exec(dataKey);
            if (coinMatch) {
              const idx = Number(coinMatch[1]);
              const row = (item as { payload?: CompareChartRow })?.payload;
              const display =
                typeof v === "number" && Number.isFinite(v)
                  ? (v as number)
                  : compareCoinValue(row, idx);
              if (display == null) {
                return ["—", name];
              }
              let text = formatCoin(display);
              if (smoothWindow > 1) {
                const raw = compareCoinRaw(row, idx);
                if (raw != null && Math.abs(raw - display) > display * 0.0001) {
                  text += ` (raw ${formatCoin(raw)})`;
                }
              }
              return [text, name];
            }
            return [formatCoin(v as number), name];
          }}
          labelFormatter={(label, payload) => {
            const base = `Wave ${label}`;
            if (leadLag && payload?.length) {
              const row = payload[0]?.payload as CompareChartRow | undefined;
              const newer = compareCoinValue(row, leadLag.newerIndex);
              const older = compareCoinValue(row, leadLag.olderIndex);
              if (newer != null && older != null) {
                const delta = formatCompareDelta(newer, older);
                return delta ? `${base} · ${delta}` : base;
              }
            }
            return base;
          }}
          contentStyle={{ background: "#16203a", border: "1px solid #2a3550" }}
        />
        {((showCoinPerMinute && props.lines.length > 1) ||
          hasSkips ||
          hasGc) && <Legend />}
        {leadLagPolygons.length > 0 && (
          <Customized
            component={(customProps: {
              xAxisMap?: Record<string, { scale?: AxisScale["scale"] }>;
              yAxisMap?: Record<string, { scale?: AxisScale["scale"] }>;
            }) => (
              <CompareLeadLagLayer
                polygons={leadLagPolygons}
                xAxisMap={customProps.xAxisMap}
                yAxisMap={customProps.yAxisMap}
              />
            )}
          />
        )}
        {hasSkips &&
          props.waveSkipsByLine?.map((skips, i) => {
            if (skips.length === 0) return null;
            const color = props.lines[i]?.stroke ?? SKIP_LINE_COLOR;
            return (
              <Line
                key={`skip-${props.lines[i]?.dataKey ?? i}`}
                yAxisId="skip"
                type="monotone"
                dataKey={`skip_${i}`}
                name={`${props.lines[i]?.name ?? `Run ${i + 1}`} jump`}
                stroke={color}
                strokeWidth={1.5}
                dot={false}
                isAnimationActive={false}
              />
            );
          })}
        {hasGc &&
          props.lines.map((line, i) => (
            <Line
              key={`gc-${line.dataKey}`}
              yAxisId="gc"
              type="monotone"
              dataKey={`gc_${i}`}
              name={`${line.name ?? `Run ${i + 1}`} GC ^`}
              stroke={line.stroke}
              strokeWidth={2}
              strokeDasharray="4 3"
              connectNulls
              dot={false}
              isAnimationActive={false}
            />
          ))}
        {showCoinPerMinute &&
          props.lines.map((line) => (
            <Line
              key={line.dataKey}
              yAxisId="coin"
              type="monotone"
              dataKey={line.dataKey}
              name={line.name}
              stroke={line.stroke}
              dot={false}
              strokeWidth={2}
              connectNulls
            />
          ))}
      </Chart>
    </ResponsiveContainer>
  );
}
