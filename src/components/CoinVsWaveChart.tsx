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
import type { CoinChartPoint, CompareChartRow, WaveSkipMarker } from "../chartData";

export type ChartLineConfig = {
  dataKey: string;
  name?: string;
  stroke: string;
};

const SKIP_AXIS_MAX = 20;
const SKIP_DOT_R = 3;

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
      <g>
        {clickable && (
          <circle
            cx={cx}
            cy={cy}
            r={12}
            fill="transparent"
            style={{ cursor: "pointer" }}
            onMouseDown={(e) => e.stopPropagation()}
            onClick={(e) => {
              e.stopPropagation();
              onSkipClick?.(skipId!, payload.wave);
            }}
          />
        )}
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
  skip_count: number;
  skip_id: string | null;
  skip_tooltip: string;
};

function mergeSingleChartData(
  data: CoinChartPoint[],
  waveSkips: WaveSkipMarker[]
): SingleChartRow[] {
  const coinByWave = new Map(data.map((d) => [d.wave, d.coin]));
  const skipByWave = new Map(waveSkips.map((s) => [s.wave, s]));
  const waves = new Set([...coinByWave.keys(), ...skipByWave.keys()]);
  return [...waves].sort((a, b) => a - b).map((wave) => {
    const skip = skipByWave.get(wave);
    return {
      wave,
      coin: coinByWave.get(wave) ?? null,
      skip_count: skip?.skip_count ?? 0,
      skip_id: skip?.id ?? null,
      skip_tooltip: skip?.skip_tooltip ?? "",
    };
  });
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
  height?: number;
  onPointClick?: (wave: number) => void;
  onSelectWaves?: (waves: number[], additive: boolean) => void;
  selectedWaves?: number[];
  onSkipClick?: (id: string, wave: number) => void;
  selectedSkipIds?: string[];
};

type CompareProps = {
  mode: "compare";
  data: CompareChartRow[];
  lines: ChartLineConfig[];
  waveSkipsByLine?: WaveSkipMarker[][];
  xAxis: "wave" | "progress";
  height?: number;
};

export type CoinVsWaveChartProps = SingleProps | CompareProps;

function clamp(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, value));
}

