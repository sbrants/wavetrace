import { useCallback, useEffect, useRef, useState } from "react";
import { api, ScannerLogView } from "../api";

const LINE_OPTIONS = [50, 100, 200, 500] as const;

export default function ScannerLogViewer() {
  const [log, setLog] = useState<ScannerLogView | null>(null);
  const [maxLines, setMaxLines] = useState(100);
  const [filter, setFilter] = useState("");
  const [autoRefresh, setAutoRefresh] = useState(false);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [scannerOn, setScannerOn] = useState(false);
  const preRef = useRef<HTMLPreElement>(null);
  const stickToBottom = useRef(true);

  const refresh = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      setLog(await api.readScannerLog(maxLines));
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }, [maxLines]);

  useEffect(() => {
    refresh();
    api.scannerRunning().then(setScannerOn).catch(() => {});
  }, [refresh]);

  useEffect(() => {
    if (!autoRefresh) return;
    const id = window.setInterval(async () => {
      try {
        setLog(await api.readScannerLog(maxLines));
        setScannerOn(await api.scannerRunning());
      } catch {
        /* keep last view */
      }
    }, 2000);
    return () => window.clearInterval(id);
  }, [autoRefresh, maxLines]);

  useEffect(() => {
    if (!stickToBottom.current || !preRef.current) return;
    preRef.current.scrollTop = preRef.current.scrollHeight;
  }, [log, filter]);

  const filtered =
    log?.lines.filter((line) =>
      filter ? line.toLowerCase().includes(filter.toLowerCase()) : true
    ) ?? [];

  const copyVisible = async () => {
    if (filtered.length === 0) return;
    await navigator.clipboard.writeText(filtered.join("\n"));
  };

  return (
    <section>
      <h3>Scanner log</h3>
      <p className="muted">
        Poll timings and OCR output from the background scanner. Log file:{" "}
        <code>{log?.path ?? "…"}</code>
      </p>
      <div className="row scanner-log-toolbar">
        <button type="button" onClick={refresh} disabled={loading}>
          {loading ? "Loading…" : "Refresh"}
        </button>
        <label className="scanner-log-auto">
          <input
            type="checkbox"
            checked={autoRefresh}
            onChange={(e) => setAutoRefresh(e.target.checked)}
          />
          Auto-refresh (2s)
        </label>
        <label>
          Lines
          <select
            value={maxLines}
            onChange={(e) => setMaxLines(Number(e.target.value))}
          >
            {LINE_OPTIONS.map((n) => (
              <option key={n} value={n}>
                {n}
              </option>
            ))}
          </select>
        </label>
        <input
          type="search"
          className="scanner-log-filter"
          placeholder="Filter lines…"
          value={filter}
          onChange={(e) => setFilter(e.target.value)}
        />
        <button
          type="button"
          disabled={filtered.length === 0}
          onClick={() => copyVisible().catch((e) => alert(e))}
        >
          Copy visible
        </button>
      </div>
      <p className="muted scanner-log-meta">
        {log
          ? log.total_lines === 0
            ? "No log entries yet — start the scanner to generate output."
            : `Showing ${filtered.length} of ${log.lines.length} loaded lines` +
              (log.truncated ? ` (${log.total_lines} total in file)` : "") +
              (log.log_tail_truncated ? " · tail capped at 2 MiB" : "") +
              (scannerOn ? " · scanner running" : "")
          : ""}
      </p>
      {error && <p className="error">{error}</p>}
      <pre
        ref={preRef}
        className="scanner-log-view"
        onScroll={() => {
          if (!preRef.current) return;
          const el = preRef.current;
          stickToBottom.current =
            el.scrollHeight - el.scrollTop - el.clientHeight < 48;
        }}
      >
        {filtered.length > 0
          ? filtered.join("\n")
          : log?.total_lines === 0
            ? ""
            : filter
              ? "No lines match the filter."
              : ""}
      </pre>
    </section>
  );
}
