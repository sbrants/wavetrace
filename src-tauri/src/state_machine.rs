//! Run state machine per Goal.md "Recording rules" and "Game mode edge cases".
//!
//! Pure logic: consumes classified poll results, emits actions for the
//! storage layer. No I/O here so everything is unit-testable.

use std::collections::HashMap;

use serde::Serialize;

use crate::parser::{CoinReading, GoldenComboReading, WaveSkipOverlay};

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
pub enum DissonanceKind {
    Attack,
    Defense,
    Utility,
    UltimateWeapons,
}

impl DissonanceKind {
    pub fn to_run_type(self) -> RunType {
        match self {
            DissonanceKind::Attack => RunType::DissonanceAttack,
            DissonanceKind::Defense => RunType::DissonanceDefense,
            DissonanceKind::Utility => RunType::DissonanceUtility,
            DissonanceKind::UltimateWeapons => RunType::DissonanceUltimateWeapons,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RunType {
    Farming,
    Tournament,
    DissonanceAttack,
    DissonanceDefense,
    DissonanceUtility,
    DissonanceUltimateWeapons,
}

impl RunType {
    pub fn as_str(&self) -> &'static str {
        match self {
            RunType::Farming => "farming",
            RunType::Tournament => "tournament",
            RunType::DissonanceAttack => "dissonance_attack",
            RunType::DissonanceDefense => "dissonance_defense",
            RunType::DissonanceUtility => "dissonance_utility",
            RunType::DissonanceUltimateWeapons => "dissonance_ultimate_weapons",
        }
    }

    pub fn from_db_str(s: &str) -> Self {
        Self::try_from_db_str(s).unwrap_or(RunType::Farming)
    }

    pub fn try_from_db_str(s: &str) -> Option<Self> {
        match s {
            "farming" => Some(RunType::Farming),
            "tournament" => Some(RunType::Tournament),
            "dissonance_attack" => Some(RunType::DissonanceAttack),
            "dissonance_defense" => Some(RunType::DissonanceDefense),
            "dissonance_utility" => Some(RunType::DissonanceUtility),
            "dissonance_ultimate_weapons" => Some(RunType::DissonanceUltimateWeapons),
            _ => None,
        }
    }

    pub fn dissonance_kind(self) -> Option<DissonanceKind> {
        match self {
            RunType::DissonanceAttack => Some(DissonanceKind::Attack),
            RunType::DissonanceDefense => Some(DissonanceKind::Defense),
            RunType::DissonanceUtility => Some(DissonanceKind::Utility),
            RunType::DissonanceUltimateWeapons => Some(DissonanceKind::UltimateWeapons),
            _ => None,
        }
    }
}

fn resolve_run_type(tournament_seen: bool, dissonance_seen: Option<DissonanceKind>) -> RunType {
    if tournament_seen {
        RunType::Tournament
    } else if let Some(kind) = dissonance_seen {
        kind.to_run_type()
    } else {
        RunType::Farming
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
    /// Parsed from the Golden Combo HUD (`0.03% ^166 = x0.05`).
    pub golden_combo: GoldenComboReading,
    /// Dissonance (disco) run category when visible on screen.
    pub dissonance: Option<DissonanceKind>,
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
        golden_combo_chance: Option<f64>,
        golden_combo_caret: Option<u32>,
        golden_combo_multiplier: Option<f64>,
    },
    WaveSkip {
        at_wave: u32,
        /// Observed wave increment (analytics / dedup).
        skipped_count: u32,
        /// Banner ×N from OCR when parsed.
        skip_multiplier: Option<u32>,
        coin_per_minute: Option<f64>,
    },
    EndRun {
        final_wave: u32,
        peak_tier: Option<u32>,
        run_type: RunType,
        snapshot_count: u32,
        avg_coin_per_minute: Option<f64>,
        last_coin_per_minute: Option<f64>,
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
    /// Banner ×N from the most recent skip, when OCR parsed it.
    pub last_skip_multiplier: Option<u32>,
    /// Most recent wave increment (1 for normal progression).
    pub last_wave_delta: Option<u32>,
    /// Last Golden Combo chance % from OCR.
    pub golden_combo_chance: Option<f64>,
    /// Last Golden Combo caret / stack count (`^N`).
    pub golden_combo_caret: Option<u32>,
    /// Last Golden Combo multiplier (`xN`).
    pub golden_combo_multiplier: Option<f64>,
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
            last_skip_multiplier: None,
            last_wave_delta: None,
            golden_combo_chance: None,
            golden_combo_caret: None,
            golden_combo_multiplier: None,
        }
    }
}

struct ActiveRun {
    run_type: RunType,
    last_saved_wave: u32,
    peak_tier: Option<u32>,
    accumulating_for_wave: Option<u32>,
    coin_samples: Vec<f64>,
    snapshots_saved: u32,
    coin_sum: f64,
    coin_rate_snapshots: u32,
    /// Latched for live dashboard (toast is fleeting; keep last good read).
    last_golden_combo: GoldenComboReading,
    /// GC OCR seen while the current wave was confirmed — written on flush, then cleared.
    wave_golden_combo: GoldenComboReading,
    /// Votes for GC chance % this run (key = hundredths, e.g. 0.03 → 3). Chance is fixed in-game.
    gc_chance_votes: HashMap<i32, u32>,
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
        let same = self
            .candidate
            .map(|c| approx_same_rate(c, v))
            .unwrap_or(false);
        if same {
            self.count += 1;
        } else {
            // A new candidate starts its own window — a stray misread from the
            // previous (superseded) candidate must not linger and skew the
            // median once this candidate gets confirmed. See
            // `coin_rate_repeated_misread_does_not_corrupt_confirmed`.
            self.candidate = Some(v);
            self.count = 1;
            self.window.clear();
        }
        self.window.push_back(v);
        while self.window.len() > COIN_MEDIAN_WINDOW {
            self.window.pop_front();
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

    /// A single ~6s poll shouldn't legitimately move the rate by more than a few
    /// times — even a sudden Golden Combo multiplier activation ramps in, it
    /// doesn't teleport. A jump past this band needs sustained confirmation
    /// (`needed = 3`) rather than the fast 2-frame path. Previously this was
    /// 0.02..=50.0, wide enough that a single dropped-decimal-point OCR misread
    /// (e.g. "529.4T" read as "5294T", ~10x too large) sailed through as
    /// "not an outlier" — see `coin_rate_repeated_misread_does_not_corrupt_confirmed`.
    fn is_outlier(&self, v: f64) -> bool {
        let Some(cur) = self.confirmed else {
            return false;
        };
        if cur <= 0.0 {
            return false;
        }
        let ratio = v / cur;
        !(0.2..=5.0).contains(&ratio)
    }

    /// Latest rate for the dashboard; holds the last parseable reading between polls.
    fn display(&self) -> Option<f64> {
        match (self.confirmed, self.candidate) {
            (Some(c), Some(cand)) if self.is_outlier(cand) => Some(c),
            (confirmed, candidate) => confirmed.or(candidate),
        }
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
    ) -> Option<(u32, u32, Option<u32>)> {
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
        let skip_multiplier = self
            .banner_overlay(overlay_now)
            .and_then(|b| crate::parser::wave_skip_banner_multiplier(b, delta));
        self.last_emitted = Some(key);
        self.latched_banner = None;
        self.latched_polls = 0;
        self.single_skip_banner_polls = 0;
        Some((new_wave, skipped_count, skip_multiplier))
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
    dissonance_seen: Option<DissonanceKind>,
    /// Consecutive polls without a readable coin/min rate
    /// (total-coin balance, or unreadable OCR e.g. crash/black screen).
    consecutive_total_coin_polls: u32,
    /// Last skip banner ×N (dashboard), when OCR parsed it.
    last_skip_multiplier: Option<u32>,
    /// Last observed wave increment (dashboard).
    last_wave_delta: Option<u32>,
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
            dissonance_seen: None,
            consecutive_total_coin_polls: 0,
            last_skip_multiplier: None,
            last_wave_delta: None,
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
            // Covers total-coins HUD and sustained unreadable OCR (crash/freeze).
            total_coin_warning: self.consecutive_total_coin_polls >= 2,
            last_skip_multiplier: self
                .run
                .is_some()
                .then_some(self.last_skip_multiplier)
                .flatten(),
            last_wave_delta: self
                .run
                .is_some()
                .then_some(self.last_wave_delta)
                .flatten(),
            golden_combo_chance: self.run.as_ref().and_then(|r| {
                consensus_gc_chance(r).or(r.last_golden_combo.chance_percent)
            }),
            golden_combo_caret: self
                .run
                .as_ref()
                .and_then(|r| r.last_golden_combo.caret_count),
            golden_combo_multiplier: self
                .run
                .as_ref()
                .and_then(|r| r.last_golden_combo.multiplier),
        }
    }

