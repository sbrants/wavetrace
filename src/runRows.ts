import {
  api,
  START_OF_RUN_ROWS,
  type RunRowsCursor,
  type RunRowsPage,
  type SnapshotRow,
  type WaveSkipRow,
} from "./api";

export type RunRows = Pick<RunRowsPage, "snapshots" | "wave_skips">;

export const EMPTY_RUN_ROWS: RunRows = { snapshots: [], wave_skips: [] };

/** Rows of a run were edited or deleted: anything else showing that run must reload it. */
export const RUN_ROWS_EDITED_EVENT = "wavetrace-run-rows-edited";
/** A backup replaced the database, renumbering rows: every reader must reload. */
export const DATA_RESTORED_EVENT = "wavetrace-data-restored";

/** `added` merged into `rows`, keeping the order of `key`. Equal keys stay in arrival
 *  order, and a stable sort of almost-sorted rows is cheap. */
export function mergeRunRows<T>(rows: T[], added: T[], key: (row: T) => number): T[] {
  return [...rows, ...added].sort((a, b) => key(a) - key(b));
}

const snapshotWave = (s: SnapshotRow) => s.wave;
const skipWave = (s: WaveSkipRow) => s.at_wave;

/**
 * One run's rows, kept current by fetching only what's been added since the last read. The
 * scanner only appends, so that's all a live view needs between full loads; fetching a long
 * run's every row on each refresh made every refresh a stall.
 *
 * Both methods resolve to the rows to show, or null when there's nothing new to show:
 * unchanged, or the result was superseded by a full load that started after it.
 */
export class RunRowsReader {
  rows: RunRows = EMPTY_RUN_ROWS;
  /** Null until a full load lands. */
  private cursor: RunRowsCursor | null = null;
  /** Bumped by each full load, so an older fetch can't undo it: rows read before a delete
   *  must not bring the deleted row back. */
  private generation = 0;
  private loading = false;
  private reading = false;

  constructor(
    readonly runId: string,
    private readonly fetchRows: (
      runId: string,
      after: RunRowsCursor
    ) => Promise<RunRowsPage> = api.runRows
  ) {}

  /** Read the whole run, replacing what's held. */
  async load(): Promise<RunRows | null> {
    const generation = ++this.generation;
    this.cursor = null;
    this.loading = true;
    try {
      const page = await this.fetchRows(this.runId, START_OF_RUN_ROWS);
      if (generation !== this.generation) return null;
      this.cursor = page.cursor;
      this.rows = { snapshots: page.snapshots, wave_skips: page.wave_skips };
      return this.rows;
    } finally {
      if (generation === this.generation) this.loading = false;
    }
  }

  /** Read rows added since the last read. Falls back to a full load if none has landed. */
  async readNew(): Promise<RunRows | null> {
    if (this.loading || this.reading) return null;
    if (!this.cursor) return this.load();
    const generation = this.generation;
    this.reading = true;
    try {
      const page = await this.fetchRows(this.runId, this.cursor);
      if (generation !== this.generation) return null;
      this.cursor = page.cursor;
      if (page.snapshots.length === 0 && page.wave_skips.length === 0) return null;
      this.rows = {
        snapshots:
          page.snapshots.length > 0
            ? mergeRunRows(this.rows.snapshots, page.snapshots, snapshotWave)
            : this.rows.snapshots,
        wave_skips:
          page.wave_skips.length > 0
            ? mergeRunRows(this.rows.wave_skips, page.wave_skips, skipWave)
            : this.rows.wave_skips,
      };
      return this.rows;
    } finally {
      this.reading = false;
    }
  }
}
