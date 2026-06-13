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
import { formatCoin } from "../api";
import type { CoinChartPoint, CompareChartRow } from "../chartData";

export type ChartLineConfig = {
  dataKey: string;
  name?: string;
  stroke: string;
};

type SingleProps = {
  mode: "single";
  data: CoinChartPoint[];
  height?: number;
};

type CompareProps = {
  mode: "compare";
  data: CompareChartRow[];
  lines: ChartLineConfig[];
  xAxis: "wave" | "progress";
  height?: number;
};

export type CoinVsWaveChartProps = SingleProps | CompareProps;

export default function CoinVsWaveChart(props: CoinVsWaveChartProps) {
  const height = props.height ?? 320;

  if (props.mode === "single") {
    if (props.data.length === 0) {
      return null;
    }
    return (
      <ResponsiveContainer width="100%" height={height}>
        <LineChart data={props.data}>
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
    );
  }

  if (props.data.length === 0) {
    return null;
  }

  const progress = props.xAxis === "progress";

  return (
    <ResponsiveContainer width="100%" height={height}>
      <LineChart data={props.data}>
        <CartesianGrid strokeDasharray="3 3" stroke="#2a3550" />
        <XAxis
          dataKey="x"
          stroke="#8da2c0"
          label={
            progress
              ? { value: "Snapshot #", position: "insideBottom", offset: -4 }
              : undefined
          }
        />
        <YAxis
          stroke="#8da2c0"
          tickFormatter={(v: number) => formatCoin(v)}
          width={70}
        />
        <Tooltip
          formatter={(v) => formatCoin(v as number)}
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
        {props.lines.map((line) => (
          <Line
            key={line.dataKey}
            type="monotone"
            dataKey={line.dataKey}
            name={line.name}
            stroke={line.stroke}
            dot={false}
            strokeWidth={2}
            connectNulls
          />
        ))}
      </LineChart>
    </ResponsiveContainer>
  );
}
