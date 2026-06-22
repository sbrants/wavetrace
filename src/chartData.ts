import { SnapshotRow, WaveSkipRow } from "./api";

export type CoinChartPoint = { wave: number; coin: number };

export type WaveSkipMarker = { id: string; wave: number; skip_count: number };

export function snapshotsToChartData(
  snapshots: SnapshotRow[]
): CoinChartPoint[] {
  return snapshots
    .filter((s) => s.coin_per_minute !== null)
    .map((s) => ({ wave: s.wave, coin: s.coin_per_minute as number }));
}

export function waveSkipsToMarkers(rows: WaveSkipRow[]): WaveSkipMarker[] {
  return rows.map((r) => ({
    id: r.id,
    wave: r.at_wave,
    skip_count: r.skipped_count,
  }));
}

export type CompareXAxis = "wave" | "progress";

export type CompareChartRow = {
  x: number;
  [key: string]: number | null | undefined;
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
