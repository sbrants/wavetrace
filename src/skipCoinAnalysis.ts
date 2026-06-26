import { SnapshotRow, WaveSkipRow } from "./api";

/**
 * Skip vs coin/min statistics for a single run.
 *
 * Filters (see Goal.md § Wave skips → Skip vs coin/min analytics):
 * - MIN_COIN (0.1T): ignore near-zero OCR.
 * - MAX_RATIO (3): drop wave-pair ratios outside [1/3, 3] so single-frame misreads
 *   do not dominate medians and Pearson r.
 */
const MIN_COIN = 1e11;
const MAX_RATIO = 3;

export type SkipCoinLagStat = {
  lag: number;
  pearsonR: number | null;
  n: number;
  medianPctChange: number | null;
  medianPctN: number;
};

export type SkipSizeLagStat = {
  skippedCount: number;
  medianPctChange: number;
  n: number;
};

export type SkipCoinAnalysis = {
  skipCount: number;
  analyzedSkips: number;
  strongestAbsR: number;
  strongestLag: number;
  lagStats: SkipCoinLagStat[];
  postLagMedians: { lag: number; medianPct: number; n: number }[];
  bySkipSizeAtLag2: SkipSizeLagStat[];
};

function safeRatio(num: number, den: number): number | null {
  if (den < MIN_COIN || num < MIN_COIN) return null;
  const r = num / den;
  if (r > MAX_RATIO || r < 1 / MAX_RATIO) return null;
  return r;
}

function pearson(xs: number[], ys: number[]): number | null {
  const n = xs.length;
  if (n < 3) return null;
  const mx = xs.reduce((a, b) => a + b, 0) / n;
  const my = ys.reduce((a, b) => a + b, 0) / n;
  let num = 0;
  let dx = 0;
  let dy = 0;
  for (let i = 0; i < n; i++) {
    const x = xs[i] - mx;
    const y = ys[i] - my;
    num += x * y;
    dx += x * x;
    dy += y * y;
  }
  const den = Math.sqrt(dx * dy);
  return den ? num / den : null;
}

function median(values: number[]): number | null {
  if (values.length === 0) return null;
  const sorted = [...values].sort((a, b) => a - b);
  const mid = Math.floor(sorted.length / 2);
  return sorted.length % 2 === 0
    ? (sorted[mid - 1] + sorted[mid]) / 2
    : sorted[mid];
}

function waveCoinMap(snapshots: SnapshotRow[]): Map<number, number> {
  const map = new Map<number, number>();
  for (const s of snapshots) {
    if (s.coin_per_minute != null && s.coin_per_minute > MIN_COIN) {
      map.set(s.wave, s.coin_per_minute);
    }
  }
  return map;
}

type SkipEvent = {
  skippedCount: number;
  lags: Map<number, number>;
};

function buildSkipEvents(
  snaps: Map<number, number>,
  waveSkips: WaveSkipRow[],
  lagMin: number,
  lagMax: number
): SkipEvent[] {
  const events: SkipEvent[] = [];
  for (const skip of waveSkips) {
    const lags = new Map<number, number>();
    for (let lag = lagMin; lag <= lagMax; lag++) {
      const coin = snaps.get(skip.at_wave + lag);
      if (coin != null) {
        lags.set(lag, coin);
      }
    }
    if (lags.size > 0) {
      events.push({ skippedCount: skip.skipped_count, lags });
    }
  }
  return events;
}

/**
 * Relate wave skips to coin/min on the same run (ignoring coin ≤ 0.1T).
 * Looks for lagged effects a few waves after each skip.
 */
export function computeSkipCoinAnalysis(
  snapshots: SnapshotRow[],
  waveSkips: WaveSkipRow[],
  lagMin = -5,
  lagMax = 10
): SkipCoinAnalysis | null {
  if (waveSkips.length === 0) return null;

  const snaps = waveCoinMap(snapshots);
  const events = buildSkipEvents(snaps, waveSkips, lagMin, lagMax);
  const lagStats: SkipCoinLagStat[] = [];

  for (let lag = lagMin; lag <= lagMax; lag++) {
    const xs: number[] = [];
    const ys: number[] = [];
    const pctChanges: number[] = [];

    for (const ev of events) {
      const coin = ev.lags.get(lag);
      if (coin == null) continue;
      xs.push(ev.skippedCount);
      ys.push(coin);
      const baseline = ev.lags.get(-1) ?? ev.lags.get(0);
      if (baseline != null) {
        const ratio = safeRatio(coin, baseline);
        if (ratio != null) {
          pctChanges.push((ratio - 1) * 100);
        }
      }
    }

    lagStats.push({
      lag,
      pearsonR: pearson(xs, ys),
      n: xs.length,
      medianPctChange: median(pctChanges),
      medianPctN: pctChanges.length,
    });
  }

  let strongestAbsR = 0;
  let strongestLag = 0;
  for (const row of lagStats) {
    if (row.pearsonR == null) continue;
    const abs = Math.abs(row.pearsonR);
    if (abs > strongestAbsR) {
      strongestAbsR = abs;
      strongestLag = row.lag;
    }
  }

  const postLagMedians: SkipCoinAnalysis["postLagMedians"] = [];
  for (let lag = 1; lag <= 5; lag++) {
    const row = lagStats.find((s) => s.lag === lag);
    if (row?.medianPctChange != null && row.medianPctN >= 1) {
      postLagMedians.push({
        lag,
        medianPct: row.medianPctChange,
        n: row.medianPctN,
      });
    }
  }

  const buckets = new Map<number, number[]>();
  for (const ev of events) {
    if (ev.skippedCount > 10) continue;
    const pre = ev.lags.get(-1) ?? ev.lags.get(0);
    const post = ev.lags.get(2);
    if (pre == null || post == null) continue;
    const ratio = safeRatio(post, pre);
    if (ratio == null) continue;
    const list = buckets.get(ev.skippedCount) ?? [];
    list.push((ratio - 1) * 100);
    buckets.set(ev.skippedCount, list);
  }

  const bySkipSizeAtLag2: SkipSizeLagStat[] = [...buckets.entries()]
    .filter(([, vals]) => vals.length >= 1)
    .sort(([a], [b]) => a - b)
    .map(([skippedCount, vals]) => ({
      skippedCount,
      medianPctChange: median(vals) ?? 0,
      n: vals.length,
    }));

  return {
    skipCount: waveSkips.length,
    analyzedSkips: events.length,
    strongestAbsR,
    strongestLag,
    lagStats,
    postLagMedians,
    bySkipSizeAtLag2,
  };
}

export function formatLagLabel(lag: number): string {
  if (lag === 0) return "0";
  return lag > 0 ? `+${lag}` : String(lag);
}

export function formatPct(value: number): string {
  const sign = value > 0 ? "+" : "";
  return `${sign}${value.toFixed(1)}%`;
}
