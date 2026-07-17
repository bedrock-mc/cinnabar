use std::{collections::VecDeque, time::Duration};

use bevy::prelude::Resource;
use protocol::{
    Packet, PlayerAuthInputError, PlayerAuthInputSnapshot, PlayerInputFlags, PlayerInputMode,
    player_auth_input,
};

mod physics;
pub use physics::{
    LocalPhysicsController, LocalPhysicsFrame, MAX_LOCAL_PHYSICS_TICKS_PER_FRAME,
    PhysicsCollisionRegistries, physics_movement_input,
};

use bevy::{
    log::debug,
    prelude::{
        ButtonInput, EulerRot, KeyCode, Local, Res, ResMut, Single, Time, Transform, Vec3, Window,
        With,
    },
    time::Real,
    window::{CursorOptions, PrimaryWindow},
};

use crate::{
    camera::{AutoFly, FlyCamera, input_is_active, movement_axes},
    runtime::world::ClientWorld,
};

pub const MOVEMENT_TICKS_PER_SECOND: f64 = 20.0;
const MOVEMENT_TICK_SECONDS: f64 = 1.0 / MOVEMENT_TICKS_PER_SECOND;
pub const OUTBOX_CAPACITY: usize = 32;

#[allow(clippy::too_many_arguments)]
pub(crate) fn advance_local_physics(
    time: Res<Time<Real>>,
    window: Single<(&Window, &CursorOptions), With<PrimaryWindow>>,
    keys: Res<ButtonInput<KeyCode>>,
    auto_fly: Res<AutoFly>,
    client_world: Res<ClientWorld>,
    collisions: Res<PhysicsCollisionRegistries>,
    mut physics: ResMut<LocalPhysicsController>,
    mut camera: Single<&mut Transform, With<FlyCamera>>,
    mut previous_blocker: Local<Option<String>>,
) {
    if auto_fly.enabled() || !physics.is_active() {
        return;
    }
    let Some(stream) = client_world.stream.as_ref() else {
        return;
    };
    let (window, cursor) = window.into_inner();
    let active = input_is_active(window, cursor);
    let axes = if active {
        movement_axes(&keys)
    } else {
        Vec3::ZERO
    };
    let (bevy_yaw, _, _) = camera.rotation.to_euler(EulerRot::YXZ);
    let yaw = (180.0 - bevy_yaw.to_degrees()).rem_euclid(360.0);
    let input = physics_movement_input(
        [axes.x, axes.z],
        yaw,
        active,
        keys.pressed(KeyCode::Space),
        keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight),
        keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight),
    );
    let world = sim::PaletteWorld::new(
        stream.collision_store(),
        collisions.registry(stream.network_id_mode()),
        stream.current_dimension(),
    );
    let frame = physics.advance(time.delta(), input, &world);
    let blocker = frame.blocked.as_ref().map(ToString::to_string);
    if blocker != *previous_blocker {
        if let Some(blocker) = blocker.as_deref() {
            debug!(%blocker, "local physics is waiting for authoritative collision data");
        }
        *previous_blocker = blocker;
    }
    if let Some(position) = physics.render_eye_position() {
        camera.translation = Vec3::from_array(position);
    }
}

/// Origin of a movement sample and the authority allowed to transmit it.
///
/// The app currently has only an independent fly camera, so the safe default
/// is deliberately non-authoritative. Phase 3's physics simulation must opt in
/// explicitly before its samples may enter the outbound scheduler.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum MovementSource {
    #[default]
    FreeCamera,
    #[allow(dead_code, reason = "reserved for the Phase 3 physics authority")]
    Physics,
}

#[derive(Debug, PartialEq, Eq)]
pub enum MovementSendError<E> {
    Encode(PlayerAuthInputError),
    Transport(E),
    RestoreOverflow,
}

/// App input sampled at a deterministic Bedrock movement tick boundary.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MovementInputSample {
    pub position: [f32; 3],
    pub move_vector: [f32; 2],
    pub pitch: f32,
    pub yaw: f32,
    pub head_yaw: f32,
    pub camera_orientation: [f32; 3],
    pub jumping: bool,
    pub sneaking: bool,
    pub sprinting: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct HeldInput {
    jumping: bool,
    sneaking: bool,
    sprinting: bool,
}

