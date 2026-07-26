use std::{collections::VecDeque, time::Instant};

use bevy::prelude::Resource;
use protocol::{
    Packet, PlayerAuthInputError, PlayerAuthInputSnapshot, PlayerInputFlags, PlayerInputMode,
    player_auth_input,
};

mod authority;
mod evidence;
mod physics;
pub use authority::{PhysicsAuthorityFault, PhysicsAuthorityGate};
use evidence::PhysicsTickSampleEvidence;
pub(crate) use evidence::{PhysicsTickEvidence, PhysicsTickEvidenceContext};
use physics::PhysicsCorrectionConfirmation;
pub use physics::{
    LocalPhysicsController, LocalPhysicsFrame, MAX_LOCAL_PHYSICS_TICKS_PER_FRAME,
    PhysicsCollisionRegistries, PhysicsCorrectionMode, PhysicsCorrectionOutcome,
    PhysicsMovementSample, PhysicsSampleContext, physics_movement_input,
};
use sim::{CollisionWorld, WorldCollisionIdentity};

use bevy::{
    log::debug,
    prelude::{EulerRot, Local, Res, ResMut, Time, Vec3},
    time::Real,
};
use semantic_input::Action;

use crate::{
    acceptance::AcceptanceRun, camera::AutoFly, local_player::LocalViewPose,
    runtime::world::ClientWorld, semantic_controls::SemanticInputSnapshot,
};

pub const OUTBOX_CAPACITY: usize = 32;

#[allow(clippy::too_many_arguments)]
pub(crate) fn advance_local_physics(
    time: Res<Time<Real>>,
    input: Res<SemanticInputSnapshot>,
    auto_fly: Res<AutoFly>,
    client_world: Res<ClientWorld>,
    collisions: Res<PhysicsCollisionRegistries>,
    acceptance: Res<AcceptanceRun>,
    mut physics: ResMut<LocalPhysicsController>,
    mut movement_ticker: ResMut<MovementTicker>,
    mut view: ResMut<LocalViewPose>,
    mut previous_blocker: Local<Option<String>>,
) {
    if acceptance.deadline_reached(Instant::now()) {
        movement_ticker.begin_terminal_drain();
    }
    if auto_fly.enabled() || !physics.is_active() {
        return;
    }
    if !movement_ticker.accepting_physics_admissions() {
        return;
    }
    let Some(stream) = client_world.stream.as_ref() else {
        return;
    };
    let semantic = input.snapshot();
    let active = semantic.is_some();
    let input_mode = semantic.map_or(PlayerInputMode::Mouse, |snapshot| {
        match snapshot.input_mode {
            semantic_input::InputMode::KeyboardMouse => PlayerInputMode::Mouse,
            semantic_input::InputMode::GamePad => PlayerInputMode::GamePad,
            semantic_input::InputMode::Touch => PlayerInputMode::Touch,
        }
    });
    let movement = input.movement();
    let (bevy_yaw, bevy_pitch, _) = view.rotation().to_euler(EulerRot::YXZ);
    let yaw = (180.0 - bevy_yaw.to_degrees()).rem_euclid(360.0);
    let input = physics_movement_input(
        movement,
        yaw,
        active,
        input.phase(Action::Jump).held,
        input.phase(Action::Sneak).held,
        input.phase(Action::Sprint).held,
    );
    let world = sim::PaletteWorld::new(
        stream.collision_store(),
        collisions.registry(stream.network_id_mode()),
        stream.current_dimension(),
    );
    let frame = physics.advance_with_context(
        time.delta(),
        input,
        PhysicsSampleContext {
            pitch: -bevy_pitch.to_degrees(),
            head_yaw: yaw,
            camera_orientation: (view.rotation() * Vec3::NEG_Z).to_array(),
            input_mode,
        },
        &world,
    );
    let blocker = frame.blocked.as_ref().map(ToString::to_string);
    if blocker != *previous_blocker {
        if let Some(blocker) = blocker.as_deref() {
            debug!(%blocker, "local physics is waiting for authoritative collision data");
        }
        *previous_blocker = blocker;
    }
    if frame.dropped_ticks != 0 && movement_ticker.physics_is_authorized() {
        movement_ticker.record_physics_fault(PhysicsAuthorityFault::PhysicsTickOverflow {
            dropped: frame.dropped_ticks,
        });
        physics.deactivate();
        return;
    }
    for sample in frame.samples {
        if let Err(fault) = movement_ticker.enqueue_completed_physics(sample) {
            debug!(?fault, "candidate Physics movement authority failed closed");
            physics.deactivate();
            return;
        }
    }
    if let Some(position) = physics.render_eye_position() {
        view.set_eye_translation(Vec3::from_array(position));
    }
}

