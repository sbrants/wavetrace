import { describe, expect, it } from "vitest";
import type { RunRowsCursor, RunRowsPage, SnapshotRow } from "./api";
import { mergeRunRows, RunRowsReader } from "./runRows";

function snapshot(wave: number): SnapshotRow {
  return {
    id: `s${wave}`,
    wave,
    tier: null,
    coin_per_minute: null,
    golden_combo_chance: null,
    golden_combo_caret: null,
    golden_combo_multiplier: null,
    recorded_at: "2026-01-01T00:00:00Z",
  };
}

/** Stands in for the `run_rows` command, with each snapshot's wave as its rowid. Calls wait
 *  until released, so a test can decide the order they finish in. */
function fakeDb(initialWaves: number[]) {
  const waves = [...initialWaves];
  const pending: (() => void)[] = [];
  const calls: RunRowsCursor[] = [];
  const fetchRows = (_runId: string, after: RunRowsCursor) => {
    calls.push(after);
    // Reads the table when the call is made, as the backend would.
    const rows = waves.filter((w) => w > after.snapshot_rowid);
    const page: RunRowsPage = {
      snapshots: rows.map(snapshot),
      wave_skips: [],
      cursor: {
        snapshot_rowid: Math.max(after.snapshot_rowid, ...rows),
        wave_skip_rowid: 0,
      },
    };
    return new Promise<RunRowsPage>((resolve) => {
      pending.push(() => resolve(page));
    });
  };
  return {
    fetchRows,
    calls,
    add: (wave: number) => waves.push(wave),
    remove: (wave: number) => waves.splice(waves.indexOf(wave), 1),
    release: async (index = 0) => {
      pending.splice(index, 1)[0]();
      await new Promise((r) => setTimeout(r));
    },
  };
}

function loaded(reader: RunRowsReader) {
  return reader.rows.snapshots.map((s) => s.wave);
}

describe("mergeRunRows", () => {
  const wave = (r: { wave: number }) => r.wave;

  it("appends rows that come after the existing ones", () => {
    expect(mergeRunRows([{ wave: 1 }, { wave: 2 }], [{ wave: 3 }], wave)).toEqual([
      { wave: 1 },
      { wave: 2 },
      { wave: 3 },
    ]);
  });

  it("slots in a row at a lower wave", () => {
    expect(mergeRunRows([{ wave: 1 }, { wave: 5 }], [{ wave: 3 }], wave)).toEqual([
      { wave: 1 },
      { wave: 3 },
      { wave: 5 },
    ]);
  });

  it("keeps arrival order among equal waves", () => {
    const merged = mergeRunRows(
      [{ wave: 1, id: "old" }],
      [{ wave: 1, id: "new" }],
      wave
    );
    expect(merged.map((r) => r.id)).toEqual(["old", "new"]);
  });
});

describe("RunRowsReader", () => {
  it("only asks for rows past what it already has", async () => {
    const db = fakeDb([1, 2]);
    const reader = new RunRowsReader("run", db.fetchRows);
    const load = reader.load();
    await db.release();
    await load;

    db.add(3);
    const read = reader.readNew();
    await db.release();

    expect((await read)?.snapshots.map((s) => s.wave)).toEqual([1, 2, 3]);
    expect(db.calls[1].snapshot_rowid).toBe(2);
  });

  it("reports no change, and keeps the same arrays, when nothing was added", async () => {
    const db = fakeDb([1]);
    const reader = new RunRowsReader("run", db.fetchRows);
    const load = reader.load();
    await db.release();
    await load;
    const before = reader.rows;

    const read = reader.readNew();
    await db.release();

    expect(await read).toBeNull();
    expect(reader.rows).toBe(before);
  });

  it("does a full load if none has landed, including after one failed", async () => {
    let fail = true;
    const db = fakeDb([1]);
    const reader = new RunRowsReader("run", (id, after) => {
      if (fail) {
        fail = false;
        return Promise.reject(new Error("db busy"));
      }
      return db.fetchRows(id, after);
    });
    await expect(reader.load()).rejects.toThrow("db busy");

    const read = reader.readNew();
    await db.release();

    expect((await read)?.snapshots.map((s) => s.wave)).toEqual([1]);
  });

  it("doesn't start a read while a full load is running", async () => {
    const db = fakeDb([1]);
    const reader = new RunRowsReader("run", db.fetchRows);
    const load = reader.load();

    expect(await reader.readNew()).toBeNull();
    expect(db.calls).toHaveLength(1);

    await db.release();
    await load;
  });

  it("doesn't add a row twice when reads overlap", async () => {
    const db = fakeDb([1]);
    const reader = new RunRowsReader("run", db.fetchRows);
    const load = reader.load();
    await db.release();
    await load;

    db.add(2);
    const first = reader.readNew();
    expect(await reader.readNew()).toBeNull();
    await db.release();
    await first;

    expect(loaded(reader)).toEqual([1, 2]);
  });

  it("doesn't bring a deleted row back when an older read finishes late", async () => {
    const db = fakeDb([1]);
    const reader = new RunRowsReader("run", db.fetchRows);
    const load = reader.load();
    await db.release();
    await load;

    db.add(2);
    const staleRead = reader.readNew(); // Sees wave 2...
    db.remove(2); // ...which is then deleted, so the edit triggers a full load.
    const reload = reader.load();

    await db.release(1);
    await reload;
    await db.release(0);

    expect(await staleRead).toBeNull();
    expect(loaded(reader)).toEqual([1]);
  });

  it("keeps only the newest of two overlapping full loads", async () => {
    const db = fakeDb([1]);
    const reader = new RunRowsReader("run", db.fetchRows);
    const stale = reader.load();
    db.add(2);
    const latest = reader.load();

    await db.release(1);
    await latest;
    await db.release(0);

    expect(await stale).toBeNull();
    expect(loaded(reader)).toEqual([1, 2]);
  });
});
