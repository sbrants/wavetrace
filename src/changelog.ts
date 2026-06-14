import changelogMd from "../CHANGELOG.md?raw";

export const CHANGELOG_TEXT = changelogMd;

/** Drop the title block and empty Unreleased section for in-app display. */
export function changelogBody(): string {
  const lines = changelogMd.split("\n");
  const start = lines.findIndex((line) => line.startsWith("## ["));
  if (start < 0) return changelogMd.trim();
  return lines.slice(start).join("\n").trim();
}