/// Origin of a movement sample and the authority allowed to transmit it.
///
/// The safe production default is deliberately non-authoritative. Local
/// prediction and perspective changes cannot opt in implicitly; Phase 3's
/// physics authority must be enabled explicitly before samples may enter the
/// outbound scheduler.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum MovementSource {
    #[default]
    FreeCamera,
    #[allow(dead_code, reason = "reserved for the Phase 3 physics authority")]
    Physics,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum MovementOutboxReconciliation {
    #[default]
    NotAuthoritative,
    Drained,
    SocketPending,
    BudgetDeferred,
    TransportRestored,
    FullRestored,
}

impl MovementOutboxReconciliation {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::NotAuthoritative => "NotAuthoritative",
            Self::Drained => "Drained",
            Self::SocketPending => "SocketPending",
            Self::BudgetDeferred => "BudgetDeferred",
            Self::TransportRestored => "TransportRestored",
            Self::FullRestored => "FullRestored",
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum MovementSendError<E> {
    Encode(PlayerAuthInputError),
    Transport(E),
    RestoreOverflow,
    MissingEvidenceContext,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PhysicsSendIdentity {
    pub(crate) session_generation: u64,
    pub(crate) tick: u64,
    pub(crate) admission_id: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PhysicsAuthorityFaultRecord {
    pub session_generation: u64,
    pub fault: PhysicsAuthorityFault,
    pub next_tick: u64,
    pub pending_count: usize,
}

#[derive(Debug, Clone, PartialEq)]
struct QueuedPhysicsSample {
    session_generation: u64,
    snapshot: PlayerAuthInputSnapshot,
    world_identity: WorldCollisionIdentity,
    evidence: PhysicsTickSampleEvidence,
}

#[derive(Debug, Clone, PartialEq)]
struct SentPhysicsSample {
    session_generation: u64,
    tick: u64,
    position: [f32; 3],
    world_identity: WorldCollisionIdentity,
}

#[derive(Debug, Clone, PartialEq)]
struct PendingPhysicsSend {
    identity: PhysicsSendIdentity,
    sample: QueuedPhysicsSample,
    evidence: PhysicsTickEvidence,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct HeldInput {
    jumping: bool,
    sneaking: bool,
    sprinting: bool,
}

impl From<&PhysicsMovementSample> for HeldInput {
    fn from(sample: &PhysicsMovementSample) -> Self {
        Self {
            jumping: sample.jumping,
            sneaking: sample.sneaking,
            sprinting: sample.sprinting,
        }
    }
}

/// Bounded retry FIFO for completed, fixed-tick physics samples.
///
/// There is intentionally no render-frame interpolation/enqueue path here:
/// only a completed simulator tick carrying immutable collision identity may
/// become a `PlayerAuthInput` candidate.
#[derive(Resource, Debug, Clone)]
pub struct MovementTicker {
    session_active: bool,
    source: MovementSource,
    session_generation: u64,
    next_tick: u64,
    previous_position: [f32; 3],
    previous_input: HeldInput,
    outbox: VecDeque<QueuedPhysicsSample>,
    pending_sends: VecDeque<PendingPhysicsSend>,
    sent_history: VecDeque<SentPhysicsSample>,
    tick_evidence: VecDeque<PhysicsTickEvidence>,
    dropped_tick_count: u64,
    sent_free_camera_packet_count: u64,
    sent_physics_packet_count: u64,
    outbox_reconciliation: MovementOutboxReconciliation,
    pending_fault: Option<PhysicsAuthorityFaultRecord>,
    next_admission_id: u64,
    terminal_drain: bool,
}

impl Default for MovementTicker {
    fn default() -> Self {
        Self {
            session_active: false,
            source: MovementSource::default(),
            session_generation: 0,
            next_tick: 0,
            previous_position: [0.0; 3],
            previous_input: HeldInput::default(),
            outbox: VecDeque::with_capacity(OUTBOX_CAPACITY),
            pending_sends: VecDeque::with_capacity(OUTBOX_CAPACITY),
            sent_history: VecDeque::with_capacity(OUTBOX_CAPACITY),
            tick_evidence: VecDeque::with_capacity(OUTBOX_CAPACITY),
            dropped_tick_count: 0,
            sent_free_camera_packet_count: 0,
            sent_physics_packet_count: 0,
            outbox_reconciliation: MovementOutboxReconciliation::NotAuthoritative,
            pending_fault: None,
            next_admission_id: 0,
            terminal_drain: false,
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
        self.previous_position = initial_position;
        self.previous_input = HeldInput::default();
        self.outbox.clear();
        self.pending_sends.clear();
        self.sent_history.clear();
        self.tick_evidence.clear();
        self.dropped_tick_count = 0;
        self.sent_free_camera_packet_count = 0;
        self.sent_physics_packet_count = 0;
        self.outbox_reconciliation = MovementOutboxReconciliation::NotAuthoritative;
        self.pending_fault = None;
        self.next_admission_id = 0;
        self.terminal_drain = false;
    }

    pub fn deactivate(&mut self) {
        self.session_active = false;
        self.outbox.clear();
        self.pending_sends.clear();
        self.sent_history.clear();
        self.outbox_reconciliation = MovementOutboxReconciliation::NotAuthoritative;
        self.previous_input = HeldInput::default();
        self.terminal_drain = false;
    }

    /// Selects the source allowed to drive outbound movement.
    ///
    /// Changing authority always discards queued/history state so samples from
    /// the prior source cannot cross the boundary. The production app leaves
    /// this at [`MovementSource::FreeCamera`] until the server-authoritative
    /// gate is explicitly enabled.
    pub fn set_source(&mut self, source: MovementSource) {
        if self.source == source {
            return;
        }
        self.source = source;
        self.previous_input = HeldInput::default();
        self.outbox.clear();
        self.pending_sends.clear();
        self.sent_history.clear();
        self.outbox_reconciliation = match source {
            MovementSource::Physics => MovementOutboxReconciliation::Drained,
            MovementSource::FreeCamera => MovementOutboxReconciliation::NotAuthoritative,
        };
        self.terminal_drain = false;
    }

    pub fn snap_non_authoritative_anchor(&mut self, tick: u64, position: [f32; 3]) {
        if !self.session_active {
            return;
        }
        self.next_tick = tick.saturating_add(1);
        self.previous_position = position;
        self.previous_input = HeldInput::default();
        self.outbox.clear();
        self.pending_sends.clear();
        self.sent_history.clear();
    }

    pub fn enqueue_completed_physics(
        &mut self,
        completed: PhysicsMovementSample,
    ) -> Result<(), PhysicsAuthorityFault> {
        if !self.accepting_physics_admissions() {
            return Err(PhysicsAuthorityFault::Unauthorized);
        }
        if completed.tick != self.next_tick {
            let fault = PhysicsAuthorityFault::TickMismatch {
                expected: self.next_tick,
                actual: completed.tick,
            };
            self.fail_physics_authority(fault);
            return Err(fault);
        }
        if self.pending_count() == OUTBOX_CAPACITY {
            let fault = PhysicsAuthorityFault::OutboxOverflow;
            self.fail_physics_authority(fault);
            return Err(fault);
        }
        if !completed.position.into_iter().all(f32::is_finite)
            || !completed.move_vector.into_iter().all(f32::is_finite)
            || !completed.camera_orientation.into_iter().all(f32::is_finite)
            || ![completed.pitch, completed.yaw, completed.head_yaw]
                .into_iter()
                .all(f32::is_finite)
        {
            let fault = PhysicsAuthorityFault::InvalidCompletedSample;
            self.fail_physics_authority(fault);
            return Err(fault);
        }
        let snapshot = self.snapshot(&completed);
        let jump_started = snapshot.flags.bits() & PlayerInputFlags::START_JUMPING.bits() != 0
            || completed.jump_repeated;
        let evidence = PhysicsTickSampleEvidence {
            session_generation: self.session_generation,
            tick: snapshot.tick,
            network_position: snapshot.position,
            input_mode: snapshot.input_mode,
            movement: snapshot.move_vector,
            jump_held: snapshot.flags.bits() & PlayerInputFlags::JUMP_DOWN.bits() != 0,
            grounded_before_tick: completed.grounded_before_tick,
            grounded_after_tick: completed.grounded_after_tick,
            jump_started,
            jump_repeated: completed.jump_repeated,
            jump_released: snapshot.flags.bits() & PlayerInputFlags::JUMP_RELEASED_RAW.bits() != 0,
        };
        self.outbox.push_back(QueuedPhysicsSample {
            session_generation: self.session_generation,
            snapshot,
            world_identity: completed.world_identity,
            evidence,
        });
        Ok(())
    }

    fn fail_physics_authority(&mut self, fault: PhysicsAuthorityFault) {
        if self.pending_fault.is_none() {
            self.pending_fault = Some(PhysicsAuthorityFaultRecord {
                session_generation: self.session_generation,
                fault,
                next_tick: self.next_tick,
                pending_count: self.pending_count(),
            });
        }
        self.source = MovementSource::FreeCamera;
        self.outbox.clear();
        self.pending_sends.clear();
        self.sent_history.clear();
        self.outbox_reconciliation = MovementOutboxReconciliation::NotAuthoritative;
        self.previous_input = HeldInput::default();
    }

    fn snapshot(&mut self, sample: &PhysicsMovementSample) -> PlayerAuthInputSnapshot {
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
            input_mode: sample.input_mode,
        };
        self.next_tick = self.next_tick.saturating_add(1);
        self.previous_position = sample.position;
        self.previous_input = current_input;
        snapshot
    }

    pub(crate) const fn physics_is_authorized(&self) -> bool {
        self.session_active && matches!(self.source, MovementSource::Physics)
    }

    #[must_use]
    fn pop_pending(&mut self) -> Option<QueuedPhysicsSample> {
        self.outbox.pop_front()
    }

    fn sent_confirmation(&self, tick: u64) -> Option<PhysicsCorrectionConfirmation> {
        self.sent_history
            .iter()
            .rev()
            .find(|sample| {
                sample.session_generation == self.session_generation && sample.tick == tick
            })
            .map(|sample| PhysicsCorrectionConfirmation {
                position: sample.position,
                world_identity: sample.world_identity.clone(),
            })
    }

    fn next_send_identity(&self, sample: &QueuedPhysicsSample) -> PhysicsSendIdentity {
        PhysicsSendIdentity {
            session_generation: sample.session_generation,
            tick: sample.snapshot.tick,
            admission_id: self.next_admission_id,
        }
    }

    fn note_command_admitted(
        &mut self,
        identity: PhysicsSendIdentity,
        sample: QueuedPhysicsSample,
        context: PhysicsTickEvidenceContext,
    ) {
        debug_assert!(self.pending_sends.len() < OUTBOX_CAPACITY);
        self.next_admission_id = self.next_admission_id.saturating_add(1);
        let staged = sample.evidence;
        let evidence = PhysicsTickEvidence {
            session_generation: staged.session_generation,
            tick: staged.tick,
            network_position: staged.network_position,
            input_mode: staged.input_mode,
            movement: staged.movement,
            jump_held: staged.jump_held,
            grounded_before_tick: staged.grounded_before_tick,
            grounded_after_tick: staged.grounded_after_tick,
            jump_started: staged.jump_started,
            jump_repeated: staged.jump_repeated,
            jump_released: staged.jump_released,
            context,
        };
        self.pending_sends.push_back(PendingPhysicsSend {
            identity,
            sample,
            evidence,
        });
    }

    fn confirm_sent(&mut self, sample: &QueuedPhysicsSample) {
        if self.sent_history.len() == OUTBOX_CAPACITY {
            self.sent_history.pop_front();
        }
        self.sent_history.push_back(SentPhysicsSample {
            session_generation: sample.session_generation,
            tick: sample.snapshot.tick,
            position: sample.snapshot.position,
            world_identity: sample.world_identity.clone(),
        });
    }

    pub(crate) fn acknowledge_physics_send(&mut self, identity: PhysicsSendIdentity) -> bool {
        if !self.physics_is_authorized() || identity.session_generation != self.session_generation {
            return false;
        }
        if self
            .pending_sends
            .front()
            .is_none_or(|pending| pending.identity != identity)
        {
            return false;
        }
        if self.tick_evidence.len() == OUTBOX_CAPACITY {
            self.fail_physics_authority(PhysicsAuthorityFault::OutboxOverflow);
            return false;
        }
        let pending = self
            .pending_sends
            .pop_front()
            .expect("matching pending socket acknowledgement was checked");
        self.confirm_sent(&pending.sample);
        self.sent_physics_packet_count = self.sent_physics_packet_count.saturating_add(1);
        let mut evidence = pending.evidence;
        evidence.network_position = pending.sample.snapshot.position;
        self.tick_evidence.push_back(evidence);
        self.refresh_outbox_reconciliation();
        true
    }

    /// Reanchors movement without allowing queued pre-anchor commands or input
    /// edges to cross the new authoritative position.
    pub(crate) fn reanchor_surface_spawn(&mut self, tick: u64, position: [f32; 3]) {
        self.next_tick = self.next_tick.max(tick.saturating_add(1));
        self.previous_position = position;
        self.previous_input = HeldInput::default();
        self.outbox.clear();
        self.pending_sends.clear();
        self.sent_history.clear();
        self.refresh_outbox_reconciliation();
    }

    pub(crate) fn begin_terminal_drain(&mut self) {
        if self.physics_is_authorized() {
            self.terminal_drain = true;
            self.refresh_outbox_reconciliation();
        }
    }

    pub(crate) const fn accepting_physics_admissions(&self) -> bool {
        self.physics_is_authorized() && !self.terminal_drain
    }

    fn refresh_outbox_reconciliation(&mut self) {
        if !self.physics_is_authorized() {
            self.outbox_reconciliation = MovementOutboxReconciliation::NotAuthoritative;
        } else if !self.outbox.is_empty() {
            self.outbox_reconciliation = MovementOutboxReconciliation::BudgetDeferred;
        } else if !self.pending_sends.is_empty() {
            self.outbox_reconciliation = MovementOutboxReconciliation::SocketPending;
        } else {
            self.outbox_reconciliation = MovementOutboxReconciliation::Drained;
        }
    }

    fn retry_front(&mut self, sample: QueuedPhysicsSample) -> Result<(), Box<QueuedPhysicsSample>> {
        if !self.physics_is_authorized() || self.outbox.len() == OUTBOX_CAPACITY {
            return Err(Box::new(sample));
        }
        self.outbox.push_front(sample);
        Ok(())
    }

    #[must_use]
    #[cfg(test)]
    #[allow(dead_code)]
    fn peek_pending(&self) -> Option<&QueuedPhysicsSample> {
        self.outbox.front()
    }

    #[must_use]
    #[cfg(test)]
    pub fn pending_snapshots(&self) -> Vec<PlayerAuthInputSnapshot> {
        self.outbox.iter().map(|sample| sample.snapshot).collect()
    }

    #[must_use]
    #[cfg(test)]
    fn pending_samples(&self) -> Vec<QueuedPhysicsSample> {
        self.outbox.iter().cloned().collect()
    }

    #[must_use]
    pub fn pending_count(&self) -> usize {
        self.outbox.len().saturating_add(self.pending_sends.len())
    }

    #[must_use]
    pub(crate) fn take_tick_evidence(&mut self) -> Vec<PhysicsTickEvidence> {
        self.tick_evidence.drain(..).collect()
    }

    #[must_use]
    pub const fn session_generation(&self) -> u64 {
        self.session_generation
    }

    #[must_use]
    pub const fn source(&self) -> MovementSource {
        self.source
    }

    #[must_use]
    pub const fn dropped_tick_count(&self) -> u64 {
        self.dropped_tick_count
    }

    #[must_use]
    pub const fn sent_free_camera_packet_count(&self) -> u64 {
        self.sent_free_camera_packet_count
    }

    #[must_use]
    pub const fn sent_physics_packet_count(&self) -> u64 {
        self.sent_physics_packet_count
    }

    #[must_use]
    pub(crate) const fn outbox_reconciliation(&self) -> MovementOutboxReconciliation {
        self.outbox_reconciliation
    }

    pub(crate) fn note_full_restore(&mut self) {
        debug_assert_eq!(
            self.outbox_reconciliation,
            MovementOutboxReconciliation::TransportRestored
        );
        self.outbox_reconciliation = MovementOutboxReconciliation::FullRestored;
    }

    #[must_use]
    #[cfg(test)]
    pub const fn next_tick(&self) -> u64 {
        self.next_tick
    }

    #[must_use]
    pub fn take_authority_fault(&mut self) -> Option<PhysicsAuthorityFaultRecord> {
        self.pending_fault.take()
    }

    #[must_use]
    pub(crate) const fn pending_authority_fault(&self) -> Option<PhysicsAuthorityFaultRecord> {
        self.pending_fault
    }

    pub(crate) fn record_physics_fault(&mut self, fault: PhysicsAuthorityFault) {
        self.fail_physics_authority(fault);
    }

    fn apply_correction_plan(
        &mut self,
        plan: &physics::PhysicsCorrectionPlan,
    ) -> Result<(), PhysicsAuthorityFault> {
        if !self.physics_is_authorized() {
            return Err(PhysicsAuthorityFault::Unauthorized);
        }
        match plan.outcome {
            PhysicsCorrectionOutcome::Snapped { .. } => {
                self.next_tick = plan.final_tick.saturating_add(1);
                self.previous_position = plan.final_position;
                self.previous_input = HeldInput::default();
                self.outbox.clear();
                self.pending_sends.clear();
                self.sent_history.clear();
                Ok(())
            }
            PhysicsCorrectionOutcome::Replayed { .. } => {
                let expected_next = plan.final_tick.saturating_add(1);
                if self.next_tick != expected_next {
                    return Err(PhysicsAuthorityFault::PendingTickMismatch {
                        expected: expected_next,
                        actual: self.next_tick,
                    });
                }
                if plan.replayed_samples.len() > OUTBOX_CAPACITY {
                    return Err(PhysicsAuthorityFault::OutboxOverflow);
                }
                for pair in plan.replayed_samples.windows(2) {
                    let expected = pair[0].tick.saturating_add(1);
                    if pair[1].tick != expected {
                        return Err(PhysicsAuthorityFault::PendingTickMismatch {
                            expected,
                            actual: pair[1].tick,
                        });
                    }
                }

                let mut replacement = VecDeque::with_capacity(self.outbox.len());
                for mut pending in self.outbox.drain(..) {
                    if pending.session_generation != self.session_generation {
                        return Err(PhysicsAuthorityFault::PendingSessionMismatch {
                            expected: self.session_generation,
                            actual: pending.session_generation,
                        });
                    }
                    let tick = pending.snapshot.tick;
                    if tick <= plan.corrected_tick {
                        continue;
                    }
                    let Some(replayed) = plan
                        .replayed_samples
                        .iter()
                        .find(|sample| sample.tick == tick)
                    else {
                        return Err(PhysicsAuthorityFault::PendingTickMismatch {
                            expected: tick,
                            actual: plan.final_tick,
                        });
                    };
                    if pending.world_identity != replayed.world_identity {
                        return Err(PhysicsAuthorityFault::PendingWorldIdentityMismatch { tick });
                    }
                    let previous_position = if tick == plan.corrected_tick.saturating_add(1) {
                        plan.corrected_position
                    } else {
                        let expected_previous = tick.saturating_sub(1);
                        let Some(previous) = plan
                            .replayed_samples
                            .iter()
                            .find(|sample| sample.tick == expected_previous)
                        else {
                            return Err(PhysicsAuthorityFault::PendingTickMismatch {
                                expected: expected_previous,
                                actual: tick,
                            });
                        };
                        previous.position
                    };
                    pending.snapshot.position = replayed.position;
                    pending.snapshot.delta = subtract(replayed.position, previous_position);
                    replacement.push_back(pending);
                }
                self.outbox = replacement;
                self.previous_position = plan.final_position;
                Ok(())
            }
        }
    }
}

pub fn reconcile_candidate_physics_correction(
    ticker: &mut MovementTicker,
    physics: &mut LocalPhysicsController,
    network_position: [f32; 3],
    tick: u64,
    on_ground: bool,
    mode: PhysicsCorrectionMode,
    world: &impl CollisionWorld,
) -> Result<PhysicsCorrectionOutcome, PhysicsAuthorityFault> {
    if !ticker.physics_is_authorized() {
        return Err(PhysicsAuthorityFault::Unauthorized);
    }
    let aligned_tick = match mode {
        PhysicsCorrectionMode::ReplayIfRetained => tick,
        PhysicsCorrectionMode::Snap => ticker
            .next_tick
            .max(tick.saturating_add(1))
            .saturating_sub(1),
    };
    let mut candidate_physics = physics.clone();
    let mut candidate_ticker = ticker.clone();
    let confirmation = candidate_ticker.sent_confirmation(aligned_tick);
    let plan = candidate_physics
        .apply_correction(
            network_position,
            aligned_tick,
            on_ground,
            mode,
            confirmation.as_ref(),
            world,
        )
        .map_err(|error| match error {
            physics::PhysicsCorrectionError::InvalidAnchor
            | physics::PhysicsCorrectionError::ReplayFailed => {
                PhysicsAuthorityFault::CorrectionReplayFailed
            }
            physics::PhysicsCorrectionError::NotRetained { tick } => {
                PhysicsAuthorityFault::CorrectionNotRetained { tick }
            }
            physics::PhysicsCorrectionError::WorldIdentityMismatch { tick } => {
                PhysicsAuthorityFault::ReplayWorldIdentityMismatch { tick }
            }
        });
    let result = plan.and_then(|plan| {
        candidate_ticker.apply_correction_plan(&plan)?;
        let outcome = plan.outcome;
        *physics = candidate_physics;
        *ticker = candidate_ticker;
        Ok(outcome)
    });
    if let Err(fault) = result {
        ticker.fail_physics_authority(fault);
        physics.deactivate();
    }
    result
}

pub(crate) fn flush_player_auth_inputs<E>(
    ticker: &mut MovementTicker,
    budget: usize,
    evidence_context: Option<PhysicsTickEvidenceContext>,
    mut send: impl FnMut(PhysicsSendIdentity, Packet) -> Result<(), E>,
) -> Result<usize, MovementSendError<E>> {
    if !ticker.physics_is_authorized() {
        ticker.outbox_reconciliation = MovementOutboxReconciliation::NotAuthoritative;
        return Ok(0);
    }
    if ticker.terminal_drain {
        ticker.refresh_outbox_reconciliation();
        return Ok(0);
    }
    if !ticker.outbox.is_empty() && evidence_context.is_none() {
        return Err(MovementSendError::MissingEvidenceContext);
    }

    let mut sent = 0;
    for _ in 0..budget {
        if ticker.tick_evidence.len() == OUTBOX_CAPACITY {
            ticker.fail_physics_authority(PhysicsAuthorityFault::OutboxOverflow);
            break;
        }
        let Some(sample) = ticker.pop_pending() else {
            break;
        };
        let packet = player_auth_input(sample.snapshot).map_err(MovementSendError::Encode)?;
        let identity = ticker.next_send_identity(&sample);
        if let Err(error) = send(identity, packet) {
            ticker
                .retry_front(sample)
                .map_err(|_| MovementSendError::RestoreOverflow)?;
            ticker.outbox_reconciliation = MovementOutboxReconciliation::TransportRestored;
            return Err(MovementSendError::Transport(error));
        }
        ticker.note_command_admitted(
            identity,
            sample,
            evidence_context.expect("nonempty outbox requires staged evidence context"),
        );
        sent += 1;
    }
    ticker.refresh_outbox_reconciliation();
    Ok(sent)
}

fn input_flags(sample: &PhysicsMovementSample, previous: HeldInput) -> PlayerInputFlags {
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
        ticker
            .enqueue_completed_physics(PhysicsMovementSample {
                tick: 11,
                position: [1.0, 2.0, 3.0],
                move_vector: [0.0; 2],
                pitch: 0.0,
                yaw: 0.0,
                head_yaw: 0.0,
                camera_orientation: [0.0, 0.0, 1.0],
                jumping: false,
                sneaking: false,
                sprinting: false,
                input_mode: PlayerInputMode::Mouse,
                grounded_before_tick: false,
                grounded_after_tick: false,
                jump_repeated: false,
                world_identity: WorldCollisionIdentity::new(
                    sim::CollisionRegistryIdentity {
                        protocol: 1001,
                        id_space: sim::CollisionIdSpace::Sequential,
                        preg_sha256: [1; 32],
                    },
                    [],
                )
                .unwrap(),
            })
            .unwrap();
        assert_eq!(ticker.outbox.len(), 1);

        // Simulate stale state surviving a future refactor so the flush guard
        // is verified independently from set_source's transition cleanup.
        ticker.source = MovementSource::FreeCamera;
        let mut sent_packets = 0;
        let flushed = flush_player_auth_inputs(&mut ticker, 8, None, |_identity, _packet| {
            sent_packets += 1;
            Ok::<_, ()>(())
        })
        .unwrap();

        assert_eq!(flushed, 0);
        assert_eq!(sent_packets, 0);
        assert_eq!(ticker.sent_free_camera_packet_count(), 0);
        assert_eq!(ticker.outbox.len(), 1);
    }
}

#[cfg(test)]
mod correction_tests;
#[cfg(test)]
mod integration_tests;
