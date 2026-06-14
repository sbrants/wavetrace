import { useState, type RefObject } from "react";
import {
  captureChartCard,
  chartScreenshotFilename,
  copyChartScreenshot,
  downloadChartScreenshot,
} from "../chartScreenshot";

export default function ChartScreenshotActions({
  targetRef,
  disabled = false,
}: {
  targetRef: RefObject<HTMLDivElement | null>;
  disabled?: boolean;
}) {
  const [busy, setBusy] = useState(false);
  const [status, setStatus] = useState<string | null>(null);

  const flash = (message: string) => {
    setStatus(message);
    window.setTimeout(() => setStatus(null), 1500);
  };

  const capture = async () => {
    if (!targetRef.current) {
      throw new Error("No chart to capture");
    }
    return captureChartCard(targetRef.current);
  };

  const filename = () => {
    const title =
      targetRef.current?.querySelector("h3")?.textContent ?? "chart";
    return chartScreenshotFilename(title);
  };

  const onCopy = async () => {
    setBusy(true);
    try {
      const blob = await capture();
      await copyChartScreenshot(blob);
      flash("Copied ✓");
    } catch (e) {
      alert(String(e));
    } finally {
      setBusy(false);
    }
  };

  const onDownload = async () => {
    setBusy(true);
    try {
      const blob = await capture();
      downloadChartScreenshot(blob, filename());
      flash("Downloaded ✓");
    } catch (e) {
      alert(String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="chart-screenshot-actions">
      <button
        type="button"
        className="icon-btn"
        disabled={disabled || busy}
        onClick={onCopy}
        aria-label="Copy chart screenshot"
        title="Copy screenshot"
      >
        <CopyIcon />
      </button>
      <button
        type="button"
        className="icon-btn"
        disabled={disabled || busy}
        onClick={onDownload}
        aria-label="Download chart screenshot"
        title="Download screenshot"
      >
        <DownloadIcon />
      </button>
      {status && <span className="chart-action-status">{status}</span>}
    </div>
  );
}

function CopyIcon() {
  return (
    <svg viewBox="0 0 24 24" width="16" height="16" aria-hidden="true">
      <rect
        x="9"
        y="9"
        width="13"
        height="13"
        rx="2"
        fill="none"
        stroke="currentColor"
        strokeWidth="2"
      />
      <path
        d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"
        fill="none"
        stroke="currentColor"
        strokeWidth="2"
      />
    </svg>
  );
}

function DownloadIcon() {
  return (
    <svg viewBox="0 0 24 24" width="16" height="16" aria-hidden="true">
      <path
        d="M12 3v12"
        fill="none"
        stroke="currentColor"
        strokeWidth="2"
        strokeLinecap="round"
      />
      <path
        d="M7 10l5 5 5-5"
        fill="none"
        stroke="currentColor"
        strokeWidth="2"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
      <path
        d="M5 21h14"
        fill="none"
        stroke="currentColor"
        strokeWidth="2"
        strokeLinecap="round"
      />
    </svg>
  );
}
