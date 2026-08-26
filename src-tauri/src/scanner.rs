//! Scanner thread: capture -> OCR -> classify -> state machine -> DB + events.

use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

use crate::parser::GoldenComboReading;
use crate::notifications::NotifyFrameContext;
use crate::state_machine::{Action, LiveState, RunStateMachine, RunType};
use crate::{capture, db, fields, ocr, settings};

/// GC-only band ticks between each full HUD poll (capture → yellow toast, no full-frame OCR).
const GC_ONLY_TICKS_BETWEEN_FULL: u32 = 2;

/// Pre-OCR downscale target width for ADB-captured frames — tighter than
/// [`ocr::DEFAULT_OCR_MAX_WIDTH`] (900) to cut through a real device's extra FX/AA
/// detail. See the comment where this is selected, in the scanner loop setup.
const ADB_OCR_MAX_WIDTH: u32 = 640;

/// GC-only jobs between each full job in the ADB pipeline — same trade the window path
/// makes (see `GC_ONLY_TICKS_BETWEEN_FULL`), now viable on ADB because capture no longer
/// waits on OCR to decide the next job's timing: GC is sampled every job regardless, so
/// this only throttles how often the slower-changing wave/tier/coin fields refresh,
/// trading CPU for a longer wave-change debounce window.
const ADB_GC_ONLY_TICKS_BETWEEN_FULL: u32 = GC_ONLY_TICKS_BETWEEN_FULL;

/// Concurrent OCR workers for the ADB capture pipeline. Full-frame OCR on a downscaled
/// ADB frame costs ~1-1.9s; at a 1s capture cadence, Little's Law puts steady-state
/// demand at ~1-2 workers in flight, so this leaves real headroom for the slower ticks.
const ADB_OCR_WORKERS: usize = 4;

/// Bounded capacity of the capture→OCR job queue. Bounded (not unbounded) so a genuinely
/// wedged worker pool throttles the capture thread instead of buffering full-resolution
/// frames (~10MB each) without limit.
const ADB_PIPELINE_QUEUE_DEPTH: usize = 6;

/// How long to wait for one window capture before giving up on it. Normal captures take
/// ~50ms; the call occasionally stops returning at all, which used to wedge the scanner.
const CAPTURE_TIMEOUT: Duration = Duration::from_secs(5);

/// A poll step taking longer than this means the scanner is wedged, not just slow: a
/// whole poll normally costs ~200ms.
const STAGE_STALL_AFTER: Duration = Duration::from_secs(10);
/// How often the watchdog restates an ongoing stall.
const STAGE_STALL_REPEAT: Duration = Duration::from_secs(30);
/// Watchdog poll interval.
const WATCHDOG_TICK: Duration = Duration::from_secs(2);

/// Where the poll loop currently is. Published so a watchdog on another thread can name
/// the step the scanner is wedged in — a step that never returns logs nothing at all,
/// which is indistinguishable in a log from the app not running.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
enum Stage {
    Sleeping = 0,
    Capturing = 1,
    OcrFull = 2,
    OcrGoldenCombo = 3,
    StateMachine = 4,
    Persisting = 5,
    Notifying = 6,
    Emitting = 7,
}

impl Stage {
    fn from_u8(v: u8) -> Stage {
        match v {
            1 => Stage::Capturing,
            2 => Stage::OcrFull,
            3 => Stage::OcrGoldenCombo,
            4 => Stage::StateMachine,
            5 => Stage::Persisting,
            6 => Stage::Notifying,
            7 => Stage::Emitting,
            _ => Stage::Sleeping,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Stage::Sleeping => "sleeping",
            Stage::Capturing => "capturing",
            Stage::OcrFull => "ocr_full_frame",
            Stage::OcrGoldenCombo => "ocr_golden_combo",
            Stage::StateMachine => "state_machine",
            Stage::Persisting => "writing_to_database",
            Stage::Notifying => "notifications",
            Stage::Emitting => "emitting_ui_event",
        }
    }
}

/// Shared, lock-free view of the poll loop's current step and when it started.
struct StageTracker {
    stage: AtomicU8,
    /// Milliseconds since `epoch`, so the watchdog never has to take a lock that the
    /// wedged thread might be holding.
    entered_ms: AtomicU64,
    epoch: Instant,
}

impl StageTracker {
    fn new() -> Self {
        Self {
            stage: AtomicU8::new(Stage::Sleeping as u8),
            entered_ms: AtomicU64::new(0),
            epoch: Instant::now(),
        }
    }

    fn since_epoch(&self) -> u64 {
        self.epoch.elapsed().as_millis() as u64
    }

    fn enter(&self, stage: Stage) {
        self.entered_ms.store(self.since_epoch(), Ordering::SeqCst);
        self.stage.store(stage as u8, Ordering::SeqCst);
    }

