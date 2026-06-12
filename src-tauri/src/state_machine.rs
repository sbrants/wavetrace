//! Run state machine per Goal.md "Recording rules" and "Game mode edge cases".
//!
//! Pure logic: consumes classified poll results, emits actions for the
//! storage layer. No I/O here so everything is unit-testable.

use serde::Serialize;

use crate::parser::CoinReading;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GameMode {
    Normal,
    TotalCoin,
    IntroSprint,
    Tournament,
    EndOfRun,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RunType {
    Normal,
    Tournament,
}

impl RunType {
    pub fn as_str(&self) -> &'static str {
        match self {
            RunType::Normal => "normal",
            RunType::Tournament => "tournament",
        }
    }
}

/// One classified poll of the captured window.
#[derive(Debug, Clone, Copy)]
pub struct PollInput {
    pub mode: GameMode,
    pub tier: Option<u32>,
    pub wave: Option<u32>,
    pub coin: CoinReading,
}

/// Side effects the caller must apply.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    StartRun {
        run_type: RunType,
    },
    Snapshot {
        wave: u32,
        tier: Option<u32>,
        coin_per_minute: Option<f64>,
    },
    EndRun {
        final_wave: u32,
        peak_tier: Option<u32>,
    },
}

#[derive(Debug, Clone, Serialize)]
pub struct LiveState {
    pub mode: GameMode,
    pub tier: Option<u32>,
    pub wave: Option<u32>,
    pub coin_per_minute: Option<f64>,
    pub run_active: bool,
    pub run_type: Option<RunType>,
    /// True while game shows total coins instead of a rate (warn the user).
    pub total_coin_warning: bool,
}

struct ActiveRun {
    run_type: RunType,
    last_saved_wave: u32,
    peak_tier: Option<u32>,
}

/// Debounce: a value must be seen on `DEBOUNCE` consecutive polls to be
/// accepted (Goal.md "OCR stability").
const DEBOUNCE: u32 = 2;

#[derive(Default)]
struct Debounced {
    candidate: Option<u32>,
    count: u32,
    confirmed: Option<u32>,
}

impl Debounced {
    fn feed(&mut self, value: Option<u32>) -> Option<u32> {
        let Some(v) = value else { return self.confirmed };
        if self.candidate == Some(v) {
            self.count += 1;
        } else {
            self.candidate = Some(v);
            self.count = 1;
        }
        if self.count >= DEBOUNCE {
            self.confirmed = Some(v);
        }
        self.confirmed
    }

    /// Latest reading for the dashboard (confirmed if stable, else most recent poll).
    fn display(&self) -> Option<u32> {
        self.confirmed.or(self.candidate)
    }
}

/// Recent readings retained for the outlier-resistant median.
const COIN_MEDIAN_WINDOW: usize = 5;

/// Coin/min changes more slowly than wave; debounce and reject single-frame
/// spikes. Once a reading is accepted, the *median* of the recent window is
/// reported so a single parseable-but-wrong OCR value can't move the number.
#[derive(Default)]
struct DebouncedCoinRate {
    candidate: Option<f64>,
    count: u32,
    confirmed: Option<f64>,
    window: std::collections::VecDeque<f64>,
}

impl DebouncedCoinRate {
    fn feed(&mut self, value: Option<f64>) -> Option<f64> {
        let Some(v) = value else {
            return self.confirmed;
        };
        self.window.push_back(v);
        while self.window.len() > COIN_MEDIAN_WINDOW {
            self.window.pop_front();
        }
        let same = self
            .candidate
            .map(|c| approx_same_rate(c, v))
            .unwrap_or(false);
        if same {
            self.count += 1;
        } else {
            self.candidate = Some(v);
            self.count = 1;
        }
        let needed = if self.is_outlier(v) { 3 } else { DEBOUNCE };
        if self.count >= needed {
            self.confirmed = Some(self.median());
        }
        self.confirmed
    }

    /// Median of the recent window; rejects single-frame OCR outliers while
    /// still tracking the slow legitimate drift of the rate.
    fn median(&self) -> f64 {
        let mut vals: Vec<f64> = self.window.iter().copied().collect();
        vals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let n = vals.len();
        if n == 0 {
            return 0.0;
        }
        if n % 2 == 1 {
            vals[n / 2]
        } else {
            (vals[n / 2 - 1] + vals[n / 2]) / 2.0
        }
    }

