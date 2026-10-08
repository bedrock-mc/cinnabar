//! Rate-limited INFO diagnostics for server movement corrections.

use std::{
    sync::Mutex,
    time::{Duration, Instant},
};

use sim::MovementMode;
use tracing::info;

use super::PhysicsMovementSample;

const MIN_LINE_SPACING: Duration = Duration::from_millis(250);
const SUMMARY_WINDOW: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CorrectionKind {
    Correct,
    Teleport,
}

impl CorrectionKind {
    const fn name(self) -> &'static str {
        match self {
            Self::Correct => "correct",
            Self::Teleport => "teleport",
        }
    }
}

struct Window {
    started: Option<Instant>,
    last_line: Option<Instant>,
    total: u64,
    in_window: u64,
    suppressed: u64,
}

static STATE: Mutex<Window> = Mutex::new(Window {
    started: None,
    last_line: None,
    total: 0,
    in_window: 0,
    suppressed: 0,
});

/// Decides whether this event prints, and emits the per-minute summary when due.
fn admit(now: Instant) -> Option<u64> {
    let mut state = STATE.lock().ok()?;
    state.total += 1;
    state.in_window += 1;
    let started = *state.started.get_or_insert(now);
    if now.duration_since(started) >= SUMMARY_WINDOW {
        info!(
            corrections = state.in_window,
            total = state.total,
            "movement corrections in the last window"
        );
        state.started = Some(now);
        state.in_window = 0;
    }
    if state
        .last_line
        .is_some_and(|last| now.duration_since(last) < MIN_LINE_SPACING)
    {
        state.suppressed += 1;
        return None;
    }
    state.last_line = Some(now);
    Some(std::mem::take(&mut state.suppressed))
}

fn mode_name(sample: &PhysicsMovementSample) -> &'static str {
    match sample.processed.mode {
        MovementMode::Flying => "fly",
        MovementMode::Gliding => "glide",
        MovementMode::Swimming => "swim",
        MovementMode::Crawling => "crawl",
        MovementMode::Riding => "ride",
        MovementMode::Walking if sample.sneaking => "sneak",
        MovementMode::Walking if sample.sprinting => "sprint",
        MovementMode::Walking => "walk",
    }
}

fn input_names(sample: &PhysicsMovementSample) -> String {
    let mut names = Vec::new();
    for (on, name) in [
        (sample.move_vector[1] > 0.0, "up"),
        (sample.move_vector[1] < 0.0, "down"),
        (sample.move_vector[0] < 0.0, "left"),
        (sample.move_vector[0] > 0.0, "right"),
        (sample.jumping, "jump"),
        (sample.sneaking, "sneak"),
        (sample.sprinting, "sprint"),
        (sample.horizontal_collision, "hcol"),
        (sample.vertical_collision, "vcol"),
    ] {
        if on {
            names.push(name);
        }
    }
    names.join("|")
}

/// Logs one server correction against the retained prediction sample for `tick`.
pub fn note_correction(
    kind: CorrectionKind,
    tick: u64,
    server_position: [f32; 3],
    server_on_ground: bool,
    predicted: Option<&PhysicsMovementSample>,
) {
    let Some(suppressed) = admit(Instant::now()) else {
        return;
    };
    match predicted {
        Some(sample) => {
            let delta = std::array::from_fn::<f32, 3, _>(|axis| {
                server_position[axis] - sample.position[axis]
            });
            info!(
                kind = kind.name(),
                tick,
                predicted = ?sample.position,
                server = ?server_position,
                ?delta,
                predicted_on_ground = sample.grounded_after_tick,
                server_on_ground,
                mode = mode_name(sample),
                inputs = %input_names(sample),
                last_pos_delta = ?sample.movement,
                suppressed,
                "server movement correction"
            );
        }
        None => info!(
            kind = kind.name(),
            tick,
            server = ?server_position,
            server_on_ground,
            suppressed,
            "server movement correction (no retained prediction)"
        ),
    }
}

/// Logs one server-driven velocity impulse.
pub fn note_motion(tick: u64, motion: [f32; 3]) {
    let Some(suppressed) = admit(Instant::now()) else {
        return;
    };
    info!(
        kind = "motion",
        tick,
        ?motion,
        suppressed,
        "server movement correction"
    );
}

static DROPPED_CORRECTIONS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Counts and logs one correction dropped for a tick outside retained history.
pub fn note_dropped_correction(tick: u64) -> u64 {
    let dropped = DROPPED_CORRECTIONS.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
    if admit(Instant::now()).is_some() {
        info!(
            tick,
            dropped, "server movement correction outside retained history dropped"
        );
    }
    dropped
}

static SKIPPED_AUTHORITY: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Counts and logs one finite server value too large for prediction to simulate.
pub fn note_skipped_authority(kind: &'static str, value: f64) -> u64 {
    let skipped = SKIPPED_AUTHORITY.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
    if admit(Instant::now()).is_some() {
        info!(
            kind,
            value, skipped, "unsimulable server movement value skipped"
        );
    }
    skipped
}