    /// Current step and how long it has been running.
    fn current(&self) -> (Stage, Duration) {
        let stage = Stage::from_u8(self.stage.load(Ordering::SeqCst));
        let entered = self.entered_ms.load(Ordering::SeqCst);
        let elapsed = self.since_epoch().saturating_sub(entered);
        (stage, Duration::from_millis(elapsed))
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScanStartMode {
    NewRun,
    ResumePrevious,
}

#[derive(Clone, Serialize)]
pub struct ScannerEvent {
    pub status: String, // scanning | window_not_found | ocr_error | stopped
    pub live: Option<LiveState>,
    pub current_run_id: Option<String>,
}

pub struct Scanner {
    running: Arc<AtomicBool>,
    pub machine: Arc<Mutex<RunStateMachine>>,
    pub current_run_id: Arc<Mutex<Option<String>>>,
    app: Arc<Mutex<Option<AppHandle>>>,
    /// Last emitted live state — UI reads this without waiting on the scanner mutex.
    cached_live: Arc<Mutex<LiveState>>,
}

impl Default for Scanner {
    fn default() -> Self {
        Self {
            running: Arc::new(AtomicBool::new(false)),
            machine: Arc::new(Mutex::new(RunStateMachine::new())),
            current_run_id: Arc::new(Mutex::new(None)),
            app: Arc::new(Mutex::new(None)),
            cached_live: Arc::new(Mutex::new(LiveState::idle())),
        }
    }
}

impl Scanner {
    pub fn cached_live_state(&self) -> LiveState {
        self.cached_live.lock().unwrap().clone()
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
        if let Ok(guard) = self.app.lock() {
            if let Some(app) = guard.as_ref() {
                emit(
                    app,
                    "stopped",
                    &self.machine,
                    &self.current_run_id,
                    &self.cached_live,
                );
            }
        }
    }

    /// Clear in-memory scan state after the database file was replaced.
    pub fn reset_after_db_restore(&self) {
        *self.machine.lock().unwrap() = RunStateMachine::new();
        *self.current_run_id.lock().unwrap() = None;
        *self.cached_live.lock().unwrap() = LiveState::idle();
    }

    pub fn has_resumable_run(&self) -> Result<bool, String> {
        if self.machine.lock().unwrap().has_active_run() {
            return Ok(true);
        }
        let conn = db::open().map_err(|e| e.to_string())?;
        Ok(db::latest_open_run(&conn)
            .map_err(|e| e.to_string())?
            .is_some())
    }

    pub fn start(&self, app: AppHandle, mode: ScanStartMode) -> Result<(), String> {
        if self.is_running() {
            return Ok(());
        }
        let conn = db::open().map_err(|e| e.to_string())?;
        let cfg = settings::load(&conn);
        let target = settings::resolve_capture_target(&conn)?;

        self.running.store(true, Ordering::SeqCst);
        if let Ok(mut guard) = self.app.lock() {
            *guard = Some(app.clone());
        }

        let log_path = db::app_data_dir().join("logs");
        std::fs::create_dir_all(&log_path).ok();
        let (start_actions, resume_wave) = match mode {
            ScanStartMode::NewRun => (
                new_run_actions(&mut self.machine.lock().unwrap(), &target),
                None,
            ),
            ScanStartMode::ResumePrevious => {
                let wave = self.prepare_resume(&conn)?;
                (Vec::new(), wave)
            }
        };
        if let Some(wave) = resume_wave {
            if let Some(notify) = app.try_state::<crate::notifications::NotifyState>() {
                notify.seed_milestone_from_wave(wave, cfg.notify_wave_every);
            }
        }
        if !start_actions.is_empty() {
            apply_actions(&conn, &self.current_run_id, &start_actions, &log_path);
            notify_scanner_actions(&app, &start_actions, None, NotifyFrameContext::default());
        }

        let running = self.running.clone();
        let machine = self.machine.clone();
        let current_run_id = self.current_run_id.clone();
        let app_slot = self.app.clone();
        let cached_live = self.cached_live.clone();

        let stages = Arc::new(StageTracker::new());
        spawn_stall_watchdog(stages.clone(), running.clone());

        std::thread::Builder::new()
            .name("wavetrace-scanner".into())
            .spawn(move || {
                let log_path = db::app_data_dir().join("logs");
                std::fs::create_dir_all(&log_path).ok();
                let _exit = ScannerExitGuard {
                    running: running.clone(),
                    app: app.clone(),
                    machine: machine.clone(),
                    current_run_id: current_run_id.clone(),
                    cached_live: cached_live.clone(),
                    app_slot,
                };
                emit(&app, "starting", &machine, &current_run_id, &cached_live);

                let mut exit_y_cache: Option<f32> = None;
                let mut gc_only_remaining: u32 = 0;
                let mut outage = CaptureOutage::default();
                let mut capturer = capture::TimeboxedCapture::new();
                // Skipping full-frame OCR on GC-only ticks saves time when a capture itself
                // is cheap (~200ms on the window path). Over ADB, the capture round trip
                // alone runs ~1.5s regardless of tick kind, so the skip buys little — while
                // tripling the gap between wave readings, which starves the 2-consecutive-
                // poll debounce that confirms a wave. Always poll full there instead.
                let gc_only_ticks_between_full = match &target {
                    capture::CaptureTarget::AdbPhone { .. } => 0,
                    capture::CaptureTarget::Window(_) => GC_ONLY_TICKS_BETWEEN_FULL,
                };
                // Full-frame OCR cost tracks how visually loaded the game state is (deep
                // wave, stacked upgrade panels/FX) far more than it tracks resolution or
                // capture source — a side-by-side of window and ADB against the *same*
                // emulator showed near-identical full-frame OCR cost despite ADB's frame
                // being downscaled to a meaningfully smaller width. What downscaling still
                // buys, confirmed by re-processing the same real captured frame at several
                // widths, is a real ~20-25% speedup acting as a low-pass filter on FX
                // noise — worth it on ADB specifically since its per-tick budget is tighter
                // (no gc-only tick "coasts" through the OCR cost the way window's do).
                let ocr_max_width = match &target {
                    capture::CaptureTarget::AdbPhone { .. } => ADB_OCR_MAX_WIDTH,
                    capture::CaptureTarget::Window(_) => ocr::DEFAULT_OCR_MAX_WIDTH,
                };
                // GC "too busy to be a toast" ink ceiling — one shared value for both
                // sources. An earlier ADB-only override here turned out to be solving the
                // wrong variable: the same side-by-side comparison showed window capture
                // losing genuine toast hits to this gate just as often as ADB once content
                // (wave depth/FX load) was actually matched — see the ceiling's own comment
                // in ocr.rs for the measurements.
                let gc_max_ink = ocr::DEFAULT_GC_MAX_INK;

                if matches!(target, capture::CaptureTarget::AdbPhone { .. }) {
                    // Capture (USB-bound) and OCR (CPU-bound) are independent resources, so
                    // over ADB they run as a pipeline instead of strictly one after the
                    // other: a dedicated thread captures on a fixed cadence, a small pool of
                    // worker threads OCRs frames concurrently, and this thread applies their
                    // results back in capture order. The window path below is untouched —
                    // its capture+OCR cost is already well under the poll interval, so
                    // pipelining would add complexity for no gain there.
                    run_pipelined_adb_scan(
                        &running,
                        &target,
                        &app,
                        &machine,
                        &current_run_id,
                        &cached_live,
                        &conn,
                        &log_path,
                        &stages,
                        cfg.poll_interval_ms,
                        ocr_max_width,
                        gc_max_ink,
                    );
                    return;
                }

                while running.load(Ordering::SeqCst) {
                    let tick = Instant::now();

                    if !running.load(Ordering::SeqCst) {
                        break;
                    }
                    let do_gc_only = gc_only_remaining > 0;
                    let capture_started = Instant::now();
                    stages.enter(Stage::Capturing);
                    let frame = capturer.capture(&target, CAPTURE_TIMEOUT);
                    let capture_ms = capture_started.elapsed().as_millis();
                    let full = match frame {
                        Err(failure) => {
                            // Prefer a full HUD poll once the window returns.
                            gc_only_remaining = 0;
                            let status = match failure {
                                capture::CaptureFailure::Minimized => "window_minimized",
                                capture::CaptureFailure::TimedOut { .. } => "capture_stalled",
                                capture::CaptureFailure::AdbUnavailable { .. }
                                | capture::CaptureFailure::AdbCaptureFailed { .. } => {
                                    "adb_device_not_found"
                                }
                                _ => "window_not_found",
                            };
                            outage.record_failure(
                                &failure,
                                &target,
                                capturer.abandoned_workers(),
                                &log_path,
                            );
                            stages.enter(Stage::Emitting);
                            emit(&app, status, &machine, &current_run_id, &cached_live);
                            stages.enter(Stage::Sleeping);
                            sleep_remainder(tick, cfg.poll_interval_ms);
                            continue;
                        }
                        Ok(full) => {
                            outage.record_success(&log_path);
                            full
                        }
                    };
                    let status = {
                        if do_gc_only {
                            let should_continue = || running.load(Ordering::SeqCst);
                            stages.enter(Stage::OcrGoldenCombo);
                            let fields = catch_frame_panic("golden combo band OCR", || {
                                fields::ocr_gc_only_cancellable(
                                    &full,
                                    exit_y_cache,
                                    &should_continue,
                                    gc_max_ink,
                                )
                            });
                            let Some(fields) = fields else {
                                gc_only_remaining = gc_only_remaining.saturating_sub(1);
                                stages.enter(Stage::Sleeping);
                                sleep_remainder(tick, cfg.poll_interval_ms);
                                continue;
                            };
                            if !should_continue() {
                                break;
                            }
                            let gc = fields::golden_combo_from_gc_only(&fields);
                            log_line(
                                &log_path,
                                &format!(
                                    "poll kind=gc {}x{} capture_ms={} ocr_ms={} full_ms={} \
                                 gc_ms={} gc_y={} gc_c={} gc_ink={} gc_skip={} \
                                 exit_y={:?} gc_band={:?} gc={:?}",
                                    full.width(),
                                    full.height(),
                                    capture_ms,
                                    fields.ocr_ms,
                                    fields.full_ms,
                                    fields.gc_ms,
                                    fields.gc_yellow_ms,
                                    fields.gc_color_ms,
                                    fields.gc_ink,
                                    fields.gc_skip,
                                    fields.exit_battle_y,
                                    fields.gc_band_lines,
                                    gc,
                                ),
                            );
                            {
                                stages.enter(Stage::StateMachine);
                                let mut sm = machine.lock().unwrap();
                                sm.poll_golden_combo_only(gc);
                            }
                            gc_only_remaining = gc_only_remaining.saturating_sub(1);
                            "scanning"
                        } else {
                            let should_continue = || running.load(Ordering::SeqCst);
                            stages.enter(Stage::OcrFull);
                            let fields = catch_frame_panic("full frame OCR", || {
                                fields::ocr_all_fields_cancellable(
                                    &full,
                                    &should_continue,
                                    ocr_max_width,
                                    gc_max_ink,
                                )
                            });
                            let Some(fields) = fields else {
                                // Retry as a full poll: nothing was read from this frame.
                                gc_only_remaining = 0;
                                stages.enter(Stage::Sleeping);
                                sleep_remainder(tick, cfg.poll_interval_ms);
                                continue;
                            };
                            if !should_continue() {
                                break;
                            }
                            let input = fields::poll_input_from_fields(&fields, &full);
                            log_line(
                                &log_path,
                                &format!(
                                    "poll kind=full {}x{} capture_ms={} ocr_ms={} full_ms={} \
                                 gc_ms={} gc_y={} gc_c={} gc_ink={} gc_skip={} \
                                 tier={:?} wave={:?} coin={:?} skip={:?} \
                                 exit_y={:?} gc_band={:?} lines={:?}",
                                    full.width(),
                                    full.height(),
                                    capture_ms,
                                    fields.ocr_ms,
                                    fields.full_ms,
                                    fields.gc_ms,
                                    fields.gc_yellow_ms,
                                    fields.gc_color_ms,
                                    fields.gc_ink,
                                    fields.gc_skip,
                                    input.tier,
                                    input.wave,
                                    input.coin,
                                    input.wave_skip_overlay,
                                    fields.exit_battle_y,
                                    fields.gc_band_lines,
                                    fields.all_lines,
                                ),
                            );
                            let frame_ctx = crate::notifications::frame_context_from_poll(&input);
                            stages.enter(Stage::StateMachine);
                            let (actions, live) = {
                                let mut sm = machine.lock().unwrap();
                                let actions = sm.poll(input);
                                let live = sm.live_state();
                                (actions, live)
                            };
                            if !actions.is_empty() {
                                stages.enter(Stage::Persisting);
                                apply_actions(&conn, &current_run_id, &actions, &log_path);
                                stages.enter(Stage::Notifying);
                                notify_scanner_actions(&app, &actions, Some(&full), frame_ctx);
                            }
                            if let Some(notify) =
                                app.try_state::<crate::notifications::NotifyState>()
                            {
                                stages.enter(Stage::Notifying);
                                notify.on_poll(
                                    &app,
                                    &fields.all_lines,
                                    &live,
                                    Some(&full),
                                    frame_ctx,
                                );
                            }
                            if fields.exit_battle_y.is_some() {
                                exit_y_cache = fields.exit_battle_y;
                            }
                            gc_only_remaining = gc_only_ticks_between_full;
                            "scanning"
                        }
                    };
                    stages.enter(Stage::Emitting);
                    emit(&app, status, &machine, &current_run_id, &cached_live);
                    stages.enter(Stage::Sleeping);
                    sleep_remainder(tick, cfg.poll_interval_ms);
                }
            })
            .map_err(|e| {
                self.running.store(false, Ordering::SeqCst);
                format!("could not start the scanner thread: {e}")
            })?;
        Ok(())
    }

    fn prepare_resume(&self, conn: &rusqlite::Connection) -> Result<Option<u32>, String> {
        let Some((id, run_type)) = db::latest_open_run(conn).map_err(|e| e.to_string())? else {
            return Err("No run to resume — start a new run instead.".into());
        };
        let (last_wave, peak_tier) = db::snapshot_stats(conn, &id).map_err(|e| e.to_string())?;
        let last_wave = last_wave.unwrap_or(0) as u32;
        let run_type = RunType::from_db_str(&run_type);
        let last_gc = db::latest_golden_combo(conn, &id)
            .map_err(|e| e.to_string())?
            .map(|(chance, caret, mult)| GoldenComboReading {
                seen: true,
                chance_percent: chance,
                caret_count: Some(caret as u32),
                multiplier: mult,
            });
        // Always re-sync from DB: the game may have advanced while the scanner was stopped.
        self.machine.lock().unwrap().resume_from_db(
            run_type,
            last_wave,
            peak_tier.map(|t| t as u32),
            last_gc,
        );
        *self.current_run_id.lock().unwrap() = Some(id);
        Ok(Some(last_wave))
    }
}

/// Whether a dispatched job should read the whole HUD or just the GC band. GC is sampled
/// on every job either way (see [`JobKind::GcOnly`]) — this only controls how often the
/// slower-changing wave/tier/coin fields get re-read, the same trade the window path has
/// always made (`GC_ONLY_TICKS_BETWEEN_FULL`), now safe to make on ADB too since capture
/// no longer waits on OCR to decide the next tick's timing.
#[derive(Clone, Copy, PartialEq, Eq)]
enum JobKind {
    Full,
    GcOnly,
}

/// One captured frame (or capture failure) tagged with its position in capture order,
/// dispatched from the capture thread to the OCR worker pool.
struct PipelineJob {
    seq: u64,
    kind: JobKind,
    frame: Result<image::RgbaImage, capture::CaptureFailure>,
    capture_ms: u128,
    abandoned_workers: u32,
}

/// What an OCR worker produced for one job, before it's applied to scanner state.
enum PipelineOutcome {
    CaptureFailed(capture::CaptureFailure),
    Full(fields::FieldOcr, image::RgbaImage),
    GcOnly(fields::FieldOcr, image::RgbaImage),
    /// Stop was requested mid-poll (panic or cancellation) — nothing to apply.
    Cancelled,
}

/// An OCR worker's result, still tagged with `seq` so the consumer can apply results in
/// capture order even though workers finish in whatever order their jobs happen to cost.
struct PipelineResult {
    seq: u64,
    capture_ms: u128,
    abandoned_workers: u32,
    outcome: PipelineOutcome,
}

/// Runs the ADB capture source as a pipeline: a dedicated thread captures on a fixed
/// cadence, [`ADB_OCR_WORKERS`] threads OCR frames concurrently, and this (the caller's)
/// thread applies results back in capture order. Blocks until `running` goes false and
/// every spawned thread has wound down.
///
/// Capture (USB-bound, ~650-730ms) and OCR (CPU-bound, ~1-1.9s on a downscaled ADB
/// frame) are independent resources. Running them strictly one after another — as the
/// window path does, where it costs nothing because that path is cheap either way — means
/// the effective poll cadence is `capture_ms + ocr_ms` regardless of the configured poll
/// interval, which on ADB was ~1.6-1.9s: slower than the 1s cadence the window path gets,
/// and slow enough to plausibly miss a brief Golden Combo toast. Decoupling them lets
/// capture run on its own ~1s cadence (matching the window path's sampling rate) while a
/// small worker pool absorbs the OCR cost in the background.
///
/// The one thing this can't relax is state-machine ordering: wave-change debouncing, GC
/// toast rise/fade tracking, and wave-skip banner detection all assume each poll reflects
/// a later moment than the last one applied. Concurrent OCR workers can finish out of
/// capture order (a fast frame can land behind a slow one submitted earlier), so results
/// are buffered by `seq` and only applied once every earlier `seq` has already landed.
#[allow(clippy::too_many_arguments)]
fn run_pipelined_adb_scan(
    running: &Arc<AtomicBool>,
    target: &capture::CaptureTarget,
    app: &AppHandle,
    machine: &Arc<Mutex<RunStateMachine>>,
    current_run_id: &Arc<Mutex<Option<String>>>,
    cached_live: &Arc<Mutex<LiveState>>,
    conn: &rusqlite::Connection,
    log_path: &std::path::Path,
    stages: &Arc<StageTracker>,
    poll_interval_ms: u64,
    ocr_max_width: u32,
    gc_max_ink: u32,
) {
    let (job_tx, job_rx) = std::sync::mpsc::sync_channel::<PipelineJob>(ADB_PIPELINE_QUEUE_DEPTH);
    let job_rx = Arc::new(Mutex::new(job_rx));
    let (result_tx, result_rx) = std::sync::mpsc::channel::<PipelineResult>();
    // Exit Battle's Y anchors the GC toast search corridor (see `toast_corridor` in
    // ocr.rs). Full jobs refresh it; GC-only jobs — run by whichever worker happens to
    // pick one up — just read the latest value. A little staleness is fine: the corridor
    // already has a documented default for when this is unknown at all.
    let exit_y_cache: Arc<Mutex<Option<f32>>> = Arc::new(Mutex::new(None));

    let producer = {
        let running = running.clone();
        let target = target.clone();
        let stages = stages.clone();
        std::thread::Builder::new()
            .name("wavetrace-scanner-adb-capture".into())
            .spawn(move || {
                let mut capturer = capture::TimeboxedCapture::new();
                let mut seq: u64 = 0;
                let mut gc_only_remaining: u32 = 0;
                while running.load(Ordering::SeqCst) {
                    let tick = Instant::now();
                    let kind = if gc_only_remaining > 0 {
                        JobKind::GcOnly
                    } else {
                        JobKind::Full
                    };
                    stages.enter(Stage::Capturing);
                    let capture_started = Instant::now();
                    let frame = capturer.capture(&target, CAPTURE_TIMEOUT);
                    let capture_ms = capture_started.elapsed().as_millis();
                    let abandoned_workers = capturer.abandoned_workers();
                    match &frame {
                        // Prefer a full HUD poll once the device returns.
                        Err(_) => gc_only_remaining = 0,
                        Ok(_) if kind == JobKind::Full => {
                            gc_only_remaining = ADB_GC_ONLY_TICKS_BETWEEN_FULL;
                        }
                        Ok(_) => gc_only_remaining = gc_only_remaining.saturating_sub(1),
                    }
                    seq += 1;
                    let job = PipelineJob {
                        seq,
                        kind,
                        frame,
                        capture_ms,
                        abandoned_workers,
                    };
                    if job_tx.send(job).is_err() {
                        break;
                    }
                    stages.enter(Stage::Sleeping);
                    sleep_remainder(tick, poll_interval_ms);
                }
            })
            .expect("spawn adb capture thread")
    };

    let mut workers = Vec::with_capacity(ADB_OCR_WORKERS);
    for i in 0..ADB_OCR_WORKERS {
        let job_rx = job_rx.clone();
        let result_tx = result_tx.clone();
        let running = running.clone();
        let stages = stages.clone();
        let exit_y_cache = exit_y_cache.clone();
        let handle = std::thread::Builder::new()
            .name(format!("wavetrace-scanner-adb-ocr-{i}"))
            .spawn(move || loop {
                let job = {
                    let rx = job_rx.lock().unwrap();
                    rx.recv()
                };
                let Ok(job) = job else { break };
                let outcome = match job.frame {
                    Err(failure) => PipelineOutcome::CaptureFailed(failure),
                    Ok(full) => {
                        let should_continue = || running.load(Ordering::SeqCst);
                        match job.kind {
                            JobKind::Full => {
                                stages.enter(Stage::OcrFull);
                                match catch_frame_panic("full frame OCR (pipelined)", || {
                                    fields::ocr_all_fields_cancellable(
                                        &full,
                                        &should_continue,
                                        ocr_max_width,
                                        gc_max_ink,
                                    )
                                }) {
                                    Some(fields) if should_continue() => {
                                        if fields.exit_battle_y.is_some() {
                                            *exit_y_cache.lock().unwrap() = fields.exit_battle_y;
                                        }
                                        PipelineOutcome::Full(fields, full)
                                    }
                                    _ => PipelineOutcome::Cancelled,
                                }
                            }
                            JobKind::GcOnly => {
                                let exit_y = *exit_y_cache.lock().unwrap();
                                stages.enter(Stage::OcrGoldenCombo);
                                match catch_frame_panic("golden combo band OCR (pipelined)", || {
                                    fields::ocr_gc_only_cancellable(
                                        &full,
                                        exit_y,
                                        &should_continue,
                                        gc_max_ink,
                                    )
                                }) {
                                    Some(fields) if should_continue() => {
                                        PipelineOutcome::GcOnly(fields, full)
                                    }
                                    _ => PipelineOutcome::Cancelled,
                                }
                            }
                        }
                    }
                };
                let result = PipelineResult {
                    seq: job.seq,
                    capture_ms: job.capture_ms,
                    abandoned_workers: job.abandoned_workers,
                    outcome,
                };
                if result_tx.send(result).is_err() {
                    break;
                }
            })
            .expect("spawn adb ocr worker thread");
        workers.push(handle);
    }
    drop(result_tx);
    drop(job_rx);

    let mut pending: std::collections::BTreeMap<u64, PipelineResult> =
        std::collections::BTreeMap::new();
    let mut next_seq: u64 = 1;
    let mut outage = CaptureOutage::default();

    while running.load(Ordering::SeqCst) {
        let result = match result_rx.recv_timeout(Duration::from_millis(500)) {
            Ok(r) => r,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        };
        pending.insert(result.seq, result);
        while let Some(result) = pending.remove(&next_seq) {
            apply_pipeline_result(
                result,
                target,
                app,
                machine,
                current_run_id,
                cached_live,
                conn,
                log_path,
                stages,
                &mut outage,
            );
            next_seq += 1;
        }
    }

    // Ensure no ADB subprocess or OCR call outlives this function: every spawned thread
    // checks `running` (already false by the time we get here) at its own loop boundary.
    let _ = producer.join();
    for w in workers {
        let _ = w.join();
    }
}

/// Applies one pipelined poll's outcome — mirrors the window path's inline per-tick logic
/// (log, state machine, persist, notify, emit), just fed from a queued result instead of
/// running right after its own capture.
#[allow(clippy::too_many_arguments)]
fn apply_pipeline_result(
    result: PipelineResult,
    target: &capture::CaptureTarget,
    app: &AppHandle,
    machine: &Arc<Mutex<RunStateMachine>>,
    current_run_id: &Arc<Mutex<Option<String>>>,
    cached_live: &Arc<Mutex<LiveState>>,
    conn: &rusqlite::Connection,
    log_path: &std::path::Path,
    stages: &Arc<StageTracker>,
    outage: &mut CaptureOutage,
) {
    let PipelineResult {
        capture_ms,
        abandoned_workers,
        outcome,
        ..
    } = result;
    match outcome {
        PipelineOutcome::Cancelled => {}
        PipelineOutcome::CaptureFailed(failure) => {
            let status = match failure {
                capture::CaptureFailure::Minimized => "window_minimized",
                capture::CaptureFailure::TimedOut { .. } => "capture_stalled",
                capture::CaptureFailure::AdbUnavailable { .. }
                | capture::CaptureFailure::AdbCaptureFailed { .. } => "adb_device_not_found",
                _ => "window_not_found",
            };
            outage.record_failure(&failure, target, abandoned_workers, log_path);
            stages.enter(Stage::Emitting);
            emit(app, status, machine, current_run_id, cached_live);
        }
        PipelineOutcome::GcOnly(fields, full) => {
            outage.record_success(log_path);
            let gc = fields::golden_combo_from_gc_only(&fields);
            log_line(
                log_path,
                &format!(
                    "poll kind=gc {}x{} capture_ms={} ocr_ms={} full_ms={} \
                 gc_ms={} gc_y={} gc_c={} gc_ink={} gc_skip={} \
                 exit_y={:?} gc_band={:?} gc={:?}",
                    full.width(),
                    full.height(),
                    capture_ms,
                    fields.ocr_ms,
                    fields.full_ms,
                    fields.gc_ms,
                    fields.gc_yellow_ms,
                    fields.gc_color_ms,
                    fields.gc_ink,
                    fields.gc_skip,
                    fields.exit_battle_y,
                    fields.gc_band_lines,
                    gc,
                ),
            );
            stages.enter(Stage::StateMachine);
            {
                let mut sm = machine.lock().unwrap();
                sm.poll_golden_combo_only(gc);
            }
            stages.enter(Stage::Emitting);
            emit(app, "scanning", machine, current_run_id, cached_live);
        }
        PipelineOutcome::Full(fields, full) => {
            outage.record_success(log_path);
            let input = fields::poll_input_from_fields(&fields, &full);
            log_line(
                log_path,
                &format!(
                    "poll kind=full {}x{} capture_ms={} ocr_ms={} full_ms={} \
                 gc_ms={} gc_y={} gc_c={} gc_ink={} gc_skip={} \
                 tier={:?} wave={:?} coin={:?} skip={:?} \
                 exit_y={:?} gc_band={:?} lines={:?}",
                    full.width(),
                    full.height(),
                    capture_ms,
                    fields.ocr_ms,
                    fields.full_ms,
                    fields.gc_ms,
                    fields.gc_yellow_ms,
                    fields.gc_color_ms,
                    fields.gc_ink,
                    fields.gc_skip,
                    input.tier,
                    input.wave,
                    input.coin,
                    input.wave_skip_overlay,
                    fields.exit_battle_y,
                    fields.gc_band_lines,
                    fields.all_lines,
                ),
            );
            let frame_ctx = crate::notifications::frame_context_from_poll(&input);
            stages.enter(Stage::StateMachine);
            let (actions, live) = {
                let mut sm = machine.lock().unwrap();
                let actions = sm.poll(input);
                let live = sm.live_state();
                (actions, live)
            };
            if !actions.is_empty() {
                stages.enter(Stage::Persisting);
                apply_actions(conn, current_run_id, &actions, log_path);
                stages.enter(Stage::Notifying);
                notify_scanner_actions(app, &actions, Some(&full), frame_ctx);
            }
            if let Some(notify) = app.try_state::<crate::notifications::NotifyState>() {
                stages.enter(Stage::Notifying);
                notify.on_poll(app, &fields.all_lines, &live, Some(&full), frame_ctx);
            }
            stages.enter(Stage::Emitting);
            emit(app, "scanning", machine, current_run_id, cached_live);
        }
    }
}

/// End any active run and open a new one, tagging from the current game screen.
pub fn new_run_actions(
    machine: &mut RunStateMachine,
    target: &capture::CaptureTarget,
) -> Vec<Action> {
    // Image-only dissonance detection — Windows OCR (WinRT) must run on the scanner
    // thread; calling it here would hit RoInitialize on the UI/command thread.
    if let Some(frame) = capture::capture_target(target) {
        if let Some(kind) = crate::dissonance_icons::detect(&frame) {
            machine.absorb_dissonance(kind);
        }
    }
    machine.manual_new_run()
}

pub fn apply_actions(
    conn: &rusqlite::Connection,
    current_run_id: &Arc<Mutex<Option<String>>>,
    actions: &[Action],
    _log_path: &std::path::Path,
) {
    for action in actions {
        let result = match action {
            Action::StartRun { run_type } => {
                // Drop stale tracking before start_run closes any open rows.
                current_run_id.lock().unwrap().take();
                db::start_run(conn, run_type.as_str())
                    .map(|id| *current_run_id.lock().unwrap() = Some(id))
            }
            Action::Snapshot {
                wave,
                tier,
                coin_per_minute,
                golden_combo_chance,
                golden_combo_caret,
                golden_combo_multiplier,
            } => {
                let id = current_run_id.lock().unwrap().clone();
                match id {
                    Some(id) => db::insert_snapshot(
                        conn,
                        &id,
                        *wave as i64,
                        tier.map(|t| t as i64),
                        *coin_per_minute,
                        *golden_combo_chance,
                        golden_combo_caret.map(|n| n as i64),
                        *golden_combo_multiplier,
                    ),
                    None => Ok(()),
                }
            }
            Action::WaveSkip {
                at_wave,
                skipped_count,
                skip_multiplier,
                coin_per_minute,
            } => {
                let id = current_run_id.lock().unwrap().clone();
                match id {
                    Some(id) => db::insert_wave_skip(
                        conn,
                        &id,
                        *at_wave as i64,
                        *skipped_count as i64,
                        skip_multiplier.map(|n| n as i64),
                        *coin_per_minute,
                    ),
                    None => Ok(()),
                }
            }
            Action::EndRun {
                final_wave,
                peak_tier,
                ..
            } => {
                let id = current_run_id.lock().unwrap().take();
                match id {
                    Some(id) => db::end_run(
                        conn,
                        &id,
                        Some(*final_wave as i64),
                        peak_tier.map(|t| t as i64),
                    ),
                    None => Ok(()),
                }
            }
        };
        if let Err(e) = result {
            db::append_app_log(&format!("DB error applying {action:?}: {e}"));
        } else if matches!(action, Action::WaveSkip { .. }) {
            db::append_app_log(&format!("Recorded {action:?}"));
        }
    }
}

fn log_line(_dir: &std::path::Path, msg: &str) {
    db::append_app_log(msg);
}

/// Runs one frame's image processing, turning a panic into a skipped frame. A panic in
/// here unwound the whole poll loop, which stopped scanning while the UI still reported
/// "scanning" over the last values it had — silent to everyone but the log.
fn catch_frame_panic<T>(what: &str, work: impl FnOnce() -> T) -> Option<T> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(work)) {
        Ok(value) => Some(value),
        // The panic hook already logged the payload and source location.
        Err(_) => {
            db::append_app_log(&format!("skipped frame: {what} panicked"));
            None
        }
    }
}