    fn is_outlier(&self, v: f64) -> bool {
        let Some(cur) = self.confirmed else {
            return false;
        };
        if cur <= 0.0 {
            return false;
        }
        let ratio = v / cur;
        ratio > 50.0 || ratio < 0.02
    }

    /// Latest rate for the dashboard; holds the last parseable reading between polls.
    fn display(&self) -> Option<f64> {
        self.confirmed.or(self.candidate)
    }
}

fn approx_same_rate(a: f64, b: f64) -> bool {
    if a == b {
        return true;
    }
    let scale = a.abs().max(b.abs()).max(1.0);
    (a - b).abs() / scale < 0.05
}

pub struct RunStateMachine {
    wave: Debounced,
    tier: Debounced,
    coin_rate: DebouncedCoinRate,
    run: Option<ActiveRun>,
    last_coin_rate: Option<f64>,
    last_mode: GameMode,
    tournament_seen: bool,
    /// Consecutive polls where coin reads as a balance (no /min).
    consecutive_total_coin_polls: u32,
}

impl Default for RunStateMachine {
    fn default() -> Self {
        Self::new()
    }
}

impl RunStateMachine {
    pub fn new() -> Self {
        Self {
            wave: Debounced::default(),
            tier: Debounced::default(),
            coin_rate: DebouncedCoinRate::default(),
            run: None,
            last_coin_rate: None,
            last_mode: GameMode::Unknown,
            tournament_seen: false,
            consecutive_total_coin_polls: 0,
        }
    }

    pub fn live_state(&self) -> LiveState {
        LiveState {
            mode: self.last_mode,
            tier: self.tier.display(),
            wave: self.wave.display(),
            coin_per_minute: self.coin_rate.display().or(self.last_coin_rate),
            run_active: self.run.is_some(),
            run_type: self.run.as_ref().map(|r| r.run_type),
            // Debounced: one missed /min frame must not flash the banner.
            total_coin_warning: self.consecutive_total_coin_polls >= 2,
        }
    }

    /// User clicked "New Run": close any active run; the next confirmed
    /// wave starts a fresh one regardless of value.
    pub fn manual_new_run(&mut self) -> Vec<Action> {
        let mut actions = Vec::new();
        if let Some(run) = self.run.take() {
            actions.push(Action::EndRun {
                final_wave: run.last_saved_wave,
                peak_tier: run.peak_tier,
            });
        }
        // Forget confirmed wave so the next confirmed reading can start a run
        // even if it is > 1.
        self.wave = Debounced::default();
        self.tournament_seen = false;
        actions.push(Action::StartRun {
            run_type: RunType::Normal,
        });
        self.run = Some(ActiveRun {
            run_type: RunType::Normal,
            last_saved_wave: 0,
            peak_tier: None,
        });
        actions
    }

