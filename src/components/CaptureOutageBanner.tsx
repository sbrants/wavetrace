import { useEffect, useState } from "react";

/** Wait this long before warning, so a brief hiccup doesn't flash a banner. */
const GRACE_MS = 5000;

const OUTAGE_STATUSES = new Set([
  "window_not_found",
  "window_minimized",
  "capture_stalled",
]);

function describe(status: string | undefined): string {
  switch (status) {
    case "window_minimized":
      return "The game window is minimized.";
    case "capture_stalled":
      return "Screen capture stopped responding and is being retried.";
    default:
      return "The game window can't be found — it may be minimized, hidden, on another virtual desktop, or the screen may be locked.";
  }
}

function formatDuration(ms: number): string {
  const total = Math.floor(ms / 1000);
  if (total < 60) return `${total}s`;
  const minutes = Math.floor(total / 60);
  if (minutes < 60) return `${minutes}m ${total % 60}s`;
  return `${Math.floor(minutes / 60)}h ${minutes % 60}m`;
}

/**
 * Warns while the scanner is running but can't capture the game window. Without this
 * the Dashboard keeps showing the last values it read, which looks like live tracking
 * when in fact nothing has been recorded since the window disappeared.
 */
export default function CaptureOutageBanner({
  running,
  status,
}: {
  running: boolean;
  status: string | undefined;
}) {
  const outage = running && status !== undefined && OUTAGE_STATUSES.has(status);
  const [since, setSince] = useState<number | null>(null);
  const [now, setNow] = useState(() => Date.now());

  useEffect(() => {
    setSince((prev) => {
      if (!outage) return null;
      return prev ?? Date.now();
    });
  }, [outage]);

  useEffect(() => {
    if (since === null) return;
    setNow(Date.now());
    const id = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(id);
  }, [since]);

  if (since === null) return null;
  const elapsed = now - since;
  if (elapsed < GRACE_MS) return null;

  return (
    <div className="capture-outage-banner" role="status" aria-live="polite">
      <strong>Not capturing the game window</strong>
      <span>
        {describe(status)} Nothing has been recorded for{" "}
        {formatDuration(elapsed)}, and the values below are from before then.
      </span>
    </div>
  );
}
