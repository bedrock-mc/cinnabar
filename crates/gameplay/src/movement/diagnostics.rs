//! Rate-limited INFO diagnostics for server movement corrections.

use std::{sync::Mutex, time::Duration};
use web_time::Instant;

use client_world::{CommittedControlEvent, LocalPlayerFacts};
use protocol::{MovePlayerMode, PlayerGameMode};
use sim::MovementMode;
use tracing::info;

use super::PhysicsMovementSample;

const MIN_LINE_SPACING: Duration = Duration::from_millis(250);
const SUMMARY_WINDOW: Duration = Duration::from_secs(60);
/// Context lines printed per session before only one per summary window prints.
const FULL_CONTEXT_LINES: u64 = 16;
const MAX_CAMERA_PRESET_CHARS: usize = 64;

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

/// Packet facts of one server position correction or teleport.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CorrectionPacket {
    /// None for CorrectPlayerMovePrediction.
    pub move_mode: Option<MovePlayerMode>,
    pub tick: u64,
    /// MovePlayer's vehicle runtime id (0 when none); CorrectPlayerMovePrediction has none.
    pub riding_runtime_id: Option<u64>,
    pub teleport_cause: Option<i32>,
    pub on_ground: bool,
}

impl CorrectionPacket {
    /// Returns the facts of a committed MovePlayer or prediction correction, else None.
    pub fn from_control(control: &CommittedControlEvent) -> Option<Self> {
        match control {
            CommittedControlEvent::MovePlayer { movement, .. } => Some(Self {
                move_mode: Some(movement.mode),
                tick: movement.source_tick,
                riding_runtime_id: Some(movement.riding_runtime_id),
                teleport_cause: movement.teleport_cause,
                on_ground: movement.on_ground,
            }),
            CommittedControlEvent::PlayerMovementCorrection { correction, .. } => Some(Self {
                move_mode: None,
                tick: correction.tick,
                riding_runtime_id: None,
                teleport_cause: None,
                on_ground: correction.on_ground,
            }),
            _ => None,
        }
    }
}

/// Local state that decides whether the player hovers, rides or falls between server snaps.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct CorrectionContext<'a> {
    pub game_mode: Option<PlayerGameMode>,
    pub can_fly: bool,
    pub server_flying: bool,
    pub has_gravity: bool,
    pub immobile: bool,
    pub mount_unique_id: Option<i64>,
    /// False while the client follows server positions without local prediction.
    pub predicting: bool,
    pub camera_preset: Option<&'a str>,
    /// A server camera pose or entity attachment replaces the player view.
    pub camera_override: bool,
}

impl<'a> CorrectionContext<'a> {
    /// Reads the committed player facts; camera state comes from its presentation owner.
    pub fn read(
        facts: &LocalPlayerFacts,
        predicting: bool,
        camera_preset: Option<&'a str>,
        camera_override: bool,
    ) -> Self {
        let capabilities = facts.game_mode_capabilities();
        Self {
            game_mode: facts.player_game_mode(),
            can_fly: capabilities.is_some_and(|capabilities| capabilities.can_fly),
            server_flying: capabilities.is_some_and(|capabilities| capabilities.flight.is_flying()),
            has_gravity: facts.has_gravity(),
            immobile: facts.is_immobile(),
            mount_unique_id: facts.mount_unique_id(),
            predicting,
            camera_preset,
            camera_override,
        }
    }
}

/// Whether one correction's context line printed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CorrectionContextLine {
    /// `suppressed` counts the quiet corrections since the previous printed line.
    Printed {
        seq: u64,
        suppressed: u64,
    },
    Quiet,
}

/// Per-session budget for correction context lines.
#[derive(Debug, Default)]
pub struct CorrectionContextLog {
    session: u64,
    total: u64,
    suppressed: u64,
    last_line: Option<Instant>,
}

impl CorrectionContextLog {
    /// Prints a session's first lines, then one line per summary window carrying the quiet count.
    pub fn admit(&mut self, session: u64, now: Instant) -> CorrectionContextLine {
        if self.session != session {
            *self = Self {
                session,
                ..Self::default()
            };
        }
        self.total += 1;
        let due = self.total <= FULL_CONTEXT_LINES
            || self
                .last_line
                .is_none_or(|last| now.duration_since(last) >= SUMMARY_WINDOW);
        if !due {
            self.suppressed += 1;
            return CorrectionContextLine::Quiet;
        }
        self.last_line = Some(now);
        CorrectionContextLine::Printed {
            seq: self.total,
            suppressed: std::mem::take(&mut self.suppressed),
        }
    }

    /// Logs one correction's packet facts and local movement state within the budget.
    pub fn note(
        &mut self,
        session: u64,
        now: Instant,
        packet: CorrectionPacket,
        context: CorrectionContext<'_>,
    ) -> CorrectionContextLine {
        let line = self.admit(session, now);
        if let CorrectionContextLine::Printed { seq, suppressed } = line {
            info!(
                packet = if packet.move_mode.is_some() {
                    "move_player"
                } else {
                    "correct"
                },
                move_mode = ?packet.move_mode,
                tick = packet.tick,
                riding_runtime_id = ?packet.riding_runtime_id,
                teleport_cause = ?packet.teleport_cause,
                server_on_ground = packet.on_ground,
                game_mode = ?context.game_mode,
                can_fly = context.can_fly,
                server_flying = context.server_flying,
                has_gravity = context.has_gravity,
                immobile = context.immobile,
                mount_unique_id = ?context.mount_unique_id,
                predicting = context.predicting,
                camera_preset = ?context.camera_preset.map(bounded_preset),
                camera_override = context.camera_override,
                seq,
                suppressed,
                "server correction movement state"
            );
        }
        line
    }
}

/// Keeps a server-chosen preset name from unbounding the line.
fn bounded_preset(name: &str) -> &str {
    name.char_indices()
        .nth(MAX_CAMERA_PRESET_CHARS)
        .map_or(name, |(end, _)| &name[..end])
}

#[cfg(test)]
#[path = "diagnostics_tests.rs"]
mod tests;
