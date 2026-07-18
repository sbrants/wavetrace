export const RUN_TYPE_FILTER_OPTIONS = [
  { value: "farming", label: "Farming" },
  { value: "tournament", label: "Tournament" },
  { value: "dissonance_attack", label: "Dissonance (Attack)" },
  { value: "dissonance_defense", label: "Dissonance (Defense)" },
  { value: "dissonance_utility", label: "Dissonance (Utility)" },
  {
    value: "dissonance_ultimate_weapons",
    label: "Dissonance (Ultimate Weapons)",
  },
] as const;

const RUN_TYPE_LABELS: Record<string, string> = Object.fromEntries(
  RUN_TYPE_FILTER_OPTIONS.map((option) => [option.value, option.label]),
);

export function formatRunType(runType: string | null | undefined): string {
  if (!runType) return "farming";
  return RUN_TYPE_LABELS[runType] ?? runType.replaceAll("_", " ");
}

export function runTypeUsesBadge(runType: string | null | undefined): boolean {
  return !!runType && runType !== "farming";
}