/// Publishes the stopped state however the poll loop ends, panic included. A scanner
/// left marked as running keeps the UI claiming it is live and refuses to restart.
struct ScannerExitGuard {
    running: Arc<AtomicBool>,
    app: AppHandle,
    machine: Arc<Mutex<RunStateMachine>>,
    current_run_id: Arc<Mutex<Option<String>>>,
    cached_live: Arc<Mutex<LiveState>>,
    app_slot: Arc<Mutex<Option<AppHandle>>>,
}

impl Drop for ScannerExitGuard {
    fn drop(&mut self) {
        self.running.store(false, Ordering::SeqCst);
        // This can run while a panic unwinds, where panicking again would abort.
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            emit(
                &self.app,
                "stopped",
                &self.machine,
                &self.current_run_id,
                &self.cached_live,
            );
            db::append_app_log("scanner thread exiting");
            if let Ok(mut guard) = self.app_slot.lock() {
                *guard = None;
            }
        }));
    }
}

/// Watches the poll loop from a second thread and reports any step that stops coming
/// back. A wedged step produces no log output of its own, so without this the log for a
/// scanner that hangs looks exactly like the log for an app that was closed.
fn spawn_stall_watchdog(stages: Arc<StageTracker>, running: Arc<AtomicBool>) {
    let _ = std::thread::Builder::new()
        .name("wavetrace-scanner-watchdog".into())
        .spawn(move || {
            let mut reported: Option<(Stage, Instant)> = None;
            while running.load(Ordering::SeqCst) {
                std::thread::sleep(WATCHDOG_TICK);
                let (stage, elapsed) = stages.current();
                if elapsed < STAGE_STALL_AFTER {
                    if let Some((stalled_stage, _)) = reported.take() {
                        db::append_app_log(&format!(
                            "scanner recovered from stall in {}",
                            stalled_stage.label()
                        ));
                    }
                    continue;
                }
                let due = match reported {
                    Some((prev, at)) => prev != stage || at.elapsed() >= STAGE_STALL_REPEAT,
                    None => true,
                };
                if due {
                    db::append_app_log(&format!(
                        "scanner STALLED in {} for {:.1}s — no poll can complete until it returns",
                        stage.label(),
                        elapsed.as_secs_f64(),
                    ));
                    reported = Some((stage, Instant::now()));
                }
            }
        });
}