    pub fn poll(&mut self, input: PollInput) -> Vec<Action> {
        let mut actions = Vec::new();
        self.last_mode = input.mode;

        if input.mode == GameMode::Tournament {
            self.tournament_seen = true;
        }

        // Coin rate only updates from a /min reading (normal / intro_sprint).
        // Total balances never overwrite the rate (Goal.md total_coin rules).
        match input.coin {
            CoinReading::Rate(v) => {
                if let Some(confirmed) = self.coin_rate.feed(Some(v)) {
                    self.last_coin_rate = Some(confirmed);
                }
                self.consecutive_total_coin_polls = 0;
            }
            CoinReading::Total(_) if matches!(input.mode, GameMode::TotalCoin | GameMode::Tournament) => {
                self.consecutive_total_coin_polls += 1;
            }
            _ => {
                // Unreadable coin line — hold warning state, don't reset streak.
            }
        }

        // End-of-run screen takes priority over everything else.
        if input.mode == GameMode::EndOfRun {
            if let Some(run) = self.run.take() {
                actions.push(Action::EndRun {
                    final_wave: run.last_saved_wave,
                    peak_tier: run.peak_tier,
                });
            }
            // Reset debounce so a stale confirmed wave can't restart the run
            // before the game actually shows wave 1 again.
            self.wave = Debounced::default();
            self.tournament_seen = false;
            return actions;
        }

        let confirmed_tier = self.tier.feed(input.tier);
        let prev_wave = self.wave.confirmed;
        let Some(wave) = self.wave.feed(input.wave) else {
            return actions;
        };
        if prev_wave == Some(wave) {
            return actions; // no confirmed change
        }

        match &mut self.run {
            None => {
                // A run starts when wave 1 is confirmed (Goal.md run lifecycle).
                if wave == 1 {
                    let run_type = if self.tournament_seen {
                        RunType::Tournament
                    } else {
                        RunType::Normal
                    };
                    actions.push(Action::StartRun { run_type });
                    self.run = Some(ActiveRun {
                        run_type,
                        last_saved_wave: 0,
                        peak_tier: None,
                    });
                    actions.extend(self.snapshot(wave, confirmed_tier));
                }
            }
            Some(run) => {
                if wave == 1 && run.last_saved_wave > 1 {
                    // Wave reset: close the run and immediately start the next.
                    let ended = self.run.take().unwrap();
                    actions.push(Action::EndRun {
                        final_wave: ended.last_saved_wave,
                        peak_tier: ended.peak_tier,
                    });
                    let run_type = if self.tournament_seen {
                        RunType::Tournament
                    } else {
                        RunType::Normal
                    };
                    self.tournament_seen = run_type == RunType::Tournament;
                    actions.push(Action::StartRun { run_type });
                    self.run = Some(ActiveRun {
                        run_type,
                        last_saved_wave: 0,
                        peak_tier: None,
                    });
                    actions.extend(self.snapshot(wave, confirmed_tier));
                } else if wave > run.last_saved_wave {
                    // Goal.md says "increases by exactly 1"; we accept any
                    // confirmed increase because at high game speed the wave
                    // can advance more than once between polls.
                    actions.extend(self.snapshot(wave, confirmed_tier));
                }
                // Confirmed decreases (other than reset to 1) are ignored as
                // misreads; debounce already filtered single-frame glitches.
            }
        }
        actions
    }