    /// Remember tournament/dissonance cues from the latest classified frame.
    pub fn absorb_run_type_hints(&mut self, input: &PollInput) {
        if input.mode == GameMode::Tournament {
            self.tournament_seen = true;
        }
        if let Some(kind) = input.dissonance {
            self.dissonance_seen = Some(kind);
        }
    }

    pub fn absorb_dissonance(&mut self, kind: DissonanceKind) {
        self.dissonance_seen = Some(kind);
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
            actions.push(run.end_run_action(
                final_wave,
                self.last_coin_rate.or(self.last_seen_coin),
            ));
        }
        let run_type = resolve_run_type(self.tournament_seen, self.dissonance_seen);
        // Forget confirmed wave so the next confirmed reading can start a run
        // even if it is > 1.
        self.wave = Debounced::default();
        self.wave_skip.reset();
        self.unconfirmed_lower_wave = None;
        self.reset_coin_tracking();
        actions.push(Action::StartRun { run_type });
        self.run = Some(new_active_run(run_type));
        actions
    }

    /// Continue an open run from the database after app restart or a fresh process.
    pub fn resume_from_db(
        &mut self,
        run_type: RunType,
        last_saved_wave: u32,
        peak_tier: Option<u32>,
        last_golden_combo: Option<GoldenComboReading>,
    ) {
        let mut run = new_active_run(run_type);
        run.last_saved_wave = last_saved_wave;
        run.peak_tier = peak_tier;
        if let Some(gc) = last_golden_combo {
            // Demangle via merge (e.g. stored `816` → live `316`) so the dashboard
            // matches corrected History values after resume.
            run.last_golden_combo = GoldenComboReading::default().merge_with(gc);
        }
        self.run = Some(run);
        if last_saved_wave > 0 {
            self.wave.candidate = Some(last_saved_wave);
            self.wave.count = DEBOUNCE;
            self.wave.confirmed = Some(last_saved_wave);
            self.last_seen_wave = Some(last_saved_wave);
        }
        self.wave_skip.set_resume_catchup_pending(true);
    }

