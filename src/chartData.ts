import { SnapshotRow, WaveSkipRow } from "./api";
import { skipChartValue, skipDisplayFromRow } from "./skipDisplay";

export type CoinChartPoint = { wave: number; coin: number };

/** Keep Recharts responsive during very long runs. Sync with `CHART_SNAPSHOT_LIMIT` in `src-tauri/src/db.rs`. */
export const MAX_CHART_POINTS = 5000;

/** Skip-axis cap; sampled independently of snapshots. Sync with `CHART_SKIP_LIMIT` in `src-tauri/src/db.rs`. */
export const MAX_CHART_SKIPS = 5000;

export function downsampleChartData(
  points: CoinChartPoint[],
  maxPoints = MAX_CHART_POINTS
): CoinChartPoint[] {
  if (points.length <= maxPoints) {
    return points;
  }
  const out: CoinChartPoint[] = [];
  const lastIndex = points.length - 1;
  for (let i = 0; i < maxPoints; i += 1) {
    const index = Math.min(lastIndex, Math.round((i * lastIndex) / (maxPoints - 1)));
    out.push(points[index]);
  }
  return out;
}

export type WaveSkipMarker = {
  id: string;
  wave: number;
  /** Y-axis value (multiplier when known, else wave jump). */
  skip_count: number;
  skip_tooltip: string;
};

export function snapshotsToChartData(
  snapshots: SnapshotRow[],
  options?: { alreadySampled?: boolean }
): CoinChartPoint[] {
  const points = snapshots
    .filter((s) => s.coin_per_minute !== null)
    .map((s) => ({ wave: s.wave, coin: s.coin_per_minute as number }));
  return options?.alreadySampled ? points : downsampleChartData(points);
}

function markerFromSkipRow(row: WaveSkipRow): WaveSkipMarker {
  const display = skipDisplayFromRow(row);
  return {
    id: row.id,
    wave: row.at_wave,
    skip_count: skipChartValue(display),
    skip_tooltip: String(display.value),
  };
}

/** Chart markers from independently sampled skip rows and +1 jump waves. */
export function buildChartWaveJumpMarkers(
  waveSkips: WaveSkipRow[],
  normalJumpWaves: number[] = []
): WaveSkipMarker[] {
  const skipByWave = new Map(waveSkips.map((row) => [row.at_wave, row]));
  const markers: WaveSkipMarker[] = waveSkips.map(markerFromSkipRow);

  for (const wave of normalJumpWaves) {
    if (!skipByWave.has(wave)) {
      markers.push({
        id: `wave-jump-${wave}`,
        wave,
        skip_count: 1,
        skip_tooltip: "1",
      });
    }
  }

  return markers.sort((a, b) => a.wave - b.wave);
}

/** Chart markers from full snapshot/skip data (History tables). */
export function buildWaveJumpMarkers(
  snapshots: SnapshotRow[],
  waveSkips: WaveSkipRow[] = []
): WaveSkipMarker[] {
  const sorted = [...snapshots].sort(
    (a, b) => a.wave - b.wave || a.recorded_at.localeCompare(b.recorded_at)
  );
  const skipByWave = new Map(waveSkips.map((row) => [row.at_wave, row]));
  const markers: WaveSkipMarker[] = [];

  for (let i = 1; i < sorted.length; i++) {
    const prevWave = sorted[i - 1].wave;
    const wave = sorted[i].wave;
    if (wave <= prevWave) continue;
    const jump = wave - prevWave;
    const skip = skipByWave.get(wave);
    if (skip) {
      markers.push(markerFromSkipRow(skip));
      skipByWave.delete(wave);
    } else if (jump === 1) {
      // Larger gaps without a recorded skip are usually scanner downtime or
      // missed OCR — not a trustworthy in-game jump.
      markers.push({
        id: `wave-jump-${wave}`,
        wave,
        skip_count: 1,
        skip_tooltip: "1",
      });
    }
  }

  for (const row of skipByWave.values()) {
    markers.push(markerFromSkipRow(row));
  }

  return markers.sort((a, b) => a.wave - b.wave);
}

export type CompareChartRow = {
  x: number;
  [key: string]: number | string | null | undefined;
};

/** Overlay compare series by absolute in-game wave number. */
export function buildCompareChartDataByWave(
  runIds: string[],
  snapshots: Record<string, SnapshotRow[]>
): CompareChartRow[] {
  const waves = new Set<number>();
  for (const id of runIds) {
    for (const s of snapshots[id] ?? []) {
      if (s.coin_per_minute !== null) {
        waves.add(s.wave);
      }
    }
  }
  return [...waves].sort((a, b) => a - b).map((wave) => {
    const row: CompareChartRow = { x: wave };
    runIds.forEach((id, i) => {
      const snap = (snapshots[id] ?? []).find((s) => s.wave === wave);
      row[`coin_${i}`] = snap?.coin_per_minute ?? null;
    });
    return row;
  });
}