/// How often to restate an ongoing capture outage in the log.
const CAPTURE_OUTAGE_HEARTBEAT: Duration = Duration::from_secs(60);

/// Tracks a run of polls that produced no frame. Without this the loop simply skips
/// the poll and logs nothing, so an hours-long outage is invisible in the log — it
/// looks identical to the app not running at all.
#[derive(Default)]
struct CaptureOutage {
    started: Option<Instant>,
    last_logged: Option<Instant>,
    missed_polls: u64,
    reason: Option<&'static str>,
}

impl CaptureOutage {
    fn record_failure(
        &mut self,
        failure: &capture::CaptureFailure,
        target: &capture::CaptureTarget,
        abandoned_threads: u32,
        log_path: &std::path::Path,
    ) {
        self.missed_polls += 1;
        let reason = failure.tag();
        let reason_changed = self.reason != Some(reason);
        self.reason = Some(reason);
        let now = Instant::now();
        let started = *self.started.get_or_insert(now);
        let due = self
            .last_logged
            .is_none_or(|at| now.duration_since(at) >= CAPTURE_OUTAGE_HEARTBEAT);
        if !due && !reason_changed {
            return;
        }
        self.last_logged = Some(now);

        let detail = match failure {
            capture::CaptureFailure::EnumerateFailed { error } => format!(" error={error:?}"),
            capture::CaptureFailure::TimedOut { after_ms } => {
                format!(
                    " gave_up_after_ms={after_ms} abandoned_capture_threads={abandoned_threads}"
                )
            }
            capture::CaptureFailure::AdbUnavailable { error }
            | capture::CaptureFailure::AdbCaptureFailed { error } => format!(" error={error:?}"),
            _ => String::new(),
        };
        let (target_desc, os_state) = match target {
            capture::CaptureTarget::Window(tw) => {
                // Ask the OS directly: xcap's window list drops minimized, cloaked (e.g.
                // moved to another virtual desktop) and zero-size windows alike, so this is
                // the only way to tell those apart afterwards from a log.
                let os_state = crate::window_probe::describe_matching_windows(&tw.title_substring)
                    .map(|s| format!(" os_windows=[{s}]"))
                    .unwrap_or_default();
                (
                    format!("target={:?} app={:?}", tw.title_substring, tw.process_name),
                    os_state,
                )
            }
            capture::CaptureTarget::AdbPhone { serial, .. } => {
                (format!("target=adb:{serial:?}"), String::new())
            }
        };
        log_line(
            log_path,
            &format!(
                "capture unavailable reason={reason} for={:.1}s missed_polls={} \
                 {target_desc}{detail}{os_state}",
                now.duration_since(started).as_secs_f64(),
                self.missed_polls,
            ),
        );
    }