impl From<MovementInputSample> for HeldInput {
    fn from(sample: MovementInputSample) -> Self {
        Self {
            jumping: sample.jumping,
            sneaking: sample.sneaking,
            sprinting: sample.sprinting,
        }
    }
}

/// Deterministic 20 Hz snapshot producer with a bounded retry FIFO.
///
/// It intentionally does not simulate movement. Phase 3's bedsim port will
/// provide the position and delta through the same [`MovementInputSample`]
/// seam. Free-camera samples are never queued or transmitted.
#[derive(Resource, Debug)]
pub struct MovementTicker {
    session_active: bool,
    source: MovementSource,
    session_generation: u64,
    next_tick: u64,
    accumulated_seconds: f64,
    previous_position: [f32; 3],
    previous_input: HeldInput,
    outbox: VecDeque<PlayerAuthInputSnapshot>,
    dropped_tick_count: u64,
}

impl Default for MovementTicker {
    fn default() -> Self {
        Self {
            session_active: false,
            source: MovementSource::default(),
            session_generation: 0,
            next_tick: 0,
            accumulated_seconds: 0.0,
            previous_position: [0.0; 3],
            previous_input: HeldInput::default(),
            outbox: VecDeque::with_capacity(OUTBOX_CAPACITY),
            dropped_tick_count: 0,
        }
    }
}

impl MovementTicker {
    pub fn reset(
        &mut self,
        session_generation: u64,
        initial_server_tick: u64,
        initial_position: [f32; 3],
    ) {
        self.session_active = true;
        self.session_generation = session_generation;
        self.next_tick = initial_server_tick.saturating_add(1);
        self.accumulated_seconds = 0.0;
        self.previous_position = initial_position;
        self.previous_input = HeldInput::default();
        self.outbox.clear();
        self.dropped_tick_count = 0;
    }

    pub fn deactivate(&mut self) {
        self.session_active = false;
        self.accumulated_seconds = 0.0;
        self.outbox.clear();
        self.previous_input = HeldInput::default();
    }

    /// Selects the source allowed to drive outbound movement.
    ///
    /// Changing authority always discards queued/history state so samples from
    /// the prior source cannot cross the boundary. The production app leaves
    /// this at [`MovementSource::FreeCamera`] until real physics exists.
    pub fn set_source(&mut self, source: MovementSource) {
        if self.source == source {
            return;
        }
        self.source = source;
        self.accumulated_seconds = 0.0;
        self.previous_input = HeldInput::default();
        self.outbox.clear();
    }

    pub fn apply_server_correction(&mut self, tick: u64, position: [f32; 3]) {
        if !self.session_active {
            return;
        }
        self.next_tick = self.next_tick.max(tick.saturating_add(1));
        self.reanchor_position(position);
    }

    pub fn reanchor_position(&mut self, position: [f32; 3]) {
        if !self.session_active {
            return;
        }
        self.accumulated_seconds = 0.0;
        self.previous_position = position;
        self.outbox.clear();
    }

    pub fn advance(
        &mut self,
        source: MovementSource,
        elapsed: Duration,
        sample: MovementInputSample,
    ) {
        if !self.can_transmit(source) {
            return;
        }
        self.accumulated_seconds += elapsed.as_secs_f64();
        let due = ((self.accumulated_seconds + f64::EPSILON) / MOVEMENT_TICK_SECONDS).floor();
        let due = due.clamp(0.0, u64::MAX as f64) as u64;
        self.accumulated_seconds -= due as f64 * MOVEMENT_TICK_SECONDS;
        let frame_start = self.previous_position;
        for tick_index in 1..=due {
            // A render frame may cover multiple Bedrock ticks. With only the
            // frame endpoints available, distribute its position change
            // uniformly so every emitted tick has a coherent position/delta
            // history. Rotation, movement axes, and held buttons intentionally
            // use the latest sample for all due ticks; edge flags still occur
            // only on the first tick through `previous_input`.
            let mut tick_sample = sample;
            tick_sample.position = if tick_index == due {
                sample.position
            } else {
                interpolate_position(frame_start, sample.position, tick_index, due)
            };
            let snapshot = self.snapshot(tick_sample);
            self.enqueue(snapshot);
        }
    }