/** Centered moving average; nulls are skipped and do not contribute to the window. */
export function smoothNullableSeries(
  values: (number | null)[],
  window: number
): (number | null)[] {
  if (window <= 1) {
    return values;
  }
  const half = Math.floor(window / 2);
  return values.map((_, i) => {
    let sum = 0;
    let count = 0;
    for (let j = i - half; j <= i + half; j += 1) {
      if (j < 0 || j >= values.length) {
        continue;
      }
      const v = values[j];
      if (v != null && Number.isFinite(v)) {
        sum += v;
        count += 1;
      }
    }
    if (count === 0) {
      return null;
    }
    return sum / count;
  });
}

/** Adds `coin_N_raw` and replaces `coin_N` with smoothed values when window > 1. */
export function applyCompareChartSmoothing(
  rows: CompareChartRow[],
  lineCount: number,
  window: number
): CompareChartRow[] {
  if (window <= 1 || lineCount === 0) {
    return rows;
  }
  const keys = Array.from({ length: lineCount }, (_, i) => `coin_${i}`);
  const series = keys.map((key) =>
    rows.map((row) => {
      const v = row[key];
      return typeof v === "number" && Number.isFinite(v) ? v : null;
    })
  );
  const smoothed = series.map((s) => smoothNullableSeries(s, window));
  return rows.map((row, rowIndex) => {
    const next: CompareChartRow = { ...row };
    keys.forEach((key, lineIndex) => {
      const raw = row[key];
      next[`${key}_raw`] =
        typeof raw === "number" && Number.isFinite(raw) ? raw : null;
      next[key] = smoothed[lineIndex][rowIndex];
    });
    return next;
  });
}

/** Index of the run with the later `started_at` (compare list order). */
export function compareNewerRunIndex(
  runs: { started_at: string }[]
): number | null {
  if (runs.length !== 2) {
    return null;
  }
  const t0 = Date.parse(runs[0].started_at);
  const t1 = Date.parse(runs[1].started_at);
  if (!Number.isFinite(t0) || !Number.isFinite(t1)) {
    return null;
  }
  return t0 >= t1 ? 0 : 1;
}

export type LeadLagPolygon = {
  tone: "ahead" | "behind";
  ring: { x: number; newer: number; older: number }[];
};

type LeadLagPoint = { x: number; newer: number; older: number };

function leadLagQuad(
  p0: LeadLagPoint,
  p1: LeadLagPoint,
  tone: "ahead" | "behind"
): LeadLagPolygon {
  return {
    tone,
    ring: [
      { x: p0.x, newer: p0.newer, older: p0.older },
      { x: p1.x, newer: p1.newer, older: p1.older },
      { x: p1.x, newer: p1.older, older: p1.older },
      { x: p0.x, newer: p0.older, older: p0.older },
    ],
  };
}

/** Polygons between newer and older coin/min series (linear between chart knots). */
export function buildLeadLagPolygons(
  rows: CompareChartRow[],
  newerIndex: number,
  olderIndex: number
): LeadLagPolygon[] {
  const nKey = `coin_${newerIndex}`;
  const oKey = `coin_${olderIndex}`;
  const knots: LeadLagPoint[] = [];
  for (const row of rows) {
    const newer = row[nKey];
    const older = row[oKey];
    if (
      typeof newer !== "number" ||
      typeof older !== "number" ||
      !Number.isFinite(newer) ||
      !Number.isFinite(older)
    ) {
      continue;
    }
    knots.push({ x: row.x, newer, older });
  }
  const out: LeadLagPolygon[] = [];
  for (let i = 0; i < knots.length - 1; i += 1) {
    const p0 = knots[i];
    const p1 = knots[i + 1];
    const d0 = p0.newer - p0.older;
    const d1 = p1.newer - p1.older;
    if (d0 === 0 && d1 === 0) {
      continue;
    }
    if (d0 * d1 < 0) {
      const t = d0 / (d0 - d1);
      const cross: LeadLagPoint = {
        x: p0.x + t * (p1.x - p0.x),
        newer: p0.newer + t * (p1.newer - p0.newer),
        older: p0.older + t * (p1.older - p0.older),
      };
      if (d0 > 0) {
        out.push(leadLagQuad(p0, cross, "ahead"));
        out.push(leadLagQuad(cross, p1, "behind"));
      } else {
        out.push(leadLagQuad(p0, cross, "behind"));
        out.push(leadLagQuad(cross, p1, "ahead"));
      }
      continue;
    }
    const tone: "ahead" | "behind" = d0 > 0 || d1 > 0 ? "ahead" : "behind";
    out.push(leadLagQuad(p0, p1, tone));
  }
  return out;
}