function SingleRunChart({
  data,
  waveSkips = [],
  waveSkipColor = "#e8b339",
  height,
  onPointClick,
  onSelectWaves,
  selectedWaves = [],
  onSkipClick,
  selectedSkipIds = [],
}: SingleProps) {
  const layoutRef = useRef<PlotOffset | null>(null);
  const dragRef = useRef<{ wave: number; coin: number } | null>(null);
  const draggedRef = useRef(false);
  const suppressClickRef = useRef(false);
  const [selectionBox, setSelectionBox] = useState<SelectionBox | null>(null);
  const selectedSet = new Set(selectedWaves);
  const selectedSkipSet = new Set(selectedSkipIds);
  const selectable = !!onSelectWaves;

  const bounds = useMemo(() => {
    const waves = data.map((d) => d.wave);
    const coins = data.map((d) => d.coin);
    return {
      waveMin: Math.min(...waves),
      waveMax: Math.max(...waves),
      coinMin: Math.min(...coins),
      coinMax: Math.max(...coins),
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
        .filter(
          (point) =>
            point.wave >= box.waveMin &&
            point.wave <= box.waveMax &&
            point.coin >= box.coinMin &&
            point.coin <= box.coinMax
        )
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

  const handleChartClick = (state: ChartMouseState) => {
    if (suppressClickRef.current) {
      suppressClickRef.current = false;
      return;
    }
    const payloads = state.activePayload;
    if (payloads?.length) {
      for (const item of payloads) {
        const row = item.payload;
        if (row && row.coin != null && onPointClick) {
          onPointClick(row.wave);
          return;
        }
      }
    }
    if (onPointClick && state?.activeLabel != null) {
      onPointClick(Number(state.activeLabel));
    }
  };

  const hasSkips = waveSkips.length > 0;
  const chartData = useMemo(
    () => (hasSkips ? mergeSingleChartData(data, waveSkips) : data),
    [data, waveSkips, hasSkips]
  );

  if (data.length === 0) {
    return null;
  }

  const Chart = hasSkips ? ComposedChart : LineChart;
  const xDomain = waveDomain(data, waveSkips);

  return (
    <ResponsiveContainer
      width="100%"
      height={height}
      className={selectable ? "chart-marquee" : undefined}
    >
      <Chart
        data={chartData}
        margin={{ top: 8, right: hasSkips ? 44 : 12, bottom: 8, left: 4 }}
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
          stroke="#8da2c0"
          tickFormatter={(v: number) => formatCoin(v)}
          width={70}
        />
        {hasSkips && (
          <YAxis
            yAxisId="skip"
            orientation="right"
            stroke="#8da2c0"
            width={36}
            allowDecimals={false}
            domain={[0, SKIP_AXIS_MAX]}
            tickCount={6}
            label={{
              value: "Wave jump",
              angle: 90,
              position: "insideRight",
              fill: "#8da2c0",
              fontSize: 11,
            }}
          />
        )}
        <Tooltip
          formatter={(v, name, item) => {
            if (String(name).toLowerCase().includes("jump")) {
              const row = (item as { payload?: SingleChartRow })?.payload;
              const value = row?.skip_tooltip?.trim() || String(v ?? "");
              return [value, "Jump"];
            }
            return [formatCoin(v as number), name];
          }}
          labelFormatter={(l) => `Wave ${l}`}
          contentStyle={{ background: "#16203a", border: "1px solid #2a3550" }}
        />
        {hasSkips && <Legend />}
        {selectionBox && (
          <ReferenceArea
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
            activeDot={
              onSkipClick
                ? { r: SKIP_DOT_R + 1, fill: waveSkipColor, stroke: "#fff" }
                : false
            }
          />
        )}
        <Line
          yAxisId="coin"
          type="monotone"
          dataKey="coin"
          name="Coin/min"
          stroke="#4cc2ff"
          strokeWidth={2}
          isAnimationActive={false}
          dot={(dotProps) => {
            const { cx, cy, payload } = dotProps;
            const row = payload as SingleChartRow | CoinChartPoint;
            if (cx == null || cy == null || row.coin == null) {
              return <g key={row.wave} />;
            }
            const wave = row.wave;
            const selected = selectedSet.has(wave);
            const visible = selected || !!onPointClick || selectable;
            return (
              <g key={wave}>
                <circle
                  cx={cx}
                  cy={cy}
                  r={12}
                  fill="transparent"
                  style={{
                    cursor: onPointClick || selectable ? "pointer" : undefined,
                  }}
                  onMouseDown={(e) => e.stopPropagation()}
                  onClick={(e) => {
                    e.stopPropagation();
                    onPointClick?.(wave);
                  }}
                />
                <circle
                  cx={cx}
                  cy={cy}
                  r={selected ? 7 : visible ? 4 : 0}
                  fill={selected ? "#e8b339" : "#16203a"}
                  stroke={selected ? "#fff" : "#4cc2ff"}
                  strokeWidth={visible ? 2 : 0}
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

  const progress = props.xAxis === "progress";
  const flatSkips = props.waveSkipsByLine?.flat() ?? [];
  const hasSkips = !progress && flatSkips.length > 0;
  const chartData = hasSkips
    ? mergeCompareWithSkips(props.data, props.waveSkipsByLine ?? [])
    : props.data;
  const Chart = hasSkips ? ComposedChart : LineChart;
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
      <Chart data={chartData} margin={{ top: 8, right: hasSkips ? 44 : 12, bottom: 8, left: 4 }}>
        <CartesianGrid strokeDasharray="3 3" stroke="#2a3550" />
        <XAxis
          dataKey="x"
          stroke="#8da2c0"
          type="number"
          domain={xDomain}
          allowDataOverflow
          label={
            progress
              ? { value: "Snapshot #", position: "insideBottom", offset: -4 }
              : undefined
          }
        />
        <YAxis
          yAxisId="coin"
          stroke="#8da2c0"
          tickFormatter={(v: number) => formatCoin(v)}
          width={70}
        />
        {hasSkips && (
          <YAxis
            yAxisId="skip"
            orientation="right"
            stroke="#8da2c0"
            width={36}
            allowDecimals={false}
            domain={[0, SKIP_AXIS_MAX]}
            tickCount={6}
            label={{
              value: "Wave jump",
              angle: 90,
              position: "insideRight",
              fill: "#8da2c0",
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
            return [formatCoin(v as number), name];
          }}
          labelFormatter={(label, payload) => {
            if (!progress) {
              return `Wave ${label}`;
            }
            const items = payload ?? [];
            const waves = items
              .map((item) => {
                const key = String(item.dataKey ?? "").replace("coin_", "wave_");
                const row = item.payload as CompareChartRow;
                const wave = row[key];
                const name = item.name ?? "Run";
                return wave != null ? `${name}: wave ${wave}` : null;
              })
              .filter(Boolean);
            return waves.length > 0
              ? `Snapshot ${label} (${waves.join(", ")})`
              : `Snapshot ${label}`;
          }}
          contentStyle={{ background: "#16203a", border: "1px solid #2a3550" }}
        />
        {props.lines.length > 1 && <Legend />}
        {hasSkips &&
          props.waveSkipsByLine?.map((skips, i) => {
            if (skips.length === 0) return null;
            const color = props.lines[i]?.stroke ?? "#e8b339";
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
        {props.lines.map((line) => (
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
