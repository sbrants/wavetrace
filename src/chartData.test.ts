import { describe, expect, it } from "vitest";
import {
  averageGoldenComboCaret,
  buildLeadLagPolygons,
  buildWaveJumpMarkers,
  compareNewerRunIndex,
  downsampleChartData,
  smoothNullableSeries,
  snapshotsToChartData,
  type CoinChartPoint,
  type CompareChartRow,
} from "./chartData";
import type { SnapshotRow, WaveSkipRow } from "./api";

function snapshot(
  wave: number,
  coinPerMinute: number | null,
  gcCaret: number | null = null,
  recordedAt = "2026-01-01T00:00:00Z"
): SnapshotRow {
  return {
    id: `s-${wave}-${recordedAt}`,
    wave,
    tier: null,
    coin_per_minute: coinPerMinute,
    golden_combo_chance: null,
    golden_combo_caret: gcCaret,
    golden_combo_multiplier: null,
    recorded_at: recordedAt,
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

describe("downsampleChartData", () => {
  const point = (wave: number): CoinChartPoint => ({
    wave,
    coin: wave,
    golden_combo_caret: null,
  });

  it("returns the input unchanged when under the cap", () => {
    const points = [point(1), point(2), point(3)];
    expect(downsampleChartData(points, 10)).toEqual(points);
  });

  it("always keeps the first and last point when downsampling", () => {
    const points = Array.from({ length: 100 }, (_, i) => point(i));
    const sampled = downsampleChartData(points, 10);
    expect(sampled).toHaveLength(10);
    expect(sampled[0]).toEqual(point(0));
    expect(sampled[sampled.length - 1]).toEqual(point(99));
  });
});

describe("snapshotsToChartData", () => {
  it("drops snapshots with neither coin/min nor a Golden Combo caret", () => {
    const snapshots = [snapshot(1, null, null), snapshot(2, 100, null), snapshot(3, null, 5)];
    const points = snapshotsToChartData(snapshots);
    expect(points.map((p) => p.wave)).toEqual([2, 3]);
  });

  it("skips re-downsampling when alreadySampled is set", () => {
    const snapshots = Array.from({ length: 6000 }, (_, i) => snapshot(i, i));
    const points = snapshotsToChartData(snapshots, { alreadySampled: true });
    expect(points).toHaveLength(6000);
  });
});

describe("averageGoldenComboCaret", () => {
  it("returns null when no snapshot has a finite caret", () => {
    expect(averageGoldenComboCaret([snapshot(1, 100, null)])).toBeNull();
  });

  it("averages only the finite caret values", () => {
    const snapshots = [snapshot(1, null, 2), snapshot(2, null, 4), snapshot(3, null, null)];
    expect(averageGoldenComboCaret(snapshots)).toBe(3);
  });
});

describe("buildWaveJumpMarkers", () => {
  it("marks a recorded wave skip using its skip row", () => {
    const snapshots = [snapshot(1, 100), snapshot(5, 100)];
    const markers = buildWaveJumpMarkers(snapshots, [skip(5, 4)]);
    expect(markers).toEqual([{ id: "skip-5", wave: 5, skip_count: 4, skip_tooltip: "4" }]);
  });

  it("treats an unrecorded +1 wave advance as a wave-jump marker", () => {
    const snapshots = [snapshot(1, 100), snapshot(2, 100)];
    const markers = buildWaveJumpMarkers(snapshots);
    expect(markers).toEqual([
      { id: "wave-jump-2", wave: 2, skip_count: 1, skip_tooltip: "1" },
    ]);
  });

  it("ignores an unrecorded jump larger than 1 (scanner downtime, not a real skip)", () => {
    const snapshots = [snapshot(1, 100), snapshot(9, 100)];
    expect(buildWaveJumpMarkers(snapshots)).toEqual([]);
  });

  it("still emits skip rows whose wave never appears in the snapshot history", () => {
    const markers = buildWaveJumpMarkers([snapshot(1, 100)], [skip(50, 10)]);
    expect(markers).toEqual([{ id: "skip-50", wave: 50, skip_count: 10, skip_tooltip: "10" }]);
  });
});

describe("smoothNullableSeries", () => {
  it("returns the series unchanged for a window of 1 or less", () => {
    const values = [1, null, 3];
    expect(smoothNullableSeries(values, 1)).toBe(values);
  });

  it("averages neighbors while skipping nulls and out-of-range indices", () => {
    // window=3 -> +-1 neighbor. Middle index sees [1, null, 3] -> skips the null.
    expect(smoothNullableSeries([1, null, 3], 3)).toEqual([1, 2, 3]);
  });

  it("returns null where every neighbor in the window is null", () => {
    expect(smoothNullableSeries([null, null, null], 3)).toEqual([null, null, null]);
  });
});

describe("compareNewerRunIndex", () => {
  it("picks the run with the later started_at as index 0", () => {
    const runs = [{ started_at: "2026-01-02T00:00:00Z" }, { started_at: "2026-01-01T00:00:00Z" }];
    expect(compareNewerRunIndex(runs)).toBe(0);
  });

  it("returns null unless exactly two runs are given", () => {
    expect(compareNewerRunIndex([{ started_at: "2026-01-01T00:00:00Z" }])).toBeNull();
  });

  it("returns null when a date fails to parse", () => {
    const runs = [{ started_at: "not-a-date" }, { started_at: "2026-01-01T00:00:00Z" }];
    expect(compareNewerRunIndex(runs)).toBeNull();
  });
});

describe("buildLeadLagPolygons", () => {
  function row(x: number, newer: number, older: number): CompareChartRow {
    return { x, coin_0: newer, coin_1: older };
  }

  it("builds a single polygon when the newer series stays ahead throughout", () => {
    const rows = [row(0, 10, 5), row(1, 20, 8)];
    const polygons = buildLeadLagPolygons(rows, 0, 1);
    expect(polygons).toHaveLength(1);
    expect(polygons[0].tone).toBe("ahead");
  });

  it("splits into two polygons at the point the lead crosses over", () => {
    const rows = [row(0, 10, 0), row(1, 0, 10)];
    const polygons = buildLeadLagPolygons(rows, 0, 1);
    expect(polygons.map((p) => p.tone)).toEqual(["ahead", "behind"]);
    // Crossing happens at the midpoint, where newer == older.
    expect(polygons[0].ring[1].x).toBeCloseTo(0.5, 5);
  });
});