    fn snapshot(&mut self, sample: MovementInputSample) -> PlayerAuthInputSnapshot {
        let current_input = HeldInput::from(sample);
        let move_vector = normalize_move_vector(sample.move_vector);
        let snapshot = PlayerAuthInputSnapshot {
            tick: self.next_tick,
            position: sample.position,
            delta: subtract(sample.position, self.previous_position),
            move_vector,
            analogue_move_vector: move_vector,
            raw_move_vector: sample.move_vector,
            pitch: sample.pitch,
            yaw: sample.yaw,
            head_yaw: sample.head_yaw,
            camera_orientation: sample.camera_orientation,
            flags: input_flags(sample, self.previous_input),
            input_mode: PlayerInputMode::Mouse,
        };
        self.next_tick = self.next_tick.saturating_add(1);
        self.previous_position = sample.position;
        self.previous_input = current_input;
        snapshot
    }

    fn enqueue(&mut self, snapshot: PlayerAuthInputSnapshot) {
        if self.outbox.len() == OUTBOX_CAPACITY {
            self.outbox.pop_front();
            self.dropped_tick_count = self.dropped_tick_count.saturating_add(1);
        }
        self.outbox.push_back(snapshot);
    }

    const fn can_transmit(&self, sample_source: MovementSource) -> bool {
        self.physics_is_authorized() && matches!(sample_source, MovementSource::Physics)
    }

    const fn physics_is_authorized(&self) -> bool {
        self.session_active && matches!(self.source, MovementSource::Physics)
    }

    #[must_use]
    pub fn pop_pending(&mut self) -> Option<PlayerAuthInputSnapshot> {
        self.outbox.pop_front()
    }

    pub fn retry_front(
        &mut self,
        snapshot: PlayerAuthInputSnapshot,
    ) -> Result<(), PlayerAuthInputSnapshot> {
        if !self.physics_is_authorized() || self.outbox.len() == OUTBOX_CAPACITY {
            return Err(snapshot);
        }
        self.outbox.push_front(snapshot);
        Ok(())
    }

    #[must_use]
    #[cfg(test)]
    #[allow(dead_code)]
    pub fn peek_pending(&self) -> Option<&PlayerAuthInputSnapshot> {
        self.outbox.front()
    }

    #[must_use]
    #[cfg(test)]
    #[allow(dead_code)]
    pub fn pending_count(&self) -> usize {
        self.outbox.len()
    }

    #[must_use]
    #[cfg(test)]
    #[allow(dead_code)]
    pub const fn session_generation(&self) -> u64 {
        self.session_generation
    }

    #[must_use]
    #[cfg(test)]
    #[allow(dead_code)]
    pub const fn dropped_tick_count(&self) -> u64 {
        self.dropped_tick_count
    }
}

pub fn flush_player_auth_inputs<E>(
    ticker: &mut MovementTicker,
    budget: usize,
    mut send: impl FnMut(Packet) -> Result<(), E>,
) -> Result<usize, MovementSendError<E>> {
    if !ticker.physics_is_authorized() {
        return Ok(0);
    }

    let mut sent = 0;
    for _ in 0..budget {
        let Some(snapshot) = ticker.pop_pending() else {
            break;
        };
        let packet = player_auth_input(snapshot).map_err(MovementSendError::Encode)?;
        if let Err(error) = send(packet) {
            ticker
                .retry_front(snapshot)
                .map_err(|_| MovementSendError::RestoreOverflow)?;
            return Err(MovementSendError::Transport(error));
        }
        sent += 1;
    }
    Ok(sent)
}