    fn snapshot(&mut self, wave: u32, tier: Option<u32>) -> Vec<Action> {
        let run = self.run.as_mut().expect("snapshot requires active run");
        run.last_saved_wave = wave;
        if let Some(t) = tier {
            run.peak_tier = Some(run.peak_tier.map_or(t, |p| p.max(t)));
        }
        vec![Action::Snapshot {
            wave,
            tier,
            coin_per_minute: self.last_coin_rate,
        }]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(mode: GameMode, tier: u32, wave: u32, coin: CoinReading) -> PollInput {
        PollInput {
            mode,
            tier: Some(tier),
            wave: Some(wave),
            coin,
        }
    }

    /// Feed the same input twice to satisfy debounce, returning all actions.
    fn feed2(sm: &mut RunStateMachine, input: PollInput) -> Vec<Action> {
        let mut a = sm.poll(input);
        a.extend(sm.poll(input));
        a
    }

    #[test]
    fn live_state_shows_first_poll_before_debounce_confirms() {
        let mut sm = RunStateMachine::new();
        sm.poll(p(GameMode::Normal, 14, 1918, CoinReading::Rate(70.0e12)));
        let live = sm.live_state();
        assert_eq!(live.tier, Some(14));
        assert_eq!(live.wave, Some(1918));
        assert_eq!(live.coin_per_minute, Some(70.0e12));
    }

    #[test]
    fn run_starts_at_wave_1_and_snapshots_increments() {
        let mut sm = RunStateMachine::new();
        let actions = feed2(&mut sm, p(GameMode::Normal, 12, 1, CoinReading::Rate(100.0)));
        assert!(actions.contains(&Action::StartRun { run_type: RunType::Normal }));
        assert!(actions.contains(&Action::Snapshot {
            wave: 1,
            tier: Some(12),
            coin_per_minute: Some(100.0)
        }));

        // Coin rate is debounced — need two matching reads for 150.
        feed2(&mut sm, p(GameMode::Normal, 12, 2, CoinReading::Rate(150.0)));
        let actions = feed2(&mut sm, p(GameMode::Normal, 12, 3, CoinReading::Rate(150.0)));
        assert!(actions.iter().any(|a| matches!(
            a,
            Action::Snapshot { wave: 3, coin_per_minute: Some(150.0), .. }
        )));
    }

    #[test]
    fn coin_rate_median_rejects_single_parseable_outlier() {
        // A drifting rate with one garbled-but-parseable frame in the middle.
        // The reported value must track the drift, not the outlier.
        let mut sm = RunStateMachine::new();
        feed2(&mut sm, p(GameMode::Normal, 12, 1, CoinReading::Rate(70.0e12)));
        sm.poll(p(GameMode::Normal, 12, 1, CoinReading::Rate(71.0e12)));
        // Outlier within the spike ratio (so it isn't gated as a 50× spike),
        // but well off the trend — median should keep us near ~70T.
        sm.poll(p(GameMode::Normal, 12, 1, CoinReading::Rate(5.0e12)));
        sm.poll(p(GameMode::Normal, 12, 1, CoinReading::Rate(72.0e12)));
        let reported = sm.live_state().coin_per_minute.unwrap();
        assert!(
            (60.0e12..=80.0e12).contains(&reported),
            "median should reject the 5T outlier, got {reported}"
        );
    }

    #[test]
    fn coin_rate_spike_requires_extra_confirmation() {
        let mut sm = RunStateMachine::new();
        feed2(&mut sm, p(GameMode::Normal, 12, 1, CoinReading::Rate(100.0e12))); // 100T
        // Single misread at 6q — must not update.
        sm.poll(p(GameMode::Normal, 12, 1, CoinReading::Rate(6.0e15)));
        assert_eq!(sm.live_state().coin_per_minute, Some(100.0e12));
        // Even two frames isn't enough for a 60× spike (needs 3).
        sm.poll(p(GameMode::Normal, 12, 1, CoinReading::Rate(6.0e15)));
        assert_eq!(sm.live_state().coin_per_minute, Some(100.0e12));
    }

    #[test]
    fn debounce_filters_single_frame_misreads() {
        let mut sm = RunStateMachine::new();
        feed2(&mut sm, p(GameMode::Normal, 12, 5, CoinReading::Rate(1.0)));
        assert!(sm.run.is_none(), "wave 5 without wave 1 must not start a run");

        feed2(&mut sm, p(GameMode::Normal, 12, 1, CoinReading::Rate(1.0)));
        assert!(sm.run.is_some());

        // 4321 -> 432 (misread, single frame) -> 4322
        feed2(&mut sm, p(GameMode::Normal, 12, 4321, CoinReading::Rate(1.0)));
        let a = sm.poll(p(GameMode::Normal, 12, 432, CoinReading::Rate(1.0)));
        assert!(a.is_empty(), "single misread frame must produce nothing");
        let a = feed2(&mut sm, p(GameMode::Normal, 12, 4322, CoinReading::Rate(1.0)));
        assert!(a.contains(&Action::Snapshot {
            wave: 4322,
            tier: Some(12),
            coin_per_minute: Some(1.0)
        }));
    }

    #[test]
    fn total_coin_mode_keeps_last_known_rate() {
        let mut sm = RunStateMachine::new();
        feed2(&mut sm, p(GameMode::Normal, 14, 1, CoinReading::Rate(500.0)));
        // total_coin.png scenario: balance shown, rate must not change.
        let actions = feed2(
            &mut sm,
            p(GameMode::TotalCoin, 14, 2, CoinReading::Total(27.46e15)),
        );
        assert!(actions.contains(&Action::Snapshot {
            wave: 2,
            tier: Some(14),
            coin_per_minute: Some(500.0) // last confirmed rate, not the total
        }));
        // feed2 above is two polls — warning should be on for sustained total_coin.
        assert!(sm.live_state().total_coin_warning);
        // Rate returns — warning clears immediately.
        feed2(&mut sm, p(GameMode::Normal, 14, 2, CoinReading::Rate(500.0)));
        assert!(!sm.live_state().total_coin_warning);
    }

    #[test]
    fn intermittent_rate_resets_warning_streak() {
        let mut sm = RunStateMachine::new();
        feed2(&mut sm, p(GameMode::Normal, 14, 1, CoinReading::Rate(100.0)));
        // Single total_coin poll (simulates one OCR frame missing /min).
        sm.poll(p(
            GameMode::TotalCoin,
            14,
            2,
            CoinReading::Total(1e15),
        ));
        assert!(!sm.live_state().total_coin_warning);
        // Rate returns on the next frame — streak clears.
        sm.poll(p(GameMode::Normal, 14, 2, CoinReading::Rate(100.0)));
        assert!(!sm.live_state().total_coin_warning);
    }

    #[test]
    fn total_coin_with_no_prior_rate_stores_null() {
        let mut sm = RunStateMachine::new();
        let actions = feed2(
            &mut sm,
            p(GameMode::TotalCoin, 14, 1, CoinReading::Total(1e15)),
        );
        assert!(actions.contains(&Action::Snapshot {
            wave: 1,
            tier: Some(14),
            coin_per_minute: None
        }));
    }

    #[test]
    fn tournament_run_gets_tagged() {
        let mut sm = RunStateMachine::new();
        // tournament.png scenario: Tier 17+ visible from the start.
        let actions = feed2(
            &mut sm,
            p(GameMode::Tournament, 17, 1, CoinReading::Total(3.06e15)),
        );
        assert!(actions.contains(&Action::StartRun {
            run_type: RunType::Tournament
        }));
    }

    #[test]
    fn end_of_run_screen_ends_run_without_snapshot() {
        let mut sm = RunStateMachine::new();
        feed2(&mut sm, p(GameMode::Normal, 11, 1, CoinReading::Rate(10.0)));
        feed2(&mut sm, p(GameMode::Normal, 11, 2, CoinReading::Rate(10.0)));
        // end_of_run.png scenario: Retry screen.
        let actions = sm.poll(PollInput {
            mode: GameMode::EndOfRun,
            tier: None,
            wave: None,
            coin: CoinReading::Unreadable,
        });
        assert_eq!(
            actions,
            vec![Action::EndRun {
                final_wave: 2,
                peak_tier: Some(11)
            }]
        );
        assert!(sm.run.is_none());

        // Stale high waves after the screen closes must not restart the run...
        let a = feed2(&mut sm, p(GameMode::Normal, 11, 5002, CoinReading::Rate(1.0)));
        assert!(a.is_empty());
        // ...but wave 1 starts the next one.
        let a = feed2(&mut sm, p(GameMode::Normal, 11, 1, CoinReading::Rate(1.0)));
        assert!(a.contains(&Action::StartRun { run_type: RunType::Normal }));
    }

    #[test]
    fn wave_reset_to_1_ends_and_restarts() {
        let mut sm = RunStateMachine::new();
        feed2(&mut sm, p(GameMode::Normal, 14, 1, CoinReading::Rate(10.0)));
        feed2(&mut sm, p(GameMode::Normal, 14, 450, CoinReading::Rate(10.0)));
        let actions = feed2(&mut sm, p(GameMode::Normal, 14, 1, CoinReading::Rate(10.0)));
        assert!(actions.contains(&Action::EndRun {
            final_wave: 450,
            peak_tier: Some(14)
        }));
        assert!(actions.contains(&Action::StartRun { run_type: RunType::Normal }));
    }

    #[test]
    fn peak_tier_tracks_maximum() {
        let mut sm = RunStateMachine::new();
        feed2(&mut sm, p(GameMode::Normal, 13, 1, CoinReading::Rate(1.0)));
        feed2(&mut sm, p(GameMode::Normal, 14, 2, CoinReading::Rate(1.0)));
        feed2(&mut sm, p(GameMode::Normal, 13, 3, CoinReading::Rate(1.0)));
        let actions = sm.poll(PollInput {
            mode: GameMode::EndOfRun,
            tier: None,
            wave: None,
            coin: CoinReading::Unreadable,
        });
        assert_eq!(
            actions,
            vec![Action::EndRun {
                final_wave: 3,
                peak_tier: Some(14)
            }]
        );
    }
}