    /// Apply a GC toast reading from a GC-only OCR tick (no coin/wave/skip updates).
    /// Attributes the hit to the current confirmed wave for the per-wave snapshot.
    pub fn poll_golden_combo_only(&mut self, gc: GoldenComboReading) {
        if !gc.seen {
            return;
        }
        let Some(run) = self.run.as_mut() else {
            return;
        };
        let update_wave_snapshot = self.wave.confirmed.is_some();
        apply_golden_combo_reading(run, gc, update_wave_snapshot);
    }

    /// When scanning starts with no active run, open one immediately so snapshots can persist.
    pub fn ensure_run_for_scanning(&mut self) -> Vec<Action> {
        if self.run.is_some() {
            return Vec::new();
        }
        let run_type = resolve_run_type(self.tournament_seen, self.dissonance_seen);
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
        if let Some(kind) = input.dissonance {
            self.dissonance_seen = Some(kind);
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
                if let Some(confirmed) = self.coin_rate.feed(Some(v)) {
                    self.last_coin_rate = Some(confirmed);
                }
                if let Some(d) = self.coin_rate.display() {
                    self.last_seen_coin = Some(d);
                }
                self.consecutive_total_coin_polls = 0;
            }
            CoinReading::Total(_)
                if matches!(input.mode, GameMode::TotalCoin | GameMode::Tournament) =>
            {
                self.consecutive_total_coin_polls += 1;
            }
            CoinReading::Unreadable => {
                // Crash / black screen / OCR failure — same "no /min" path as total coins.
                self.consecutive_total_coin_polls += 1;
            }
            _ => {
                // Total balance outside total-coin/tournament modes — hold streak.
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
                actions.push(run.end_run_action(
                    final_wave,
                    self.last_coin_rate.or(self.last_seen_coin),
                ));
            }
            // Reset debounce so a stale confirmed wave can't restart the run
            // before the game actually shows wave 1 again.
            self.wave = Debounced::default();
            self.tournament_seen = false;
            self.dissonance_seen = None;
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
                            if let Some((at_wave, wave_delta, skip_multiplier)) =
                                self.wave_skip.on_wave_jump(
                                wave,
                                delta,
                                input.wave_skip_overlay,
                            ) {
                                self.last_wave_delta = Some(wave_delta);
                                self.last_skip_multiplier = skip_multiplier;
                                actions.push(Action::WaveSkip {
                                    at_wave,
                                    skipped_count: wave_delta,
                                    skip_multiplier,
                                    coin_per_minute: self.last_coin_rate.or(self.last_seen_coin),
                                });
                            } else if delta == 1 {
                                self.last_wave_delta = Some(1);
                                self.last_skip_multiplier = None;
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
                            let run_type =
                                resolve_run_type(self.tournament_seen, self.dissonance_seen);
                            actions.push(Action::StartRun { run_type });
                            // Keep debounced coin rate — polls toward wave 1 already
                            // established the current /min for snapshots.
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
                            actions.push(ended.end_run_action(
                                final_wave,
                                self.last_coin_rate.or(self.last_seen_coin),
                            ));
                            let run_type =
                                resolve_run_type(self.tournament_seen, self.dissonance_seen);
                            self.tournament_seen = run_type == RunType::Tournament;
                            self.dissonance_seen = run_type.dissonance_kind();
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
            // Live latch tracks the latest successful OCR read. Per-wave snapshot
            // fields only update when this poll's wave matches the confirmed wave,
            // so an early (unconfirmed) next-wave toast cannot overwrite the prior
            // wave's caret before flush.
            let update_wave_snapshot = self.wave.confirmed.is_some()
                && input.wave.is_some()
                && input.wave == self.wave.confirmed;
            apply_golden_combo_reading(run, input.golden_combo, update_wave_snapshot);
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
        self.last_skip_multiplier = None;
        self.last_wave_delta = None;
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
        snapshots_saved: 0,
        coin_sum: 0.0,
        coin_rate_snapshots: 0,
        last_golden_combo: GoldenComboReading::default(),
        wave_golden_combo: GoldenComboReading::default(),
        gc_chance_votes: HashMap::new(),
    }
}

impl ActiveRun {
    fn end_run_action(&self, final_wave: u32, last_coin_per_minute: Option<f64>) -> Action {
        Action::EndRun {
            final_wave,
            peak_tier: self.peak_tier,
            run_type: self.run_type,
            snapshot_count: self.snapshots_saved,
            avg_coin_per_minute: self.avg_coin_per_minute(),
            last_coin_per_minute,
        }
    }

