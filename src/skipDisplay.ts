export type SkipDisplay =
  | { kind: "multiplier"; value: number }
  | { kind: "delta"; value: number };

/**
 * Wave jump size is always the observed wave increment (`skipped_count`).
 * Banner `skip_multiplier` only marks the event for a live `×` prefix — it must
 * not replace the jump value (OCR ×N can differ from the real delta by ±1).
 */
export function skipDisplayFromRow(row: {
  skipped_count: number;
  skip_multiplier?: number | null;
}): SkipDisplay {
  if (row.skip_multiplier != null) {
    return { kind: "multiplier", value: row.skipped_count };
  }
  return { kind: "delta", value: row.skipped_count };
}

/** Table / chart values — plain number; context comes from "Wave jump" labels. */
export function formatSkipDisplay(display: SkipDisplay): string {
  return String(display.value);
}

/** Dashboard live stat — × prefix only when banner multiplier is known. */
export function formatSkipLiveStat(display: SkipDisplay): string {
  return display.kind === "multiplier"
    ? `×${display.value}`
    : String(display.value);
}

export function skipChartValue(display: SkipDisplay): number {
  return display.value;
}
