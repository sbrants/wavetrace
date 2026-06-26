import json
import sqlite3
import statistics
from math import sqrt
from pathlib import Path

# Skip vs coin/min analysis (same rules as src/skipCoinAnalysis.ts; see Goal.md).
# MIN_COIN: ignore near-zero OCR. MAX_RATIO: drop ratios outside [1/3, 3] — OCR spikes, not game economics.

db_dir = Path.home() / "AppData/Roaming/wavetrace"
db = next(
    (db_dir / name for name in ("wavetrace.db", "wavewatch.db", "towerrun.db") if (db_dir / name).exists()),
    db_dir / "wavetrace.db",
)
conn = sqlite3.connect(db)
conn.row_factory = sqlite3.Row

MIN_COIN = 1e11  # ignore near-zero OCR
MAX_RATIO = 3.0

runs = conn.execute(
    """
SELECT r.id, r.started_at, r.run_type,
       (SELECT COUNT(*) FROM snapshots s
        WHERE s.run_id=r.id AND s.coin_per_minute > ?) AS snap_n,
       (SELECT COUNT(*) FROM wave_skips w WHERE w.run_id=r.id) AS skip_n
FROM runs r
WHERE (SELECT COUNT(*) FROM snapshots s
       WHERE s.run_id=r.id AND s.coin_per_minute > ?) > 50
  AND (SELECT COUNT(*) FROM wave_skips w WHERE w.run_id=r.id) > 5
ORDER BY r.started_at DESC
""",
    (MIN_COIN, MIN_COIN),
).fetchall()

lags = list(range(-5, 11))
results = []

for run in runs:
    rid = run["id"]
    snaps = {
        row["wave"]: row["coin_per_minute"]
        for row in conn.execute(
            """
            SELECT wave, coin_per_minute FROM snapshots
            WHERE run_id=? AND coin_per_minute > ?
            ORDER BY wave
            """,
            (rid, MIN_COIN),
        )
    }
    skip_waves = {
        row["at_wave"]
        for row in conn.execute(
            "SELECT at_wave FROM wave_skips WHERE run_id=?", (rid,)
        )
    }
    skips = list(
        conn.execute(
            "SELECT at_wave, skipped_count FROM wave_skips WHERE run_id=? ORDER BY at_wave",
            (rid,),
        )
    )
    if not snaps or not skips:
        continue

    # Control points: snapshots not within 3 waves of any skip
    controls = []
    for wave, coin in snaps.items():
        if any(abs(wave - sw) <= 3 for sw in skip_waves):
            continue
        controls.append((wave, coin))

    for sk in skips:
        at = sk["at_wave"]
        row = {"skipped_count": sk["skipped_count"], "at_wave": at}
        for lag in lags:
            row[f"lag_{lag}"] = snaps.get(at + lag)
        results.append(row)

    for wave, coin in controls[: len(skips) * 2]:
        row = {"skipped_count": 0, "at_wave": wave, "control": True}
        for lag in lags:
            row[f"lag_{lag}"] = snaps.get(wave + lag)
        results.append(row)


def pearson(xs, ys):
    n = len(xs)
    if n < 10:
        return None
    mx, my = sum(xs) / n, sum(ys) / n
    num = sum((x - mx) * (y - my) for x, y in zip(xs, ys))
    den = sqrt(sum((x - mx) ** 2 for x in xs) * sum((y - my) ** 2 for y in ys))
    return num / den if den else None


def safe_ratio(num, den):
    if not num or not den or den < MIN_COIN or num < MIN_COIN:
        return None
    r = num / den
    if r > MAX_RATIO or r < 1 / MAX_RATIO:
        return None
    return r


def pct_change_lag(events, lag):
    vals = []
    for r in events:
        pre = r.get("lag_-1") or r.get("lag_0")
        post = r.get(f"lag_{lag}")
        ratio = safe_ratio(post, pre)
        if ratio is not None:
            vals.append((ratio - 1) * 100)
    if len(vals) < 10:
        return None
    return {
        "median_pct": statistics.median(vals),
        "mean_pct": statistics.mean(vals),
        "n": len(vals),
    }


skips_only = [r for r in results if not r.get("control")]
controls_only = [r for r in results if r.get("control")]

corr = {}
for lag in lags:
    xs, ys = [], []
    for r in skips_only:
        v = r.get(f"lag_{lag}")
        if v and v > MIN_COIN:
            xs.append(r["skipped_count"])
            ys.append(v / 1e12)
    corr[lag] = {"r": pearson(xs, ys), "n": len(xs)}

# Coin trajectory around skip vs control (median coin T by lag)
def median_by_lag(events):
    out = {}
    for lag in lags:
        vals = [
            r[f"lag_{lag}"] / 1e12
            for r in events
            if r.get(f"lag_{lag}") is not None and r[f"lag_{lag}"] > MIN_COIN
        ]
        if len(vals) >= 10:
            out[lag] = {"median_t": statistics.median(vals), "n": len(vals)}
    return out

skip_traj = median_by_lag(skips_only)
ctrl_traj = median_by_lag(controls_only)

post_skip = {k: pct_change_lag(skips_only, k) for k in range(1, 6)}
post_ctrl = {k: pct_change_lag(controls_only, k) for k in range(1, 6)}

# Skip size buckets vs coin change +2 waves
buckets = {}
for r in skips_only:
    sc = r["skipped_count"]
    if sc > 10:
        continue
    pre = r.get("lag_-1") or r.get("lag_0")
    post = r.get("lag_2")
    ratio = safe_ratio(post, pre)
    if ratio is None:
        continue
    buckets.setdefault(sc, []).append((ratio - 1) * 100)

by_size = {
    sc: {"median_pct": statistics.median(vs), "n": len(vs)}
    for sc, vs in sorted(buckets.items())
    if len(vs) >= 20
}

best_corr_lag = max(
    ((lag, abs(v["r"])) for lag, v in corr.items() if v["r"] is not None),
    key=lambda x: x[1],
    default=(None, 0),
)

out = {
    "run_count": len(runs),
    "skip_events": len(skips_only),
    "control_points": len(controls_only),
    "min_coin_filter_t": MIN_COIN / 1e12,
    "corr_skipped_count_vs_coin_t": {str(k): v for k, v in corr.items()},
    "strongest_corr": {"lag": best_corr_lag[0], "abs_r": best_corr_lag[1]},
    "median_coin_trajectory_skip": {str(k): v for k, v in skip_traj.items()},
    "median_coin_trajectory_control": {str(k): v for k, v in ctrl_traj.items()},
    "pct_change_after_skip_vs_control": {
        str(k): {"skip": post_skip[k], "control": post_ctrl[k]}
        for k in range(1, 6)
        if post_skip.get(k) or post_ctrl.get(k)
    },
    "pct_change_2_waves_by_skip_size": by_size,
}
print(json.dumps(out, indent=2))
