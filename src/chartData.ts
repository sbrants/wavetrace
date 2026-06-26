import { SnapshotRow, WaveSkipRow } from "./api";
import { skipChartValue, skipDisplayFromRow } from "./skipDisplay";

export type CoinChartPoint = { wave: number; coin: number };

export type WaveSkipMarker = {
  id: string;
  wave: number;
  /** Y-axis value (multiplier when known, else wave jump). */
  skip_count: number;
  skip_tooltip: string;
};

export function snapshotsToChartData(
  snapshots: SnapshotRow[]
): CoinChartPoint[] {
  return snapshots
    .filter((s) => s.coin_per_minute !== null)
    .map((s) => ({ wave: s.wave, coin: s.coin_per_minute as number }));
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

/** Chart markers from snapshot wave gaps; merge DB skips for banner multipliers. */
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

export type CompareXAxis = "wave" | "progress";

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

/** Overlay compare series by snapshot index (1 = first snapshot in each run). */
export function buildCompareChartDataByProgress(
  runIds: string[],
  snapshots: Record<string, SnapshotRow[]>
): CompareChartRow[] {
  const series = runIds.map((id) =>
    (snapshots[id] ?? [])
      .filter((s) => s.coin_per_minute !== null)
      .sort((a, b) => a.wave - b.wave)
  );
  const maxLen = Math.max(0, ...series.map((pts) => pts.length));
  return Array.from({ length: maxLen }, (_, i) => {
    const row: CompareChartRow = { x: i + 1 };
    series.forEach((pts, ri) => {
      const snap = pts[i];
      row[`coin_${ri}`] = snap?.coin_per_minute ?? null;
      row[`wave_${ri}`] = snap?.wave ?? null;
    });
    return row;
  });
}