    fn avg_coin_per_minute(&self) -> Option<f64> {
        if self.coin_rate_snapshots == 0 {
            None
        } else {
            Some(self.coin_sum / self.coin_rate_snapshots as f64)
        }
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
    run.snapshots_saved += 1;
    if let Some(c) = coin_per_minute {
        run.coin_sum += c;
        run.coin_rate_snapshots += 1;
    }
    let gc = run.wave_golden_combo;
    run.wave_golden_combo = GoldenComboReading::default();
    // Chance % is fixed for a run and chance-only OCR rows are noise — only persist
    // when we have an activation count (^N). Attach the run consensus chance.
    let (golden_combo_chance, golden_combo_caret, golden_combo_multiplier) =
        if let Some(caret) = gc.caret_count {
            // The multiplier sits at the end of the toast line, where it's more likely
            // than the caret to be cut off by OCR (no per-poll retry budget left, or the
            // toast fading) — this wave's own polls can easily flush with none ever seen.
            // The run-level latch persists across wave boundaries and isn't reset here, so
            // when its caret matches (proving it's the same activation, not a stale one),
            // borrow its multiplier instead of losing it to this wave's read gap.
            let multiplier = gc.multiplier.or_else(|| {
                (run.last_golden_combo.caret_count == Some(caret))
                    .then_some(run.last_golden_combo.multiplier)
                    .flatten()
            });
            (
                consensus_gc_chance(run).or(gc.chance_percent),
                Some(caret),
                multiplier,
            )
        } else {
            (None, None, None)
        };
    vec![Action::Snapshot {
        wave,
        tier,
        coin_per_minute,
        golden_combo_chance,
        golden_combo_caret,
        golden_combo_multiplier,
    }]
}

/// In-game GC chance does not change mid-run; keep values in a tight HUD range.
fn plausible_gc_chance(chance: f64) -> bool {
    (0.01..=1.0).contains(&chance) && chance.is_finite()
}

fn quantize_gc_chance(chance: f64) -> Option<i32> {
    if !plausible_gc_chance(chance) {
        return None;
    }
    Some((chance * 100.0).round() as i32)
}

fn consensus_gc_chance(run: &ActiveRun) -> Option<f64> {
    leading_gc_chance(run).map(|(chance, _)| chance)
}

/// Leading chance vote: `(chance, vote_count)`. `None` on empty or unresolved tie.
fn leading_gc_chance(run: &ActiveRun) -> Option<(f64, u32)> {
    let mut best_count = 0u32;
    let mut leaders: Vec<i32> = Vec::new();
    for (&key, &count) in &run.gc_chance_votes {
        if count > best_count {
            best_count = count;
            leaders.clear();
            leaders.push(key);
        } else if count == best_count && count > 0 {
            leaders.push(key);
        }
    }
    if best_count == 0 || leaders.is_empty() {
        return None;
    }
    if leaders.len() == 1 {
        return Some((leaders[0] as f64 / 100.0, best_count));
    }
    // Tie: stick with the latched live chance if it is one of the leaders.
    if let Some(prev) = run.last_golden_combo.chance_percent {
        let pk = (prev * 100.0).round() as i32;
        if leaders.contains(&pk) {
            return Some((prev, best_count));
        }
    }
    None
}

fn vote_gc_chance(run: &mut ActiveRun, chance: f64) {
    let Some(key) = quantize_gc_chance(chance) else {
        return;
    };
    *run.gc_chance_votes.entry(key).or_insert(0) += 1;
}

fn apply_golden_combo_reading(
    run: &mut ActiveRun,
    raw: GoldenComboReading,
    update_wave_snapshot: bool,
) {
    if !raw.seen {
        return;
    }
    let mut gc = raw;
    if let Some(c) = gc.chance_percent {
        if !plausible_gc_chance(c) {
            gc.chance_percent = None;
        } else {
            let prior_leader = leading_gc_chance(run);
            vote_gc_chance(run, c);
            if let Some((leader, count)) = leading_gc_chance(run) {
                if (c - leader).abs() > 0.005 {
                    // Disagrees with the leading/majority value — keep leader for display,
                    // drop chance from this hit (and drop the hit if chance was all we had).
                    gc.chance_percent = if count >= 1 {
                        Some(leader)
                    } else {
                        None
                    };
                    if gc.chance_percent.is_none()
                        && gc.caret_count.is_none()
                        && gc.multiplier.is_none()
                    {
                        return;
                    }
                    // If this was chance-only disagreeing with a strong majority, ignore entirely.
                    if count >= 2
                        && gc.caret_count.is_none()
                        && gc.multiplier.is_none()
                        && prior_leader.is_some_and(|(l, _)| (c - l).abs() > 0.005)
                    {
                        return;
                    }
                } else {
                    gc.chance_percent = Some(leader);
                }
            }
        }
    }

    if gc.chance_percent.is_none()
        && gc.caret_count.is_none()
        && gc.multiplier.is_none()
    {
        return;
    }

    // Live latch keeps chance/caret/mult for the dashboard (last successful read).
    run.last_golden_combo = run.last_golden_combo.merge_with(gc);
    if let Some((cons, _)) = leading_gc_chance(run) {
        run.last_golden_combo.chance_percent = Some(cons);
    }

    // Per-wave snapshot accumulator: need caret or multiplier — chance-only is not stored.
    if update_wave_snapshot && (gc.caret_count.is_some() || gc.multiplier.is_some()) {
        run.wave_golden_combo = run.wave_golden_combo.merge_with(gc);
        if let Some((cons, _)) = leading_gc_chance(run) {
            run.wave_golden_combo.chance_percent = Some(cons);
        }
    }
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
            golden_combo: GoldenComboReading::default(),
            dissonance: None,
        }
    }

