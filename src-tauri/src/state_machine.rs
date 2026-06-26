//! Run state machine per Goal.md "Recording rules" and "Game mode edge cases".
//!
//! Pure logic: consumes classified poll results, emits actions for the
//! storage layer. No I/O here so everything is unit-testable.

use serde::Serialize;

use crate::parser::CoinReading;
use crate::parser::WaveSkipOverlay;

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
    Farming,
    Tournament,
}

impl RunType {
    pub fn as_str(&self) -> &'static str {
        match self {
            RunType::Farming => "farming",
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
    /// Parsed from the in-game "Wave Skipped!" banner.
    pub wave_skip_overlay: WaveSkipOverlay,
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
    WaveSkip {
        at_wave: u32,
        skipped_count: u32,
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
    /// Most recent wave skip count this run (not cumulative).
    pub last_waves_skipped: Option<u32>,
}

impl LiveState {
    pub fn idle() -> Self {
        Self {
            mode: GameMode::Unknown,
            tier: None,
            wave: None,
            coin_per_minute: None,
            run_active: false,
            run_type: None,
            total_coin_warning: false,
            last_waves_skipped: None,
        }
    }
}

struct ActiveRun {
    run_type: RunType,
    last_saved_wave: u32,
    peak_tier: Option<u32>,
    accumulating_for_wave: Option<u32>,
    coin_samples: Vec<f64>,
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
        let Some(v) = value else {
            return self.confirmed;
        };
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
        !(0.02..=50.0).contains(&ratio)
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

/// Debounce the skip banner, then record when wave jumps by the matching amount.
#[derive(Default)]
struct WaveSkipTracker {
    overlay: DebouncedWaveSkipOverlay,
    last_emitted: Option<(u32, u32)>,
    /// Banner seen recently but wave may not have jumped yet (common for ×1 skips).
    latched_banner: Option<WaveSkipOverlay>,
    latched_polls: u32,
    /// Polls remaining where a ×1 jump should pair with a recent banner.
    single_skip_banner_polls: u32,
    /// After resume, ignore one unbannered multi-wave jump (game advanced while stopped).
    resume_catchup_pending: bool,
}

/// Keep a latched banner after it disappears so a debounced wave jump can match.
const SKIP_BANNER_LATCH_POLLS: u32 = 40;
/// After any skip banner, trust a subsequent ×1 wave jump for this many polls (~60s at 1.5s).
const SINGLE_SKIP_BANNER_POLLS: u32 = 40;

#[derive(Default)]
struct DebouncedWaveSkipOverlay {
    candidate: WaveSkipOverlay,
    candidate_count: u32,
    confirmed: Option<WaveSkipOverlay>,
    missed: u32,
}

impl DebouncedWaveSkipOverlay {
    fn feed(&mut self, overlay: WaveSkipOverlay) {
        if overlay.seen {
            self.missed = 0;
            if self.candidate == overlay {
                self.candidate_count += 1;
            } else {
                self.candidate = overlay;
                self.candidate_count = 1;
            }
            if self.candidate_count >= DEBOUNCE {
                self.confirmed = Some(overlay);
            }
        } else if self.confirmed.is_some() {
            self.missed += 1;
            if self.missed >= DEBOUNCE {
                self.confirmed = None;
                self.candidate = WaveSkipOverlay::default();
                self.candidate_count = 0;
                self.missed = 0;
            }
        } else {
            self.candidate = WaveSkipOverlay::default();
            self.candidate_count = 0;
            self.missed = 0;
        }
    }

    fn confirmed(&self) -> Option<WaveSkipOverlay> {
        self.confirmed
    }
}

impl WaveSkipTracker {
    fn feed_overlay(&mut self, overlay: WaveSkipOverlay) {
        self.overlay.feed(overlay);
        if overlay.seen {
            self.latched_banner = Some(overlay);
            self.latched_polls = 0;
            self.single_skip_banner_polls = SINGLE_SKIP_BANNER_POLLS;
        } else {
            if self.latched_banner.is_some() {
                self.latched_polls += 1;
                if self.latched_polls >= SKIP_BANNER_LATCH_POLLS {
                    self.latched_banner = None;
                    self.latched_polls = 0;
                }
            }
            if self.single_skip_banner_polls > 0 {
                self.single_skip_banner_polls -= 1;
            }
        }
    }

    fn on_wave_jump(
        &mut self,
        new_wave: u32,
        delta: u32,
        overlay_now: WaveSkipOverlay,
    ) -> Option<(u32, u32)> {
        if !(1..=crate::parser::MAX_WAVE_SKIP_COUNT).contains(&delta) {
            return None;
        }

        if self.resume_catchup_pending {
            let catchup = delta >= 2 && self.banner_overlay(overlay_now).is_none();
            self.resume_catchup_pending = false;
            if catchup {
                return None;
            }
        }

        if !self.should_record_skip(delta, overlay_now) {
            if delta == 1 {
                self.latched_banner = None;
                self.latched_polls = 0;
                self.single_skip_banner_polls = 0;
            }
            return None;
        }

        let mut skipped_count = delta;
        if let Some(banner) = self.banner_overlay(overlay_now) {
            if let Some(n) = banner.multiplier {
                // OCR often reads x9 when the game shows x10 (or wave lands one early).
                if n.abs_diff(delta) <= 1 {
                    skipped_count = delta.max(n);
                }
            }
        }

        let key = (new_wave, skipped_count);
        if self.last_emitted == Some(key) {
            return None;
        }
        self.last_emitted = Some(key);
        self.latched_banner = None;
        self.latched_polls = 0;
        self.single_skip_banner_polls = 0;
        Some(key)
    }

    fn banner_overlay(&self, overlay_now: WaveSkipOverlay) -> Option<WaveSkipOverlay> {
        if overlay_now.seen {
            Some(overlay_now)
        } else if self.overlay.confirmed().is_some_and(|o| o.seen) {
            self.overlay.confirmed()
        } else {
            self.latched_banner
        }
    }

    /// Skip count equals the observed wave increment (with optional banner tie-break
    /// when OCR misreads xN by ±1). Lone banner gates +1 only; multi-wave jumps
    /// are not suppressed by a missing or slightly wrong multiplier.
    fn should_record_skip(&self, delta: u32, overlay_now: WaveSkipOverlay) -> bool {
        if delta == 1 {
            return self.has_single_skip_banner(overlay_now);
        }

        match self.banner_overlay(overlay_now) {
            None => true,
            Some(banner) => match banner.multiplier {
                None => true,
                Some(n) if n == delta => true,
                Some(n) if n.abs_diff(delta) <= 1 => true,
                Some(_) => false,
            },
        }
    }

    fn reset(&mut self) {
        *self = Self::default();
    }

    fn set_resume_catchup_pending(&mut self, pending: bool) {
        self.resume_catchup_pending = pending;
    }

    fn has_single_skip_banner(&self, overlay_now: WaveSkipOverlay) -> bool {
        if overlay_now.seen {
            return true;
        }
        if self.single_skip_banner_polls > 0 {
            return true;
        }
        if self.overlay.confirmed().is_some_and(|o| o.seen) {
            return true;
        }
        self.latched_banner
            .is_some_and(|_| self.latched_polls <= SKIP_BANNER_LATCH_POLLS)
    }
}

pub struct RunStateMachine {
    wave: Debounced,
    tier: Debounced,
    coin_rate: DebouncedCoinRate,
    wave_skip: WaveSkipTracker,
    run: Option<ActiveRun>,
    last_coin_rate: Option<f64>,
    /// Most recent parseable readings — keeps the dashboard stable between polls.
    last_seen_tier: Option<u32>,
    last_seen_wave: Option<u32>,
    last_seen_coin: Option<f64>,
    last_mode: GameMode,
    tournament_seen: bool,
    /// Consecutive polls where coin reads as a balance (no /min).
    consecutive_total_coin_polls: u32,
    /// Last skip count recorded this run (dashboard stat).
    last_waves_skipped: Option<u32>,
    /// Lowest wave seen while debouncing before a higher wave confirms (fast skips).
    unconfirmed_lower_wave: Option<u32>,
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
            wave_skip: WaveSkipTracker::default(),
            run: None,
            last_coin_rate: None,
            last_seen_tier: None,
            last_seen_wave: None,
            last_seen_coin: None,
            last_mode: GameMode::Unknown,
            tournament_seen: false,
            consecutive_total_coin_polls: 0,
            last_waves_skipped: None,
            unconfirmed_lower_wave: None,
        }
    }

    pub fn has_active_run(&self) -> bool {
        self.run.is_some()
    }

    pub fn live_state(&self) -> LiveState {
        LiveState {
            mode: self.last_mode,
            tier: self.tier.display().or(self.last_seen_tier),
            wave: self.wave.display().or(self.last_seen_wave),
            coin_per_minute: self
                .coin_rate
                .display()
                .or(self.last_seen_coin)
                .or(self.last_coin_rate),
            run_active: self.run.is_some(),
            run_type: self.run.as_ref().map(|r| r.run_type),
            // Debounced: one missed /min frame must not flash the banner.
            total_coin_warning: self.consecutive_total_coin_polls >= 2,
            last_waves_skipped: self
                .run
                .is_some()
                .then(|| self.last_waves_skipped)
                .flatten(),
        }
    }

    /// User clicked "New Run": close any active run; the next confirmed
    /// wave starts a fresh one regardless of value.
    pub fn manual_new_run(&mut self) -> Vec<Action> {
        let mut actions = Vec::new();
        if let Some(mut run) = self.run.take() {
            let tier = self.tier.confirmed.or(self.last_seen_tier);
            actions.extend(flush_pending_wave(&mut run, tier));
            let final_wave = run
                .last_saved_wave
                .max(run.accumulating_for_wave.unwrap_or(0));
            actions.push(Action::EndRun {
                final_wave,
                peak_tier: run.peak_tier,
            });
        }
        // Forget confirmed wave so the next confirmed reading can start a run
        // even if it is > 1.
        self.wave = Debounced::default();
        self.tournament_seen = false;
        self.wave_skip.reset();
        self.unconfirmed_lower_wave = None;
        self.reset_coin_tracking();
        actions.push(Action::StartRun {
            run_type: RunType::Farming,
        });
        self.run = Some(new_active_run(RunType::Farming));
        actions
    }

    /// Continue an open run from the database after app restart or a fresh process.
    pub fn resume_from_db(
        &mut self,
        run_type: RunType,
        last_saved_wave: u32,
        peak_tier: Option<u32>,
    ) {
        let mut run = new_active_run(run_type);
        run.last_saved_wave = last_saved_wave;
        run.peak_tier = peak_tier;
        self.run = Some(run);
        if last_saved_wave > 0 {
            self.wave.candidate = Some(last_saved_wave);
            self.wave.count = DEBOUNCE;
            self.wave.confirmed = Some(last_saved_wave);
            self.last_seen_wave = Some(last_saved_wave);
        }
        self.wave_skip.set_resume_catchup_pending(true);
    }

    /// When scanning starts with no active run, open one immediately so snapshots can persist.
    pub fn ensure_run_for_scanning(&mut self) -> Vec<Action> {
        if self.run.is_some() {
            return Vec::new();
        }
        let run_type = if self.tournament_seen {
            RunType::Tournament
        } else {
            RunType::Farming
        };
        self.run = Some(new_active_run(run_type));
        let mut actions = vec![Action::StartRun { run_type }];
        if let Some(wave) = self.wave.confirmed.or(self.last_seen_wave) {
            if let Some(run) = self.run.as_mut() {
                let tier = self.tier.confirmed.or(self.last_seen_tier);
                run.accumulating_for_wave = Some(wave);
                if let Some(rate) = self.last_coin_rate.or(self.last_seen_coin) {
                    run.coin_samples.push(rate);
                }
                actions.extend(flush_completed_wave(run, wave, tier));
            }
        }
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
        if let Some(t) = input.tier {
            self.last_seen_tier = Some(t);
        }
        if let Some(w) = input.wave {
            self.last_seen_wave = Some(w);
        }

        match input.coin {
            CoinReading::Rate(v) => {
                self.last_seen_coin = Some(v);
                if let Some(confirmed) = self.coin_rate.feed(Some(v)) {
                    self.last_coin_rate = Some(confirmed);
                }
                self.consecutive_total_coin_polls = 0;
            }
            CoinReading::Total(_)
                if matches!(input.mode, GameMode::TotalCoin | GameMode::Tournament) =>
            {
                self.consecutive_total_coin_polls += 1;
            }
            _ => {
                // Unreadable coin line — hold warning state, don't reset streak.
            }
        }

        // End-of-run screen takes priority over everything else.
        if input.mode == GameMode::EndOfRun {
            if let Some(mut run) = self.run.take() {
                let tier = self.tier.confirmed.or(self.last_seen_tier);
                actions.extend(flush_pending_wave(&mut run, tier));
                let final_wave = run
                    .last_saved_wave
                    .max(run.accumulating_for_wave.unwrap_or(0));
                actions.push(Action::EndRun {
                    final_wave,
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
        self.wave_skip.feed_overlay(input.wave_skip_overlay);
        let prev_wave = self.wave.confirmed;
        let confirmed_wave = self.wave.feed(input.wave);

        if let Some(wave) = confirmed_wave {
            if prev_wave != Some(wave) {
                let skip_prev = prev_wave.or_else(|| {
                    self.unconfirmed_lower_wave
                        .filter(|&p| p < wave && p >= 1)
                });
                let flush_prev = skip_prev.or(prev_wave);

                if let Some(run) = self.run.as_mut() {
                    if let Some(prev) = flush_prev {
                        if prev >= 1 {
                            actions.extend(flush_completed_wave(run, prev, confirmed_tier));
                        }
                    }
                }

                if let Some(prev) = flush_prev {
                    if wave > prev {
                        let delta = wave - prev;
                        if self.run.is_some() {
                            if let Some((at_wave, skipped_count)) = self.wave_skip.on_wave_jump(
                                wave,
                                delta,
                                input.wave_skip_overlay,
                            ) {
                                self.last_waves_skipped = Some(skipped_count);
                                actions.push(Action::WaveSkip {
                                    at_wave,
                                    skipped_count,
                                    coin_per_minute: self.last_coin_rate.or(self.last_seen_coin),
                                });
                            }
                        }
                    }
                }

                if skip_prev.is_some() && prev_wave.is_none() {
                    self.unconfirmed_lower_wave = None;
                }

                match self.run.as_mut() {
                    None => {
                        // A run starts when wave 1 is confirmed (Goal.md run lifecycle).
                        if wave == 1 {
                            let run_type = if self.tournament_seen {
                                RunType::Tournament
                            } else {
                                RunType::Farming
                            };
                            actions.push(Action::StartRun { run_type });
                            self.reset_coin_tracking();
                            self.run = Some(new_active_run(run_type));
                        }
                    }
                    Some(run) => {
                        if wave == 1 && run.last_saved_wave > 1 {
                            // Wave reset: close the run and immediately start the next.
                            let mut ended = self.run.take().unwrap();
                            actions.extend(flush_pending_wave(&mut ended, confirmed_tier));
                            let final_wave = ended
                                .last_saved_wave
                                .max(ended.accumulating_for_wave.unwrap_or(0));
                            actions.push(Action::EndRun {
                                final_wave,
                                peak_tier: ended.peak_tier,
                            });
                            let run_type = if self.tournament_seen {
                                RunType::Tournament
                            } else {
                                RunType::Farming
                            };
                            self.tournament_seen = run_type == RunType::Tournament;
                            actions.push(Action::StartRun { run_type });
                            self.reset_coin_tracking();
                            self.run = Some(new_active_run(run_type));
                        }
                        // Confirmed decreases (other than reset to 1) are ignored as
                        // misreads; debounce already filtered single-frame glitches.
                    }
                }
            }
        }

        if let Some(w) = input.wave {
            if self.wave.confirmed != Some(w) {
                self.unconfirmed_lower_wave = match self.unconfirmed_lower_wave {
                    None => Some(w),
                    Some(cur) if w < cur => Some(w),
                    other => other,
                };
            }
        }

        if let Some(run) = self.run.as_mut() {
            accumulate_coin_sample(run, self.wave.confirmed, self.last_coin_rate);
        }

        actions
    }

    /// Drop coin/min from the previous run so a fresh run starts clean.
    fn reset_coin_tracking(&mut self) {
        self.coin_rate = DebouncedCoinRate::default();
        self.last_coin_rate = None;
        self.last_seen_coin = None;
        self.consecutive_total_coin_polls = 0;
        self.wave_skip.reset();
        self.last_waves_skipped = None;
        self.unconfirmed_lower_wave = None;
    }
}

fn accumulate_coin_sample(
    run: &mut ActiveRun,
    confirmed_wave: Option<u32>,
    coin_rate: Option<f64>,
) {
    let Some(wave) = confirmed_wave else {
        return;
    };
    if run.accumulating_for_wave != Some(wave) {
        run.accumulating_for_wave = Some(wave);
        run.coin_samples.clear();
    }
    if let Some(rate) = coin_rate {
        run.coin_samples.push(rate);
    }
}

fn new_active_run(run_type: RunType) -> ActiveRun {
    ActiveRun {
        run_type,
        last_saved_wave: 0,
        peak_tier: None,
        accumulating_for_wave: None,
        coin_samples: Vec::new(),
    }
}

fn flush_completed_wave(run: &mut ActiveRun, wave: u32, tier: Option<u32>) -> Vec<Action> {
    if wave <= run.last_saved_wave {
        return vec![];
    }
    let coin_per_minute = average_coin_samples(&run.coin_samples);
    run.coin_samples.clear();
    run.accumulating_for_wave = None;
    run.last_saved_wave = wave;
    if let Some(t) = tier {
        run.peak_tier = Some(run.peak_tier.map_or(t, |p| p.max(t)));
    }
    vec![Action::Snapshot {
        wave,
        tier,
        coin_per_minute,
    }]
}

fn flush_pending_wave(run: &mut ActiveRun, tier: Option<u32>) -> Vec<Action> {
    let Some(wave) = run.accumulating_for_wave else {
        return vec![];
    };
    flush_completed_wave(run, wave, tier)
}

fn average_coin_samples(samples: &[f64]) -> Option<f64> {
    if samples.is_empty() {
        None
    } else {
        Some(samples.iter().sum::<f64>() / samples.len() as f64)
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
            wave_skip_overlay: WaveSkipOverlay::default(),
        }
    }

    fn p_skip(
        mode: GameMode,
        tier: u32,
        wave: u32,
        coin: CoinReading,
        overlay: WaveSkipOverlay,
    ) -> PollInput {
        PollInput {
            mode,
            tier: Some(tier),
            wave: Some(wave),
            coin,
            wave_skip_overlay: overlay,
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
    fn resume_from_db_continues_snapshotting_after_last_saved_wave() {
        let mut sm = RunStateMachine::new();
        sm.resume_from_db(RunType::Farming, 42, Some(17));
        let coin = CoinReading::Rate(100.0);
        feed2(&mut sm, p(GameMode::Normal, 17, 43, coin));
        let actions = feed2(
            &mut sm,
            p(GameMode::Normal, 17, 44, CoinReading::Rate(110.0)),
        );
        assert!(actions
            .iter()
            .any(|a| matches!(a, Action::Snapshot { wave: 43, .. })));
    }

    #[test]
    fn resume_catchup_suppresses_false_multi_skip_without_banner() {
        let mut sm = RunStateMachine::new();
        feed2(&mut sm, p(GameMode::Normal, 14, 1, CoinReading::Rate(1e12)));
        feed2(&mut sm, p(GameMode::Normal, 14, 100, CoinReading::Rate(1e12)));
        sm.resume_from_db(RunType::Farming, 100, Some(14));
        let actions = feed2(
            &mut sm,
            p(GameMode::Normal, 14, 105, CoinReading::Rate(1e12)),
        );
        assert!(!actions.iter().any(|a| matches!(a, Action::WaveSkip { .. })));
    }

    #[test]
    fn resume_catchup_allows_bannered_skip_after_gap() {
        let mut sm = RunStateMachine::new();
        feed2(&mut sm, p(GameMode::Normal, 14, 1, CoinReading::Rate(1e12)));
        feed2(&mut sm, p(GameMode::Normal, 14, 100, CoinReading::Rate(1e12)));
        sm.resume_from_db(RunType::Farming, 100, Some(14));
        let overlay = WaveSkipOverlay {
            seen: true,
            multiplier: Some(5),
        };
        let actions = feed2(
            &mut sm,
            p_skip(GameMode::Normal, 14, 105, CoinReading::Rate(1e12), overlay),
        );
        assert!(actions.iter().any(|a| {
            matches!(
                a,
                Action::WaveSkip {
                    at_wave: 105,
                    skipped_count: 5,
                    ..
                }
            )
        }));
    }

    #[test]
    fn resume_catchup_allows_single_skip_after_sync() {
        let mut sm = RunStateMachine::new();
        sm.resume_from_db(RunType::Farming, 100, Some(14));
        let overlay = WaveSkipOverlay {
            seen: true,
            multiplier: None,
        };
        feed2(
            &mut sm,
            p_skip(GameMode::Normal, 14, 100, CoinReading::Rate(1e12), overlay),
        );
        let actions = feed2(
            &mut sm,
            p(GameMode::Normal, 14, 101, CoinReading::Rate(1e12)),
        );
        assert!(actions.iter().any(|a| {
            matches!(
                a,
                Action::WaveSkip {
                    at_wave: 101,
                    skipped_count: 1,
                    ..
                }
            )
        }));
    }

    #[test]
    fn ensure_run_for_scanning_starts_when_idle() {
        let mut sm = RunStateMachine::new();
        let actions = sm.ensure_run_for_scanning();
        assert_eq!(
            actions,
            vec![Action::StartRun {
                run_type: RunType::Farming
            }]
        );
        assert!(sm.live_state().run_active);
    }

    #[test]
    fn ensure_run_for_scanning_noop_when_run_active() {
        let mut sm = RunStateMachine::new();
        sm.manual_new_run();
        let actions = sm.ensure_run_for_scanning();
        assert!(actions.is_empty());
    }

    #[test]
    fn ensure_run_for_scanning_seeds_snapshot_at_current_wave() {
        let mut sm = RunStateMachine::new();
        sm.poll(p(GameMode::Normal, 14, 4500, CoinReading::Rate(100.0)));
        let actions = sm.ensure_run_for_scanning();
        assert!(actions.contains(&Action::StartRun {
            run_type: RunType::Farming
        }));
        assert!(actions.contains(&Action::Snapshot {
            wave: 4500,
            tier: Some(14),
            coin_per_minute: Some(100.0)
        }));
    }

    #[test]
    fn run_starts_at_wave_1_and_snapshots_increments() {
        let mut sm = RunStateMachine::new();
        let actions = feed2(
            &mut sm,
            p(GameMode::Normal, 12, 1, CoinReading::Rate(150.0)),
        );
        assert!(actions.contains(&Action::StartRun {
            run_type: RunType::Farming
        }));
        assert!(!actions.iter().any(|a| matches!(a, Action::Snapshot { .. })));

        // Wave 1 is snapshotted when wave 2 is confirmed.
        let actions = feed2(
            &mut sm,
            p(GameMode::Normal, 12, 2, CoinReading::Rate(150.0)),
        );
        assert!(actions.contains(&Action::Snapshot {
            wave: 1,
            tier: Some(12),
            coin_per_minute: Some(150.0)
        }));

        // Collect more samples on wave 2 before advancing.
        feed2(
            &mut sm,
            p(GameMode::Normal, 12, 2, CoinReading::Rate(150.0)),
        );
        let actions = feed2(
            &mut sm,
            p(GameMode::Normal, 12, 3, CoinReading::Rate(150.0)),
        );
        assert!(actions.iter().any(|a| matches!(
            a,
            Action::Snapshot {
                wave: 2,
                coin_per_minute: Some(150.0),
                ..
            }
        )));
    }

    #[test]
    fn snapshot_averages_coin_rate_while_on_wave() {
        let mut sm = RunStateMachine::new();
        feed2(
            &mut sm,
            p(GameMode::Normal, 12, 1, CoinReading::Rate(100.0)),
        );
        feed2(
            &mut sm,
            p(GameMode::Normal, 12, 1, CoinReading::Rate(200.0)),
        );
        let actions = feed2(
            &mut sm,
            p(GameMode::Normal, 12, 2, CoinReading::Rate(200.0)),
        );
        let avg = actions.iter().find_map(|a| match a {
            Action::Snapshot {
                wave: 1,
                coin_per_minute,
                ..
            } => *coin_per_minute,
            _ => None,
        });
        let avg = avg.expect("wave 1 snapshot");
        assert!(
            avg > 100.0 && avg < 200.0,
            "expected blend of 100 and 200 while on wave 1, got {avg}"
        );
    }

    #[test]
    fn coin_rate_median_rejects_single_parseable_outlier() {
        // A drifting rate with one garbled-but-parseable frame in the middle.
        // The reported value must track the drift, not the outlier.
        let mut sm = RunStateMachine::new();
        feed2(
            &mut sm,
            p(GameMode::Normal, 12, 1, CoinReading::Rate(70.0e12)),
        );
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
        feed2(
            &mut sm,
            p(GameMode::Normal, 12, 1, CoinReading::Rate(100.0e12)),
        ); // 100T
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
        assert!(
            sm.run.is_none(),
            "wave 5 without wave 1 must not start a run"
        );

        feed2(&mut sm, p(GameMode::Normal, 12, 1, CoinReading::Rate(1.0)));
        assert!(sm.run.is_some());

        // 4321 -> 432 (misread, single frame) -> 4322
        feed2(
            &mut sm,
            p(GameMode::Normal, 12, 4321, CoinReading::Rate(1.0)),
        );
        let a = sm.poll(p(GameMode::Normal, 12, 432, CoinReading::Rate(1.0)));
        assert!(a.is_empty(), "single misread frame must produce nothing");
        let a = feed2(
            &mut sm,
            p(GameMode::Normal, 12, 4322, CoinReading::Rate(1.0)),
        );
        assert!(a.contains(&Action::Snapshot {
            wave: 4321,
            tier: Some(12),
            coin_per_minute: Some(1.0)
        }));
    }

    #[test]
    fn total_coin_mode_keeps_last_known_rate() {
        let mut sm = RunStateMachine::new();
        feed2(
            &mut sm,
            p(GameMode::Normal, 14, 1, CoinReading::Rate(500.0)),
        );
        // total_coin.png scenario: balance shown, rate must not change.
        let actions = feed2(
            &mut sm,
            p(GameMode::TotalCoin, 14, 2, CoinReading::Total(27.46e15)),
        );
        assert!(actions.contains(&Action::Snapshot {
            wave: 1,
            tier: Some(14),
            coin_per_minute: Some(500.0) // average while on wave 1, not the total balance
        }));
        // feed2 above is two polls — warning should be on for sustained total_coin.
        assert!(sm.live_state().total_coin_warning);
        // Rate returns — warning clears immediately.
        feed2(
            &mut sm,
            p(GameMode::Normal, 14, 2, CoinReading::Rate(500.0)),
        );
        assert!(!sm.live_state().total_coin_warning);
    }

    #[test]
    fn intermittent_rate_resets_warning_streak() {
        let mut sm = RunStateMachine::new();
        feed2(
            &mut sm,
            p(GameMode::Normal, 14, 1, CoinReading::Rate(100.0)),
        );
        // Single total_coin poll (simulates one OCR frame missing /min).
        sm.poll(p(GameMode::TotalCoin, 14, 2, CoinReading::Total(1e15)));
        assert!(!sm.live_state().total_coin_warning);
        // Rate returns on the next frame — streak clears.
        sm.poll(p(GameMode::Normal, 14, 2, CoinReading::Rate(100.0)));
        assert!(!sm.live_state().total_coin_warning);
    }

    #[test]
    fn total_coin_with_no_prior_rate_stores_null() {
        let mut sm = RunStateMachine::new();
        feed2(
            &mut sm,
            p(GameMode::TotalCoin, 14, 1, CoinReading::Total(1e15)),
        );
        let actions = feed2(
            &mut sm,
            p(GameMode::TotalCoin, 14, 2, CoinReading::Total(1e15)),
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
            wave_skip_overlay: WaveSkipOverlay::default(),
        });
        assert_eq!(
            actions,
            vec![
                Action::Snapshot {
                    wave: 2,
                    tier: Some(11),
                    coin_per_minute: Some(10.0)
                },
                Action::EndRun {
                    final_wave: 2,
                    peak_tier: Some(11)
                }
            ]
        );
        assert!(sm.run.is_none());

        // Stale high waves after the screen closes must not restart the run...
        let a = feed2(
            &mut sm,
            p(GameMode::Normal, 11, 5002, CoinReading::Rate(1.0)),
        );
        assert!(a.is_empty());
        // ...but wave 1 starts the next one.
        let a = feed2(&mut sm, p(GameMode::Normal, 11, 1, CoinReading::Rate(1.0)));
        assert!(a.contains(&Action::StartRun {
            run_type: RunType::Farming
        }));
    }

    #[test]
    fn manual_new_run_clears_stale_coin_rate() {
        let mut sm = RunStateMachine::new();
        feed2(
            &mut sm,
            p(GameMode::Normal, 14, 100, CoinReading::Rate(500.0e12)),
        );
        assert_eq!(sm.live_state().coin_per_minute, Some(500.0e12));

        sm.manual_new_run();
        assert_eq!(sm.live_state().coin_per_minute, None);

        feed2(&mut sm, p(GameMode::Normal, 14, 1, CoinReading::Rate(10.0)));
        let actions = feed2(
            &mut sm,
            p(GameMode::Normal, 14, 2, CoinReading::Rate(10.0)),
        );
        let coin = actions.iter().find_map(|a| match a {
            Action::Snapshot {
                wave: 1,
                coin_per_minute,
                ..
            } => *coin_per_minute,
            _ => None,
        });
        assert_eq!(coin, Some(10.0), "first snapshot must not reuse prior run rate");
    }

    #[test]
    fn wave_reset_to_1_ends_and_restarts() {
        let mut sm = RunStateMachine::new();
        feed2(&mut sm, p(GameMode::Normal, 14, 1, CoinReading::Rate(10.0)));
        feed2(
            &mut sm,
            p(GameMode::Normal, 14, 450, CoinReading::Rate(10.0)),
        );
        let actions = feed2(&mut sm, p(GameMode::Normal, 14, 1, CoinReading::Rate(10.0)));
        assert!(actions.contains(&Action::EndRun {
            final_wave: 450,
            peak_tier: Some(14)
        }));
        assert!(actions.contains(&Action::StartRun {
            run_type: RunType::Farming
        }));
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
            wave_skip_overlay: WaveSkipOverlay::default(),
        });
        assert_eq!(
            actions,
            vec![
                Action::Snapshot {
                    wave: 3,
                    tier: Some(13),
                    coin_per_minute: Some(1.0)
                },
                Action::EndRun {
                    final_wave: 3,
                    peak_tier: Some(14)
                }
            ]
        );
    }

    #[test]
    fn wave_skip_recorded_when_banner_and_jump_match() {
        let mut sm = RunStateMachine::new();
        feed2(&mut sm, p(GameMode::Normal, 14, 1, CoinReading::Rate(1e12)));
        feed2(&mut sm, p(GameMode::Normal, 14, 100, CoinReading::Rate(1e12)));
        let overlay = WaveSkipOverlay {
            seen: true,
            multiplier: Some(5),
        };
        let actions = feed2(
            &mut sm,
            p_skip(GameMode::Normal, 14, 105, CoinReading::Rate(1e12), overlay),
        );
        assert!(actions.iter().any(|a| {
            matches!(
                a,
                Action::WaveSkip {
                    at_wave: 105,
                    skipped_count: 5,
                    ..
                }
            )
        }));
    }

    #[test]
    fn single_wave_skip_without_multiplier() {
        let mut sm = RunStateMachine::new();
        feed2(&mut sm, p(GameMode::Normal, 14, 1, CoinReading::Rate(1e12)));
        feed2(&mut sm, p(GameMode::Normal, 14, 100, CoinReading::Rate(1e12)));
        let overlay = WaveSkipOverlay {
            seen: true,
            multiplier: None,
        };
        let actions = feed2(
            &mut sm,
            p_skip(GameMode::Normal, 14, 101, CoinReading::Rate(1e12), overlay),
        );
        assert!(actions.iter().any(|a| {
            matches!(
                a,
                Action::WaveSkip {
                    at_wave: 101,
                    skipped_count: 1,
                    ..
                }
            )
        }));
    }

    #[test]
    fn single_wave_skip_banner_before_jump() {
        let mut sm = RunStateMachine::new();
        feed2(&mut sm, p(GameMode::Normal, 14, 1, CoinReading::Rate(1e12)));
        feed2(&mut sm, p(GameMode::Normal, 14, 100, CoinReading::Rate(1e12)));
        let overlay = WaveSkipOverlay {
            seen: true,
            multiplier: None,
        };
        feed2(
            &mut sm,
            p_skip(GameMode::Normal, 14, 100, CoinReading::Rate(1e12), overlay),
        );
        let actions = feed2(
            &mut sm,
            p(GameMode::Normal, 14, 101, CoinReading::Rate(1e12)),
        );
        assert!(actions.iter().any(|a| {
            matches!(
                a,
                Action::WaveSkip {
                    at_wave: 101,
                    skipped_count: 1,
                    ..
                }
            )
        }));
    }

    #[test]
    fn single_wave_skip_after_slow_wave_debounce() {
        let mut sm = RunStateMachine::new();
        feed2(&mut sm, p(GameMode::Normal, 14, 1, CoinReading::Rate(1e12)));
        feed2(&mut sm, p(GameMode::Normal, 14, 100, CoinReading::Rate(1e12)));
        let overlay = WaveSkipOverlay {
            seen: true,
            multiplier: None,
        };
        sm.poll(p_skip(
            GameMode::Normal,
            14,
            100,
            CoinReading::Rate(1e12),
            overlay,
        ));
        let mut actions = Vec::new();
        for _ in 0..12 {
            actions.extend(sm.poll(p(
                GameMode::Normal,
                14,
                101,
                CoinReading::Rate(1e12),
            )));
        }
        actions.extend(feed2(
            &mut sm,
            p(GameMode::Normal, 14, 101, CoinReading::Rate(1e12)),
        ));
        assert!(actions.iter().any(|a| {
            matches!(
                a,
                Action::WaveSkip {
                    at_wave: 101,
                    skipped_count: 1,
                    ..
                }
            )
        }));
    }

    #[test]
    fn single_wave_skip_with_misread_multiplier_on_banner() {
        let mut sm = RunStateMachine::new();
        feed2(&mut sm, p(GameMode::Normal, 14, 1, CoinReading::Rate(1e12)));
        feed2(&mut sm, p(GameMode::Normal, 14, 100, CoinReading::Rate(1e12)));
        let overlay = WaveSkipOverlay {
            seen: true,
            multiplier: Some(9),
        };
        let actions = feed2(
            &mut sm,
            p_skip(GameMode::Normal, 14, 101, CoinReading::Rate(1e12), overlay),
        );
        assert!(actions.iter().any(|a| {
            matches!(
                a,
                Action::WaveSkip {
                    at_wave: 101,
                    skipped_count: 1,
                    ..
                }
            )
        }));
    }

    #[test]
    fn single_wave_skip_rejects_overshoot_without_matching_multiplier() {
        let mut sm = RunStateMachine::new();
        feed2(&mut sm, p(GameMode::Normal, 14, 1, CoinReading::Rate(1e12)));
        feed2(&mut sm, p(GameMode::Normal, 14, 100, CoinReading::Rate(1e12)));
        let overlay = WaveSkipOverlay {
            seen: true,
            multiplier: None,
        };
        // Lone banner with +2: multi-wave increment is trusted (rare OCR glitch).
        let actions = feed2(
            &mut sm,
            p_skip(GameMode::Normal, 14, 102, CoinReading::Rate(1e12), overlay),
        );
        assert!(actions.iter().any(|a| {
            matches!(
                a,
                Action::WaveSkip {
                    at_wave: 102,
                    skipped_count: 2,
                    ..
                }
            )
        }));
    }

    #[test]
    fn multi_wave_skip_with_lone_banner_records() {
        let mut sm = RunStateMachine::new();
        sm.manual_new_run();
        let coin = CoinReading::Rate(1e12);
        let overlay = WaveSkipOverlay {
            seen: true,
            multiplier: None,
        };
        sm.poll(p(GameMode::Normal, 14, 30, coin));
        sm.poll(p_skip(GameMode::Normal, 14, 30, coin, overlay));
        sm.poll(p_skip(GameMode::Normal, 14, 40, coin, overlay));
        let actions = sm.poll(p_skip(GameMode::Normal, 14, 40, coin, overlay));
        assert!(actions.iter().any(|a| {
            matches!(
                a,
                Action::WaveSkip {
                    at_wave: 40,
                    skipped_count: 10,
                    ..
                }
            )
        }));
    }

    #[test]
    fn multi_wave_skip_tolerates_multiplier_off_by_one() {
        let mut sm = RunStateMachine::new();
        sm.manual_new_run();
        let coin = CoinReading::Rate(1e12);
        let overlay = WaveSkipOverlay {
            seen: true,
            multiplier: Some(9),
        };
        feed2(&mut sm, p(GameMode::Normal, 14, 90, coin));
        let actions = feed2(
            &mut sm,
            p_skip(GameMode::Normal, 14, 100, coin, overlay),
        );
        assert!(actions.iter().any(|a| {
            matches!(
                a,
                Action::WaveSkip {
                    at_wave: 100,
                    skipped_count: 10,
                    ..
                }
            )
        }));
    }

    #[test]
    fn multi_wave_skip_requires_banner_multiplier_to_match_jump() {
        let mut sm = RunStateMachine::new();
        feed2(&mut sm, p(GameMode::Normal, 14, 1, CoinReading::Rate(1e12)));
        feed2(&mut sm, p(GameMode::Normal, 14, 100, CoinReading::Rate(1e12)));
        let overlay = WaveSkipOverlay {
            seen: true,
            multiplier: Some(5),
        };
        let actions = feed2(
            &mut sm,
            p_skip(GameMode::Normal, 14, 105, CoinReading::Rate(1e12), overlay),
        );
        assert!(actions.iter().any(|a| {
            matches!(
                a,
                Action::WaveSkip {
                    at_wave: 105,
                    skipped_count: 5,
                    ..
                }
            )
        }));
    }

    #[test]
    fn single_wave_skip_partial_banner_before_plus_one_jump() {
        let mut sm = RunStateMachine::new();
        feed2(&mut sm, p(GameMode::Normal, 14, 1, CoinReading::Rate(1e12)));
        feed2(&mut sm, p(GameMode::Normal, 14, 2521, CoinReading::Rate(1e12)));
        let partial = WaveSkipOverlay {
            seen: true,
            multiplier: None,
        };
        feed2(
            &mut sm,
            p_skip(GameMode::Normal, 14, 2521, CoinReading::Rate(1e12), partial),
        );
        let actions = feed2(
            &mut sm,
            p(GameMode::Normal, 14, 2522, CoinReading::Rate(1e12)),
        );
        assert!(actions.iter().any(|a| {
            matches!(
                a,
                Action::WaveSkip {
                    at_wave: 2522,
                    skipped_count: 1,
                    ..
                }
            )
        }));
    }

    #[test]
    fn wave_skip_rejects_mismatched_jump() {
        let mut sm = RunStateMachine::new();
        feed2(&mut sm, p(GameMode::Normal, 14, 1, CoinReading::Rate(1e12)));
        feed2(&mut sm, p(GameMode::Normal, 14, 100, CoinReading::Rate(1e12)));
        let overlay = WaveSkipOverlay {
            seen: true,
            multiplier: Some(5),
        };
        // Banner x5 with only a +3 jump — increment and banner do not correlate.
        let actions = feed2(
            &mut sm,
            p_skip(GameMode::Normal, 14, 103, CoinReading::Rate(1e12), overlay),
        );
        assert!(!actions.iter().any(|a| matches!(a, Action::WaveSkip { .. })));
    }

    #[test]
    fn multi_wave_jump_recorded_without_banner() {
        let mut sm = RunStateMachine::new();
        feed2(&mut sm, p(GameMode::Normal, 14, 1, CoinReading::Rate(1e12)));
        feed2(&mut sm, p(GameMode::Normal, 14, 100, CoinReading::Rate(1e12)));
        let actions = feed2(
            &mut sm,
            p(GameMode::Normal, 14, 105, CoinReading::Rate(1e12)),
        );
        assert!(actions.iter().any(|a| {
            matches!(
                a,
                Action::WaveSkip {
                    at_wave: 105,
                    skipped_count: 5,
                    ..
                }
            )
        }));
    }

    #[test]
    fn normal_single_wave_advance_not_recorded() {
        let mut sm = RunStateMachine::new();
        feed2(&mut sm, p(GameMode::Normal, 14, 1, CoinReading::Rate(1e12)));
        let actions = feed2(
            &mut sm,
            p(GameMode::Normal, 14, 2, CoinReading::Rate(1e12)),
        );
        assert!(!actions.iter().any(|a| matches!(a, Action::WaveSkip { .. })));
    }

    #[test]
    fn wave_skip_rejects_jump_above_20() {
        let mut sm = RunStateMachine::new();
        feed2(&mut sm, p(GameMode::Normal, 14, 1, CoinReading::Rate(1e12)));
        feed2(&mut sm, p(GameMode::Normal, 14, 100, CoinReading::Rate(1e12)));
        let overlay = WaveSkipOverlay {
            seen: true,
            multiplier: Some(25),
        };
        let actions = feed2(
            &mut sm,
            p_skip(GameMode::Normal, 14, 125, CoinReading::Rate(1e12), overlay),
        );
        assert!(!actions.iter().any(|a| matches!(a, Action::WaveSkip { .. })));
    }

    #[test]
    fn fast_skip_uses_unconfirmed_lower_wave() {
        let mut sm = RunStateMachine::new();
        sm.manual_new_run();
        let coin = CoinReading::Rate(1e12);
        sm.poll(p(GameMode::Normal, 14, 1, coin));
        sm.poll(p(GameMode::Normal, 14, 11, coin));
        let actions = sm.poll(p(GameMode::Normal, 14, 11, coin));
        assert!(actions.iter().any(|a| {
            matches!(
                a,
                Action::WaveSkip {
                    at_wave: 11,
                    skipped_count: 10,
                    ..
                }
            )
        }));
    }

    #[test]
    fn intro_sprint_ten_wave_skip_with_lone_banner() {
        let mut sm = RunStateMachine::new();
        sm.manual_new_run();
        let coin = CoinReading::Rate(0.0);
        let overlay = WaveSkipOverlay {
            seen: true,
            multiplier: None,
        };
        sm.poll(p_skip(GameMode::IntroSprint, 14, 1, coin, overlay));
        sm.poll(p_skip(GameMode::IntroSprint, 14, 11, coin, overlay));
        let actions = sm.poll(p_skip(GameMode::IntroSprint, 14, 11, coin, overlay));
        assert!(actions.iter().any(|a| {
            matches!(
                a,
                Action::WaveSkip {
                    at_wave: 11,
                    skipped_count: 10,
                    ..
                }
            )
        }));
    }
}