    fn record_success(&mut self, log_path: &std::path::Path) {
        let Some(started) = self.started.take() else {
            return;
        };
        log_line(
            log_path,
            &format!(
                "capture restored after {:.1}s reason={} missed_polls={}",
                started.elapsed().as_secs_f64(),
                self.reason.unwrap_or("unknown"),
                self.missed_polls,
            ),
        );
        self.last_logged = None;
        self.missed_polls = 0;
        self.reason = None;
    }
}

fn emit(
    app: &AppHandle,
    status: &str,
    machine: &Arc<Mutex<RunStateMachine>>,
    current_run_id: &Arc<Mutex<Option<String>>>,
    cached_live: &Arc<Mutex<LiveState>>,
) {
    let live = machine.lock().unwrap().live_state();
    *cached_live.lock().unwrap() = live.clone();
    if let Some(notify) = app.try_state::<crate::notifications::NotifyState>() {
        notify.on_scanner_status(app, status, &live);
    }
    crate::tray::update_scanner_ui(app, status, &live);
    let event = ScannerEvent {
        status: status.to_string(),
        live: Some(live),
        current_run_id: current_run_id.lock().unwrap().clone(),
    };
    app.emit("scanner-update", event).ok();
}

pub fn notify_scanner_actions(
    app: &AppHandle,
    actions: &[Action],
    capture: Option<&image::RgbaImage>,
    frame: NotifyFrameContext,
) {
    if let Some(notify) = app.try_state::<crate::notifications::NotifyState>() {
        notify.on_actions(app, actions, capture, frame);
    }
}

fn sleep_remainder(tick: Instant, interval_ms: u64) {
    let elapsed = tick.elapsed();
    let interval = Duration::from_millis(interval_ms);
    if elapsed < interval {
        std::thread::sleep(interval - elapsed);
    }
}

#[cfg(test)]
mod tests {
    use super::{Stage, StageTracker};

    #[test]
    fn stage_survives_the_atomic_round_trip() {
        for stage in [
            Stage::Sleeping,
            Stage::Capturing,
            Stage::OcrFull,
            Stage::OcrGoldenCombo,
            Stage::StateMachine,
            Stage::Persisting,
            Stage::Notifying,
            Stage::Emitting,
        ] {
            assert_eq!(Stage::from_u8(stage as u8), stage);
        }
    }

    #[test]
    fn tracker_reports_the_stage_it_entered() {
        let tracker = StageTracker::new();
        tracker.enter(Stage::Capturing);
        let (stage, elapsed) = tracker.current();
        assert_eq!(stage, Stage::Capturing);
        assert!(elapsed.as_secs() < 1);
    }
}