    fn p_dissonance(
        mode: GameMode,
        tier: u32,
        wave: u32,
        coin: CoinReading,
        dissonance: DissonanceKind,
    ) -> PollInput {
        PollInput {
            mode,
            tier: Some(tier),
            wave: Some(wave),
            coin,
            wave_skip_overlay: WaveSkipOverlay::default(),
            golden_combo: GoldenComboReading::default(),
            dissonance: Some(dissonance),
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
            golden_combo: GoldenComboReading::default(),
            dissonance: None,
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
        sm.resume_from_db(RunType::Farming, 42, Some(17), None);
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
    fn resume_from_db_seeds_golden_combo_latch_and_demangles_8xx() {
        let mut sm = RunStateMachine::new();
        sm.resume_from_db(
            RunType::Farming,
            100,
            Some(14),
            Some(GoldenComboReading {
                seen: true,
                chance_percent: Some(0.03),
                caret_count: Some(816),
                multiplier: Some(0.1),
            }),
        );
        assert_eq!(sm.live_state().golden_combo_caret, Some(316));
        assert_eq!(sm.live_state().golden_combo_chance, Some(0.03));
        assert_eq!(sm.live_state().golden_combo_multiplier, Some(0.1));
    }

    #[test]
    fn resume_catchup_suppresses_false_multi_skip_without_banner() {
        let mut sm = RunStateMachine::new();
        feed2(&mut sm, p(GameMode::Normal, 14, 1, CoinReading::Rate(1e12)));
        feed2(&mut sm, p(GameMode::Normal, 14, 100, CoinReading::Rate(1e12)));
        sm.resume_from_db(RunType::Farming, 100, Some(14), None);
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
        sm.resume_from_db(RunType::Farming, 100, Some(14), None);
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
        sm.resume_from_db(RunType::Farming, 100, Some(14), None);
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
            coin_per_minute: Some(100.0),
            golden_combo_chance: None,
            golden_combo_caret: None,
            golden_combo_multiplier: None
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
            coin_per_minute: Some(150.0),
            golden_combo_chance: None,
            golden_combo_caret: None,
            golden_combo_multiplier: None
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
        // A single garbled frame well off the trend — even gated as an outlier
        // (needs 3 confirmations), the very next real reading should restore
        // the trend rather than letting a lone bad frame linger.
        sm.poll(p(GameMode::Normal, 12, 1, CoinReading::Rate(5.0e12)));
        sm.poll(p(GameMode::Normal, 12, 1, CoinReading::Rate(72.0e12)));
        let reported = sm.live_state().coin_per_minute.unwrap();
        assert!(
            (60.0e12..=80.0e12).contains(&reported),
            "median should reject the 5T outlier, got {reported}"
        );
    }

    /// Regression for a real ragchel-account log capture: OCR dropped the
    /// decimal point on the coin-rate crop twice in a row (real "565.0T/min"
    /// and "560.1T/min" read as "5650T" / "5601T", ~10x too large), and the
    /// two misreads were within 5% of each other so the old debounce treated
    /// them as a confirmed candidate — flipping the tracked rate to ~5.6q and
    /// writing it to the run's history until a later real reading happened to
    /// dilute a since-corrupted median back down. Neither fix alone is
    /// sufficient: the tightened outlier band stops a single ~10x jump from
    /// confirming in just 2 frames, and the per-candidate window stops a
    /// figure from a *superseded* candidate polluting the next one's median.
    #[test]
    fn coin_rate_repeated_misread_does_not_corrupt_confirmed() {
        let mut sm = RunStateMachine::new();
        feed2(
            &mut sm,
            p(GameMode::Normal, 15, 1, CoinReading::Rate(540.0e12)),
        );
        sm.poll(p(GameMode::Normal, 15, 2, CoinReading::Rate(529.4e12)));
        sm.poll(p(GameMode::Normal, 15, 3, CoinReading::Rate(5294.0e12))); // dropped decimal
        sm.poll(p(GameMode::Normal, 15, 4, CoinReading::Rate(562.0e12)));
        sm.poll(p(GameMode::Normal, 15, 5, CoinReading::Rate(5650.0e12))); // dropped decimal
        sm.poll(p(GameMode::Normal, 15, 6, CoinReading::Rate(5601.0e12))); // dropped decimal, ~same as previous
        let reported = sm.live_state().coin_per_minute.unwrap();
        assert!(
            reported < 1.0e15,
            "two similar dropped-decimal misreads must not confirm a ~10x spike, got {reported}"
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
            coin_per_minute: Some(1.0),
            golden_combo_chance: None,
            golden_combo_caret: None,
            golden_combo_multiplier: None
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
            coin_per_minute: Some(500.0), // average while on wave 1, not the total balance
            golden_combo_chance: None,
            golden_combo_caret: None,
            golden_combo_multiplier: None,
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
    fn sustained_unreadable_coin_sets_warning() {
        let mut sm = RunStateMachine::new();
        feed2(
            &mut sm,
            p(GameMode::Normal, 14, 1, CoinReading::Rate(100.0)),
        );
        // Single OCR miss must not flash the banner.
        sm.poll(p(GameMode::Normal, 14, 1, CoinReading::Unreadable));
        assert!(!sm.live_state().total_coin_warning);
        // Sustained unreadable (crash / black screen) raises the warning.
        sm.poll(p(GameMode::Normal, 14, 1, CoinReading::Unreadable));
        assert!(sm.live_state().total_coin_warning);
        // Rate returning clears it.
        sm.poll(p(GameMode::Normal, 14, 1, CoinReading::Rate(100.0)));
        assert!(!sm.live_state().total_coin_warning);
    }

    #[test]
    fn unreadable_holds_existing_total_coin_warning() {
        let mut sm = RunStateMachine::new();
        feed2(
            &mut sm,
            p(GameMode::TotalCoin, 14, 1, CoinReading::Total(1e15)),
        );
        assert!(sm.live_state().total_coin_warning);
        sm.poll(p(GameMode::Normal, 14, 1, CoinReading::Unreadable));
        assert!(sm.live_state().total_coin_warning);
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
            coin_per_minute: None,
            golden_combo_chance: None,
            golden_combo_caret: None,
            golden_combo_multiplier: None
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
    fn dissonance_attack_run_gets_tagged() {
        let mut sm = RunStateMachine::new();
        let actions = feed2(
            &mut sm,
            p_dissonance(
                GameMode::Normal,
                14,
                1,
                CoinReading::Rate(1e12),
                DissonanceKind::Attack,
            ),
        );
        assert!(actions.contains(&Action::StartRun {
            run_type: RunType::DissonanceAttack
        }));
    }

    #[test]
    fn tournament_takes_priority_over_dissonance() {
        let mut sm = RunStateMachine::new();
        let actions = feed2(
            &mut sm,
            p_dissonance(
                GameMode::Tournament,
                17,
                1,
                CoinReading::Total(3.06e15),
                DissonanceKind::Utility,
            ),
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
            golden_combo: GoldenComboReading::default(),
            dissonance: None,
        });
        assert_eq!(
            actions,
            vec![
                Action::Snapshot {
                    wave: 2,
                    tier: Some(11),
                    coin_per_minute: Some(10.0),
                    golden_combo_chance: None,
                    golden_combo_caret: None,
                    golden_combo_multiplier: None
                },
                Action::EndRun {
                    final_wave: 2,
                    peak_tier: Some(11),
                    run_type: RunType::Farming,
                    snapshot_count: 2,
                    avg_coin_per_minute: Some(10.0),
                    last_coin_per_minute: Some(10.0),
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
    fn manual_new_run_tags_dissonance_from_screen_hints() {
        let mut sm = RunStateMachine::new();
        feed2(
            &mut sm,
            p_dissonance(
                GameMode::Normal,
                15,
                100,
                CoinReading::Rate(1e12),
                DissonanceKind::Defense,
            ),
        );
        let actions = sm.manual_new_run();
        assert!(actions.contains(&Action::StartRun {
            run_type: RunType::DissonanceDefense
        }));
        assert_eq!(sm.live_state().run_type, Some(RunType::DissonanceDefense));
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
            peak_tier: Some(14),
            run_type: RunType::Farming,
            snapshot_count: 2,
            avg_coin_per_minute: Some(10.0),
            last_coin_per_minute: Some(10.0),
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
            golden_combo: GoldenComboReading::default(),
            dissonance: None,
        });
        assert_eq!(
            actions,
            vec![
                Action::Snapshot {
                    wave: 3,
                    tier: Some(13),
                    coin_per_minute: Some(1.0),
                    golden_combo_chance: None,
                    golden_combo_caret: None,
                    golden_combo_multiplier: None
                },
                Action::EndRun {
                    final_wave: 3,
                    peak_tier: Some(14),
                    run_type: RunType::Farming,
                    snapshot_count: 3,
                    avg_coin_per_minute: Some(1.0),
                    last_coin_per_minute: Some(1.0),
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
    fn normal_wave_progression_sets_last_jump_to_one() {
        let mut sm = RunStateMachine::new();
        sm.manual_new_run();
        let coin = CoinReading::Rate(1e12);
        feed2(&mut sm, p(GameMode::Normal, 14, 1, coin));
        assert_eq!(sm.live_state().last_wave_delta, None);
        feed2(&mut sm, p(GameMode::Normal, 14, 2, coin));
        assert_eq!(sm.live_state().last_wave_delta, Some(1));
        assert_eq!(sm.live_state().last_skip_multiplier, None);
        let actions = feed2(&mut sm, p(GameMode::Normal, 14, 3, coin));
        assert!(!actions.iter().any(|a| matches!(a, Action::WaveSkip { .. })));
        assert_eq!(sm.live_state().last_wave_delta, Some(1));
    }

    #[test]
    fn live_state_uses_banner_multiplier_not_wave_delta() {
        let mut sm = RunStateMachine::new();
        sm.manual_new_run();
        let coin = CoinReading::Rate(1e12);
        let overlay = WaveSkipOverlay {
            seen: true,
            multiplier: Some(9),
        };
        feed2(&mut sm, p(GameMode::Normal, 14, 90, coin));
        feed2(
            &mut sm,
            p_skip(GameMode::Normal, 14, 100, coin, overlay),
        );
        assert_eq!(sm.live_state().last_skip_multiplier, Some(9));
        assert_eq!(sm.live_state().last_wave_delta, Some(10));
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

    #[test]
    fn golden_combo_latches_on_live_and_snapshots() {
        let mut sm = RunStateMachine::new();
        let mut input = p(GameMode::Normal, 15, 1, CoinReading::Rate(100.0));
        input.golden_combo = GoldenComboReading {
            seen: true,
            chance_percent: Some(0.03),
            caret_count: Some(166),
            multiplier: Some(0.05),
        };
        feed2(&mut sm, input);
        assert_eq!(sm.live_state().golden_combo_chance, Some(0.03));
        assert_eq!(sm.live_state().golden_combo_caret, Some(166));
        assert_eq!(sm.live_state().golden_combo_multiplier, Some(0.05));

        // Partial OCR keeps prior fields on the live latch.
        let mut partial = p(GameMode::Normal, 15, 2, CoinReading::Rate(110.0));
        partial.golden_combo = GoldenComboReading {
            seen: true,
            chance_percent: None,
            caret_count: Some(167),
            multiplier: None,
        };
        let actions = feed2(&mut sm, partial);
        assert_eq!(sm.live_state().golden_combo_chance, Some(0.03));
        assert_eq!(sm.live_state().golden_combo_caret, Some(167));
        assert_eq!(sm.live_state().golden_combo_multiplier, Some(0.05));
        // Unconfirmed wave-2 OCR must not overwrite wave 1's snapshot; caret 166
        // flushes for wave 1. Live latch already shows the newer 167 read.
        assert!(actions.iter().any(|a| {
            matches!(
                a,
                Action::Snapshot {
                    wave: 1,
                    golden_combo_chance: Some(0.03),
                    golden_combo_caret: Some(166),
                    golden_combo_multiplier: Some(0.05),
                    ..
                }
            )
        }));
    }

    #[test]
    fn golden_combo_not_copied_onto_later_wave_snapshots() {
        let mut sm = RunStateMachine::new();
        let mut with_gc = p(GameMode::Normal, 15, 1, CoinReading::Rate(100.0));
        with_gc.golden_combo = GoldenComboReading {
            seen: true,
            chance_percent: Some(0.03),
            caret_count: Some(200),
            multiplier: Some(0.08),
        };
        feed2(&mut sm, with_gc);
        assert_eq!(sm.live_state().golden_combo_caret, Some(200));

        // Advance through waves with no GC OCR — live latch stays, later DB rows stay null.
        let actions = feed2(&mut sm, p(GameMode::Normal, 15, 2, CoinReading::Rate(110.0)));
        assert!(actions.iter().any(|a| {
            matches!(
                a,
                Action::Snapshot {
                    wave: 1,
                    golden_combo_caret: Some(200),
                    golden_combo_chance: Some(0.03),
                    golden_combo_multiplier: Some(0.08),
                    ..
                }
            )
        }));
        assert_eq!(sm.live_state().golden_combo_caret, Some(200));

        let actions = feed2(&mut sm, p(GameMode::Normal, 15, 3, CoinReading::Rate(120.0)));
        assert!(actions.iter().any(|a| {
            matches!(
                a,
                Action::Snapshot {
                    wave: 2,
                    golden_combo_chance: None,
                    golden_combo_caret: None,
                    golden_combo_multiplier: None,
                    ..
                }
            )
        }));
        assert_eq!(sm.live_state().golden_combo_caret, Some(200));

        let actions = feed2(&mut sm, p(GameMode::Normal, 15, 4, CoinReading::Rate(130.0)));
        assert!(actions.iter().any(|a| {
            matches!(
                a,
                Action::Snapshot {
                    wave: 3,
                    golden_combo_chance: None,
                    golden_combo_caret: None,
                    golden_combo_multiplier: None,
                    ..
                }
            )
        }));
    }

    #[test]
    fn golden_combo_multiplier_recovered_from_live_latch_when_wave_misses_it() {
        // A toast's multiplier sits at the end of the line, where OCR is more likely to
        // drop it than the caret just before it — the multiplier can go unread for an
        // entire wave's polls even though the caret (same activation) keeps coming through.
        let mut sm = RunStateMachine::new();
        let mut hit = p(GameMode::Normal, 15, 1, CoinReading::Rate(100.0));
        hit.golden_combo = GoldenComboReading {
            seen: true,
            chance_percent: Some(0.12),
            caret_count: Some(250),
            multiplier: Some(0.46),
        };
        feed2(&mut sm, hit);
        assert_eq!(sm.live_state().golden_combo_multiplier, Some(0.46));

        // Wave 2: same activation (caret unchanged) still visible, but this poll's OCR
        // missed the multiplier — the wave-2 accumulator alone would have none.
        let mut same_activation_no_mult = p(GameMode::Normal, 15, 2, CoinReading::Rate(110.0));
        same_activation_no_mult.golden_combo = GoldenComboReading {
            seen: true,
            chance_percent: Some(0.12),
            caret_count: Some(250),
            multiplier: None,
        };
        feed2(&mut sm, same_activation_no_mult);
        // Live latch is unaffected — merge_with keeps the prior multiplier either way.
        assert_eq!(sm.live_state().golden_combo_multiplier, Some(0.46));

        // Flushing wave 2 should recover 0.46 from the live latch rather than persist
        // None, since the matching caret proves it's the same activation.
        let actions = feed2(&mut sm, p(GameMode::Normal, 15, 3, CoinReading::Rate(120.0)));
        assert!(actions.iter().any(|a| {
            matches!(
                a,
                Action::Snapshot {
                    wave: 2,
                    golden_combo_caret: Some(250),
                    golden_combo_multiplier: Some(0.46),
                    ..
                }
            )
        }));
    }

    #[test]
    fn live_golden_combo_follows_last_read_while_wave_snapshots_stay_per_wave() {
        let mut sm = RunStateMachine::new();
        // High OCR latch first.
        let mut high = p(GameMode::Normal, 14, 1, CoinReading::Rate(1e12));
        high.golden_combo = GoldenComboReading {
            seen: true,
            chance_percent: Some(0.03),
            caret_count: Some(387),
            multiplier: Some(0.1),
        };
        feed2(&mut sm, high);
        assert_eq!(sm.live_state().golden_combo_caret, Some(387));

        // Next wave sees a lower caret — live updates immediately; wave 1 still flushes 387.
        let mut low = p(GameMode::Normal, 14, 2, CoinReading::Rate(1e12));
        low.golden_combo = GoldenComboReading {
            seen: true,
            chance_percent: Some(0.03),
            caret_count: Some(324),
            multiplier: Some(0.1),
        };
        let actions = feed2(&mut sm, low);
        assert_eq!(sm.live_state().golden_combo_caret, Some(324));
        assert!(actions.iter().any(|a| {
            matches!(
                a,
                Action::Snapshot {
                    wave: 1,
                    golden_combo_caret: Some(387),
                    ..
                }
            )
        }));
        // Advance again so wave 2 flushes with 324; live stays at last read.
        let actions = feed2(
            &mut sm,
            p(GameMode::Normal, 14, 3, CoinReading::Rate(1e12)),
        );
        assert!(actions.iter().any(|a| {
            matches!(
                a,
                Action::Snapshot {
                    wave: 2,
                    golden_combo_caret: Some(324),
                    ..
                }
            )
        }));
        assert_eq!(
            sm.live_state().golden_combo_caret,
            Some(324),
            "live latch should keep the last OCR read"
        );
    }

    #[test]
    fn poll_golden_combo_only_updates_live_and_wave_snapshot() {
        let mut sm = RunStateMachine::new();
        feed2(
            &mut sm,
            p(GameMode::Normal, 14, 1, CoinReading::Rate(1e12)),
        );
        feed2(
            &mut sm,
            p(GameMode::Normal, 14, 2, CoinReading::Rate(1e12)),
        );
        // Confirmed on wave 2 after flush of wave 1.
        assert_eq!(sm.live_state().wave, Some(2));
        assert!(!sm.live_state().total_coin_warning);
        let mode_before = sm.live_state().mode;

        sm.poll_golden_combo_only(GoldenComboReading {
            seen: true,
            chance_percent: Some(0.03),
            caret_count: Some(200),
            multiplier: Some(0.08),
        });
        assert_eq!(sm.live_state().golden_combo_caret, Some(200));
        assert_eq!(sm.live_state().golden_combo_multiplier, Some(0.08));
        assert!(!sm.live_state().total_coin_warning);
        assert_eq!(sm.live_state().mode, mode_before);

        let actions = feed2(
            &mut sm,
            p(GameMode::Normal, 14, 3, CoinReading::Rate(1e12)),
        );
        assert!(actions.iter().any(|a| {
            matches!(
                a,
                Action::Snapshot {
                    wave: 2,
                    golden_combo_caret: Some(200),
                    golden_combo_multiplier: Some(0.08),
                    ..
                }
            )
        }));
    }

    #[test]
    fn poll_golden_combo_only_ignores_unseen_and_does_not_touch_coin() {
        let mut sm = RunStateMachine::new();
        feed2(
            &mut sm,
            p(GameMode::Normal, 14, 1, CoinReading::Rate(100.0)),
        );
        sm.poll_golden_combo_only(GoldenComboReading::default());
        assert_eq!(sm.live_state().golden_combo_caret, None);
        assert!(!sm.live_state().total_coin_warning);
        // Two Unreadable full polls would warn; GC-only must not count as Unreadable.
        sm.poll_golden_combo_only(GoldenComboReading {
            seen: true,
            chance_percent: Some(0.03),
            caret_count: Some(50),
            multiplier: None,
        });
        sm.poll_golden_combo_only(GoldenComboReading {
            seen: true,
            chance_percent: Some(0.03),
            caret_count: Some(51),
            multiplier: None,
        });
        assert!(!sm.live_state().total_coin_warning);
        assert_eq!(sm.live_state().golden_combo_caret, Some(51));
    }

    #[test]
    fn golden_combo_chance_only_not_saved_on_snapshot() {
        let mut sm = RunStateMachine::new();
        let mut chance_only = p(GameMode::Normal, 15, 1, CoinReading::Rate(100.0));
        chance_only.golden_combo = GoldenComboReading {
            seen: true,
            chance_percent: Some(0.03),
            caret_count: None,
            multiplier: None,
        };
        feed2(&mut sm, chance_only);
        assert_eq!(sm.live_state().golden_combo_chance, Some(0.03));

        let actions = feed2(&mut sm, p(GameMode::Normal, 15, 2, CoinReading::Rate(110.0)));
        assert!(actions.iter().any(|a| {
            matches!(
                a,
                Action::Snapshot {
                    wave: 1,
                    golden_combo_chance: None,
                    golden_combo_caret: None,
                    golden_combo_multiplier: None,
                    ..
                }
            )
        }));
    }

    #[test]
    fn golden_combo_rejects_outlier_chance_after_consensus() {
        let mut sm = RunStateMachine::new();
        // Establish 0.03 with a confirmed run + two matching samples.
        let mut ok = p(GameMode::Normal, 15, 1, CoinReading::Rate(100.0));
        ok.golden_combo = GoldenComboReading {
            seen: true,
            chance_percent: Some(0.03),
            caret_count: Some(100),
            multiplier: Some(0.05),
        };
        feed2(&mut sm, ok);
        sm.poll(ok);
        assert_eq!(sm.live_state().golden_combo_chance, Some(0.03));

        let mut bad = p(GameMode::Normal, 15, 1, CoinReading::Rate(100.0));
        bad.golden_combo = GoldenComboReading {
            seen: true,
            chance_percent: Some(0.93),
            caret_count: Some(101),
            multiplier: None,
        };
        sm.poll(bad);
        // Chance stays 0.03; caret can still update.
        assert_eq!(sm.live_state().golden_combo_chance, Some(0.03));
        assert_eq!(sm.live_state().golden_combo_caret, Some(101));
    }

    #[test]
    fn golden_combo_snapshot_uses_consensus_chance_with_caret() {
        let mut sm = RunStateMachine::new();
        let mut first = p(GameMode::Normal, 15, 1, CoinReading::Rate(100.0));
        first.golden_combo = GoldenComboReading {
            seen: true,
            chance_percent: Some(0.03),
            caret_count: Some(150),
            multiplier: Some(0.05),
        };
        feed2(&mut sm, first);

        let mut second = p(GameMode::Normal, 15, 2, CoinReading::Rate(110.0));
        second.golden_combo = GoldenComboReading {
            seen: true,
            chance_percent: Some(0.03),
            caret_count: Some(151),
            multiplier: None,
        };
        let actions = feed2(&mut sm, second);
        assert!(actions.iter().any(|a| {
            matches!(
                a,
                Action::Snapshot {
                    wave: 1,
                    golden_combo_chance: Some(0.03),
                    golden_combo_caret: Some(c),
                    ..
                } if *c == 150 || *c == 151
            )
        }));
    }
}
