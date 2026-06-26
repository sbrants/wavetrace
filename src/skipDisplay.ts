export type SkipDisplay =
  | { kind: "multiplier"; value: number }
  | { kind: "delta"; value: number };

/** Prefer banner ×N when stored; otherwise wave jump count. */
export function skipDisplayFromRow(row: {
  skipped_count: number;
  skip_multiplier?: number | null;
}): SkipDisplay {
  if (row.skip_multiplier != null) {
    return { kind: "multiplier", value: row.skip_multiplier };
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

export function formatSkipTooltipFromRow(row: {
  skipped_count: number;
  skip_multiplier?: number | null;
}): string {
  return formatSkipDisplay(skipDisplayFromRow(row));
}
