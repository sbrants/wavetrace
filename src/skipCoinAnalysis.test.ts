import { describe, expect, it } from "vitest";
import { computeSkipCoinAnalysis, formatLagLabel, formatPct } from "./skipCoinAnalysis";
import type { SnapshotRow, WaveSkipRow } from "./api";

const MIN_COIN = 1e11;

function snapshot(wave: number, coinPerMinute: number | null): SnapshotRow {
  return {
    id: `s-${wave}`,
    wave,
    tier: null,
    coin_per_minute: coinPerMinute,
    golden_combo_chance: null,
    golden_combo_caret: null,
    golden_combo_multiplier: null,
    recorded_at: "2026-01-01T00:00:00Z",
  };
}

function skip(atWave: number, skippedCount: number): WaveSkipRow {
  return {
    id: `skip-${atWave}`,
    at_wave: atWave,
    skipped_count: skippedCount,
    skip_multiplier: null,
    coin_per_minute: null,
    recorded_at: "2026-01-01T00:00:00Z",
  };
}

describe("computeSkipCoinAnalysis", () => {
  it("returns null when there are no wave skips", () => {
    expect(computeSkipCoinAnalysis([snapshot(1, MIN_COIN * 2)], [])).toBeNull();
  });

  it("ignores snapshots at or below the MIN_COIN OCR-noise floor", () => {
    const snapshots = [
      snapshot(9, MIN_COIN), // exactly at floor: excluded
      snapshot(10, MIN_COIN * 2),
      snapshot(12, MIN_COIN * 4),
    ];
    const result = computeSkipCoinAnalysis(snapshots, [skip(10, 3)], -1, 2);
    expect(result).not.toBeNull();
    const lagMinus1 = result!.lagStats.find((s) => s.lag === -1);
    // wave 9 sits at exactly MIN_COIN, so lag -1 (wave 9) must have no data point.
    expect(lagMinus1?.n).toBe(0);
  });

  it("computes a lag-2 post/pre ratio bucketed by skip size", () => {
    const snapshots = [snapshot(10, MIN_COIN * 2), snapshot(12, MIN_COIN * 4)];
    const result = computeSkipCoinAnalysis(snapshots, [skip(10, 5)], -1, 2);
    expect(result?.bySkipSizeAtLag2).toEqual([
      { skippedCount: 5, medianPctChange: 100, n: 1 },
    ]);
  });

  it("drops wave-pair ratios outside the [1/3, 3] outlier band from lag-2 stats", () => {
    const snapshots = [snapshot(10, MIN_COIN * 2), snapshot(12, MIN_COIN * 10)];
    const result = computeSkipCoinAnalysis(snapshots, [skip(10, 5)], -1, 2);
    expect(result?.bySkipSizeAtLag2).toEqual([]);
  });

  it("finds the lag with the strongest-magnitude Pearson correlation", () => {
    // At lag 1, skipped_count and coin/min move in perfect lockstep -> r == 1.
    const snapshots = [
      snapshot(11, MIN_COIN * 2),
      snapshot(21, MIN_COIN * 4),
      snapshot(31, MIN_COIN * 6),
    ];
    const waveSkips = [skip(10, 1), skip(20, 2), skip(30, 3)];
    const result = computeSkipCoinAnalysis(snapshots, waveSkips, 1, 1);
    expect(result?.strongestLag).toBe(1);
    expect(result?.strongestAbsR).toBeCloseTo(1, 5);
  });
});

describe("formatLagLabel", () => {
  it("labels zero, positive, and negative lags", () => {
    expect(formatLagLabel(0)).toBe("0");
    expect(formatLagLabel(3)).toBe("+3");
    expect(formatLagLabel(-2)).toBe("-2");
  });
});

describe("formatPct", () => {
  it("signs positive changes and rounds to one decimal", () => {
    expect(formatPct(12.34)).toBe("+12.3%");
    expect(formatPct(-5)).toBe("-5.0%");
    expect(formatPct(0)).toBe("0.0%");
  });
});
