import { useMemo } from "react";
import { SnapshotRow, WaveSkipRow } from "../api";
import {
  computeSkipCoinAnalysis,
  formatLagLabel,
  formatPct,
} from "../skipCoinAnalysis";

type Props = {
  snapshots: SnapshotRow[];
  waveSkips: WaveSkipRow[];
};

export default function SkipCoinAnalytics({ snapshots, waveSkips }: Props) {
  const analysis = useMemo(
    () => computeSkipCoinAnalysis(snapshots, waveSkips),
    [snapshots, waveSkips]
  );

  if (!analysis) return null;

  const corrBars = analysis.lagStats.filter(
    (s) => s.lag >= -3 && s.lag <= 8 && s.n >= 3
  );
  const maxAbsR = Math.max(0.12, ...corrBars.map((s) => Math.abs(s.pearsonR ?? 0)));

  const lag2Median =
    analysis.postLagMedians.find((p) => p.lag === 2)?.medianPct ?? null;

  return (
    <div className="skip-analytics-panel">
      <div className="skip-analytics-header">
        <h3>Skip vs coin/min (this run)</h3>
        <span className="muted">
          Coin/min {'>'} 0.1T only · % change vs wave before skip · ratios capped
          at 3×
        </span>
      </div>

      <div className="skip-analytics-stats">
        <div className="skip-analytics-stat">
          <span className="skip-analytics-stat-label">Skips</span>
          <span className="skip-analytics-stat-value">{analysis.skipCount}</span>
        </div>
        <div className="skip-analytics-stat">
          <span className="skip-analytics-stat-label">Strongest |r|</span>
          <span className="skip-analytics-stat-value">
            {analysis.strongestAbsR.toFixed(2)}
            <span className="muted skip-analytics-stat-hint">
              {" "}
              lag {formatLagLabel(analysis.strongestLag)}
            </span>
          </span>
        </div>
        <div className="skip-analytics-stat">
          <span className="skip-analytics-stat-label">Median Δ +2 waves</span>
          <span className="skip-analytics-stat-value">
            {lag2Median != null ? formatPct(lag2Median) : "—"}
          </span>
        </div>
      </div>

      {analysis.analyzedSkips < 3 ? (
        <p className="muted skip-analytics-note">
          Not enough skip events with overlapping snapshot data to show lag
          charts yet ({analysis.analyzedSkips} with coin readings).
        </p>
      ) : (
        <>
          <div className="skip-analytics-section">
            <h4>Skip size vs coin/min by lag</h4>
            <p className="muted skip-analytics-caption">
              Pearson r between waves skipped and coin/min (T) at each lag relative
              to the skip wave. Values near 0 mean no linear link.
            </p>
            <div
              className="skip-corr-chart"
              role="img"
              aria-label="Correlation by lag chart"
            >
              {corrBars.map((row) => {
                const r = row.pearsonR ?? 0;
                const heightPct = (Math.abs(r) / maxAbsR) * 50;
                const positive = r >= 0;
                return (
                  <div key={row.lag} className="skip-corr-bar-col">
                    <div className="skip-corr-bar-track">
                      <div className="skip-corr-zero-line" />
                      <div
                        className={`skip-corr-bar-fill ${positive ? "positive" : "negative"}`}
                        style={{ height: `${heightPct}%` }}
                      />
                    </div>
                    <span className="skip-corr-bar-value">
                      {row.pearsonR != null ? row.pearsonR.toFixed(2) : "—"}
                    </span>
                    <span className="skip-corr-bar-label">
                      {formatLagLabel(row.lag)}
                    </span>
                  </div>
                );
              })}
            </div>
          </div>

          {analysis.postLagMedians.length > 0 && (
            <div className="skip-analytics-section">
              <h4>Median coin/min change after skip</h4>
              <table className="skip-analytics-table">
                <thead>
                  <tr>
                    <th>Lag (waves)</th>
                    <th>Median Δ vs wave before</th>
                    <th>n</th>
                  </tr>
                </thead>
                <tbody>
                  {analysis.postLagMedians.map((row) => (
                    <tr key={row.lag}>
                      <td>+{row.lag}</td>
                      <td>{formatPct(row.medianPct)}</td>
                      <td>{row.n}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}

          {analysis.bySkipSizeAtLag2.length > 0 && (
            <div className="skip-analytics-section">
              <h4>By skip size — median Δ at +2 waves</h4>
              <table className="skip-analytics-table">
                <thead>
                  <tr>
                    <th>Skipped</th>
                    <th>Median Δ coin/min</th>
                    <th>n</th>
                  </tr>
                </thead>
                <tbody>
                  {analysis.bySkipSizeAtLag2.map((row) => (
                    <tr key={row.skippedCount}>
                      <td>×{row.skippedCount}</td>
                      <td>{formatPct(row.medianPctChange)}</td>
                      <td>{row.n}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </>
      )}
    </div>
  );
}