fn input_flags(sample: MovementInputSample, previous: HeldInput) -> PlayerInputFlags {
    let mut flags = PlayerInputFlags::NONE;
    if sample.move_vector[1] > 0.0 {
        flags |= PlayerInputFlags::UP;
    } else if sample.move_vector[1] < 0.0 {
        flags |= PlayerInputFlags::DOWN;
    }
    if sample.move_vector[0] < 0.0 {
        flags |= PlayerInputFlags::LEFT;
    } else if sample.move_vector[0] > 0.0 {
        flags |= PlayerInputFlags::RIGHT;
    }

    if sample.jumping {
        flags |= PlayerInputFlags::JUMP_DOWN
            | PlayerInputFlags::JUMPING
            | PlayerInputFlags::JUMP_CURRENT_RAW;
        if !previous.jumping {
            flags |= PlayerInputFlags::START_JUMPING | PlayerInputFlags::JUMP_PRESSED_RAW;
        }
    } else if previous.jumping {
        flags |= PlayerInputFlags::JUMP_RELEASED_RAW;
    }

    if sample.sneaking {
        flags |= PlayerInputFlags::SNEAKING | PlayerInputFlags::SNEAK_DOWN;
        if !previous.sneaking {
            flags |= PlayerInputFlags::START_SNEAKING | PlayerInputFlags::SNEAK_PRESSED_RAW;
        }
    } else if previous.sneaking {
        flags |= PlayerInputFlags::STOP_SNEAKING | PlayerInputFlags::SNEAK_RELEASED_RAW;
    }

    if sample.sprinting {
        flags |= PlayerInputFlags::SPRINT_DOWN | PlayerInputFlags::SPRINTING;
        if !previous.sprinting {
            flags |= PlayerInputFlags::START_SPRINTING;
        }
    } else if previous.sprinting {
        flags |= PlayerInputFlags::STOP_SPRINTING;
    }
    flags
}

fn subtract(lhs: [f32; 3], rhs: [f32; 3]) -> [f32; 3] {
    [lhs[0] - rhs[0], lhs[1] - rhs[1], lhs[2] - rhs[2]]
}

fn interpolate_position(
    start: [f32; 3],
    end: [f32; 3],
    numerator: u64,
    denominator: u64,
) -> [f32; 3] {
    debug_assert!(denominator > 0);
    let fraction = numerator as f64 / denominator as f64;
    std::array::from_fn(|axis| {
        (f64::from(start[axis]) + f64::from(end[axis] - start[axis]) * fraction) as f32
    })
}

fn normalize_move_vector(vector: [f32; 2]) -> [f32; 2] {
    let length_squared = vector[0].mul_add(vector[0], vector[1] * vector[1]);
    if length_squared > 1.0 {
        let inverse_length = length_squared.sqrt().recip();
        [vector[0] * inverse_length, vector[1] * inverse_length]
    } else {
        vector
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flush_refuses_a_stale_queue_without_physics_authority() {
        let mut ticker = MovementTicker::default();
        ticker.reset(1, 10, [0.0; 3]);
        ticker.set_source(MovementSource::Physics);
        ticker.advance(
            MovementSource::Physics,
            Duration::from_millis(50),
            MovementInputSample {
                position: [1.0, 2.0, 3.0],
                move_vector: [0.0; 2],
                pitch: 0.0,
                yaw: 0.0,
                head_yaw: 0.0,
                camera_orientation: [0.0, 0.0, 1.0],
                jumping: false,
                sneaking: false,
                sprinting: false,
            },
        );
        assert_eq!(ticker.outbox.len(), 1);

        // Simulate stale state surviving a future refactor so the flush guard
        // is verified independently from set_source's transition cleanup.
        ticker.source = MovementSource::FreeCamera;
        let mut sent_packets = 0;
        let flushed = flush_player_auth_inputs(&mut ticker, 8, |_packet| {
            sent_packets += 1;
            Ok::<_, ()>(())
        })
        .unwrap();

        assert_eq!(flushed, 0);
        assert_eq!(sent_packets, 0);
        assert_eq!(ticker.outbox.len(), 1);
    }
}

#[cfg(test)]
mod integration_tests;
