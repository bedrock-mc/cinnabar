use std::collections::VecDeque;

use bevy::prelude::Resource;
#[cfg(test)]
use protocol::PlayerInputMode;
use protocol::{
    Packet, PlayerAuthInputError, PlayerAuthInputSnapshot, PlayerInputFlags, player_auth_input,
};

mod authority;
mod effects;
mod encoding;
mod evidence;
mod physics;
mod runtime_system;
mod speed_authority;
mod trace;
pub use authority::{PhysicsAuthorityFault, PhysicsAuthorityGate};
pub(crate) use effects::LocalMovementEffectTimeline;
use encoding::{input_flags, normalize_move_vector};
use evidence::PhysicsTickSampleEvidence;
pub(crate) use evidence::{PhysicsTickEvidence, PhysicsTickEvidenceContext};
use physics::PhysicsCorrectionConfirmation;
pub use physics::{
    LocalPhysicsController, LocalPhysicsFrame, MAX_LOCAL_PHYSICS_TICKS_PER_FRAME,
    PhysicsCollisionRegistries, PhysicsCorrectionMode, PhysicsCorrectionOutcome,
    PhysicsMovementSample, PhysicsSampleContext, physics_movement_input,
};
pub(crate) use runtime_system::advance_local_physics;
use sim::{CollisionWorld, WorldCollisionIdentity};
pub(crate) use speed_authority::LocalMovementSpeedAuthority;
use tokio::sync::watch;
pub(crate) use trace::{pending_trace_line, write_trace_line};

pub const OUTBOX_CAPACITY: usize = 32;

/// Origin of a movement sample and the authority allowed to transmit it.
///
/// The pre-session default is deliberately non-authoritative. StartGame
/// selects production physics only after the collision registry and server
/// anchor are available.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum MovementSource {
    #[default]
    FreeCamera,
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
    pub(crate) reanchor_epoch: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
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
    retry_after_cancellation: bool,
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
///
/// Production construction requires the network-owned authority epoch
/// publisher, so a publisher-less ticker cannot silently skip invalidation.
///
/// ```compile_fail
/// let _ticker = bedrock_client::movement::MovementTicker::default();
/// ```
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
    reanchor_epoch: u64,
    terminal_drain: bool,
    epoch_publisher: watch::Sender<u64>,
}

#[cfg(test)]
impl Default for MovementTicker {
    fn default() -> Self {
        let (epoch_publisher, _epoch_receiver) = watch::channel(0);
        Self::with_epoch_publisher(epoch_publisher)
    }
}

impl MovementTicker {
    pub(crate) fn with_epoch_publisher(epoch_publisher: watch::Sender<u64>) -> Self {
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
            reanchor_epoch: 0,
            terminal_drain: false,
            epoch_publisher,
        }
    }

    pub fn reset(
        &mut self,

        session_generation: u64,
        initial_server_tick: u64,
        initial_position: [f32; 3],
    ) {
        self.position_authority_changed();
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
        self.position_authority_changed();
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
    /// the prior source cannot cross the boundary. Production StartGame
    /// explicitly selects [`MovementSource::Physics`]; `--freecam` and
    /// auto-fly acceptance explicitly retain [`MovementSource::FreeCamera`].
    pub fn set_source(&mut self, source: MovementSource) {
        if self.source == source {
            return;
        }
        self.position_authority_changed();
        self.source = source;
        self.previous_input = HeldInput::default();
        self.outbox.clear();
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
        self.position_authority_changed();
        self.next_tick = tick.saturating_add(1);
        self.previous_position = position;
        self.previous_input = HeldInput::default();
        self.outbox.clear();
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
            self.fail_physics_authority(&fault);
            return Err(fault);
        }
        if self.pending_count() == OUTBOX_CAPACITY {
            let fault = PhysicsAuthorityFault::OutboxOverflow;
            self.fail_physics_authority(&fault);
            return Err(fault);
        }
        if !completed.position.into_iter().all(f32::is_finite)
            || !completed.velocity.into_iter().all(f32::is_finite)
            || !completed.move_vector.into_iter().all(f32::is_finite)
            || !completed.camera_orientation.into_iter().all(f32::is_finite)
            || ![completed.pitch, completed.yaw, completed.head_yaw]
                .into_iter()
                .all(f32::is_finite)
        {
            let fault = PhysicsAuthorityFault::InvalidCompletedSample;
            self.fail_physics_authority(&fault);
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

    fn fail_physics_authority(&mut self, fault: &PhysicsAuthorityFault) {
        if self.pending_fault.is_none() {
            bevy::log::warn!(
                ?fault,
                session_generation = self.session_generation,
                next_tick = self.next_tick,
                pending_count = self.pending_count(),
                "local physics authority failed closed"
            );
            self.pending_fault = Some(PhysicsAuthorityFaultRecord {
                session_generation: self.session_generation,
                fault: fault.clone(),
                next_tick: self.next_tick,
                pending_count: self.pending_count(),
            });
        }
        self.position_authority_changed();
        self.source = MovementSource::FreeCamera;
        self.outbox.clear();
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
            delta: sample.velocity,
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

    pub(crate) fn enforce_local_physics_authority(
        &self,
        local_physics: &mut LocalPhysicsController,
    ) {
        if !self.physics_is_authorized() {
            local_physics.deactivate();
        }
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
            reanchor_epoch: self.reanchor_epoch,
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
            retry_after_cancellation: false,
        });
    }

    fn restore_admitted(
        &mut self,
        identity: PhysicsSendIdentity,
    ) -> Result<QueuedPhysicsSample, PhysicsAuthorityFault> {
        let pending = self
            .pending_sends
            .pop_back()
            .filter(|pending| pending.identity == identity)
            .ok_or(PhysicsAuthorityFault::PendingTickMismatch {
                expected: identity.tick,
                actual: self
                    .pending_sends
                    .back()
                    .map_or(0, |pending| pending.identity.tick),
            })?;
        self.next_admission_id = self.next_admission_id.saturating_sub(1);
        Ok(pending.sample)
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
        if self
            .pending_sends
            .front()
            .is_none_or(|pending| pending.identity != identity)
        {
            return false;
        }
        if self.tick_evidence.len() == OUTBOX_CAPACITY {
            self.pending_sends.pop_front();
            self.fail_physics_authority(&PhysicsAuthorityFault::OutboxOverflow);
            return false;
        }
        let pending = self
            .pending_sends
            .pop_front()
            .expect("matching pending socket acknowledgement was checked");
        if self.physics_is_authorized()
            && identity.session_generation == self.session_generation
            && identity.reanchor_epoch == self.reanchor_epoch
        {
            self.confirm_sent(&pending.sample);
        }
        self.sent_physics_packet_count = self.sent_physics_packet_count.saturating_add(1);
        self.tick_evidence.push_back(pending.evidence);
        self.refresh_outbox_reconciliation();
        true
    }

    pub(crate) fn resolve_cancelled_physics_send(
        &mut self,
        identity: PhysicsSendIdentity,
        definitely_unsent: bool,
    ) -> bool {
        if self
            .pending_sends
            .front()
            .is_none_or(|pending| pending.identity != identity)
        {
            return false;
        }
        if !definitely_unsent {
            self.pending_sends.pop_front();
            self.fail_physics_authority(&PhysicsAuthorityFault::IndeterminatePhysicsSend {
                tick: identity.tick,
            });
            return true;
        }
        let pending = self
            .pending_sends
            .pop_front()
            .expect("matching pending socket cancellation was checked");
        // A definitely-unsent replay remains required work even when terminal
        // drain has closed transmission. Restoring it keeps the existing
        // terminal deadline fail-closed instead of manufacturing `Drained`.
        if pending.retry_after_cancellation
            && self.physics_is_authorized()
            && pending.sample.session_generation == self.session_generation
            && self.retry_replayed_sample(pending.sample).is_err()
        {
            self.fail_physics_authority(&PhysicsAuthorityFault::OutboxOverflow);
            return true;
        }
        self.refresh_outbox_reconciliation();
        true
    }

    /// Reanchors movement without allowing queued pre-anchor commands or input
    /// edges to cross the new authoritative position.
    pub(crate) fn reanchor_surface_spawn(&mut self, tick: u64, position: [f32; 3]) {
        self.position_authority_changed();
        self.next_tick = self.next_tick.max(tick.saturating_add(1));
        self.previous_position = position;
        self.previous_input = HeldInput::default();
        self.outbox.clear();
        self.sent_history.clear();
        self.refresh_outbox_reconciliation();
    }

    pub(crate) fn begin_terminal_drain(&mut self) {
        if self.physics_is_authorized() {
            self.terminal_drain = true;
            self.refresh_outbox_reconciliation();
        }
    }

    pub(crate) fn accepting_physics_admissions(&self) -> bool {
        self.physics_is_authorized()
            && !self.terminal_drain
            && !self.has_unresolved_position_authority_change()
    }
    pub(crate) fn can_advance_physics_frame(&self) -> bool {
        self.accepting_physics_admissions()
            && self.pending_count()
                <= OUTBOX_CAPACITY.saturating_sub(MAX_LOCAL_PHYSICS_TICKS_PER_FRAME)
    }

    /// Records the explicit event that the server-authoritative local position
    /// changed. Every transport admission from the prior epoch becomes
    /// obsolete regardless of whether its packet fields happen to compare
    /// equal after reconciliation. Publishing here keeps worker invalidation
    /// inseparable from advancing the epoch.
    fn position_authority_changed(&mut self) {
        self.reanchor_epoch = self.reanchor_epoch.wrapping_add(1);
        for pending in &mut self.pending_sends {
            pending.retry_after_cancellation = false;
        }
        self.sent_history.clear();
        self.epoch_publisher.send_if_modified(|published| {
            if *published == self.reanchor_epoch {
                false
            } else {
                *published = self.reanchor_epoch;
                true
            }
        });
        self.refresh_outbox_reconciliation();
    }

    fn has_unresolved_position_authority_change(&self) -> bool {
        self.pending_sends
            .iter()
            .any(|pending| pending.identity.reanchor_epoch != self.reanchor_epoch)
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

    fn retry_replayed_sample(
        &mut self,
        sample: QueuedPhysicsSample,
    ) -> Result<(), Box<QueuedPhysicsSample>> {
        if !self.physics_is_authorized() || self.pending_count() == OUTBOX_CAPACITY {
            return Err(Box::new(sample));
        }
        let insertion = self
            .outbox
            .iter()
            .position(|queued| queued.snapshot.tick > sample.snapshot.tick)
            .unwrap_or(self.outbox.len());
        self.outbox.insert(insertion, sample);
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
    pub(crate) fn pending_authority_fault(&self) -> Option<&PhysicsAuthorityFaultRecord> {
        self.pending_fault.as_ref()
    }

    #[cfg(test)]
    pub(crate) const fn reanchor_epoch(&self) -> u64 {
        self.reanchor_epoch
    }

    pub(crate) fn record_physics_fault(&mut self, fault: PhysicsAuthorityFault) {
        self.fail_physics_authority(&fault);
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
                self.position_authority_changed();
                self.next_tick = plan.final_tick.saturating_add(1);
                self.previous_position = plan.final_position;
                self.previous_input = HeldInput::default();
                self.outbox.clear();
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

                let replay_sample = |pending: &mut QueuedPhysicsSample| {
                    if pending.session_generation != self.session_generation {
                        return Err(PhysicsAuthorityFault::PendingSessionMismatch {
                            expected: self.session_generation,
                            actual: pending.session_generation,
                        });
                    }
                    let tick = pending.snapshot.tick;
                    if tick <= plan.corrected_tick {
                        return Ok(None);
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
                    pending.snapshot.position = replayed.position;
                    pending.snapshot.delta = replayed.velocity;
                    pending.snapshot.flags = pending
                        .snapshot
                        .flags
                        .with_mask(
                            PlayerInputFlags::HORIZONTAL_COLLISION,
                            replayed.horizontal_collision,
                        )
                        .with_mask(
                            PlayerInputFlags::VERTICAL_COLLISION,
                            replayed.vertical_collision,
                        );
                    pending.evidence.network_position = replayed.position;
                    Ok(Some(()))
                };

                let mut replacement = VecDeque::with_capacity(self.outbox.len());
                for mut pending in self.outbox.drain(..) {
                    if replay_sample(&mut pending)?.is_none() {
                        continue;
                    }
                    replacement.push_back(pending);
                }
                self.outbox = replacement;
                for pending in &mut self.pending_sends {
                    let _ = replay_sample(&mut pending.sample)?;
                }

                self.position_authority_changed();
                for pending in &mut self.pending_sends {
                    pending.retry_after_cancellation =
                        pending.sample.snapshot.tick > plan.corrected_tick;
                }
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

    let apply_candidate = |mode| {
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
            })?;
        candidate_ticker.apply_correction_plan(&plan)?;
        Ok((candidate_ticker, candidate_physics, plan.outcome))
    };

    let mut result = apply_candidate(mode);
    if matches!(mode, PhysicsCorrectionMode::ReplayIfRetained)
        && matches!(
            result,
            Err(PhysicsAuthorityFault::CorrectionNotRetained { .. }
                | PhysicsAuthorityFault::CorrectionReplayFailed
                | PhysicsAuthorityFault::ReplayWorldIdentityMismatch { .. }
                | PhysicsAuthorityFault::PendingWorldIdentityMismatch { .. })
        )
    {
        // A delayed correction can outlive local history, and replaying from a
        // changed anchor or after a newly committed subchunk can legitimately
        // encounter different immutable chunk revisions. The server position
        // remains authoritative in each case, so discard speculative history
        // and continue from a current-tick snap instead of silently restoring
        // free-camera movement.
        result = apply_candidate(PhysicsCorrectionMode::Snap);
    }

    match result {
        Ok((candidate_ticker, candidate_physics, outcome)) => {
            *physics = candidate_physics;
            *ticker = candidate_ticker;
            Ok(outcome)
        }
        Err(fault) => {
            ticker.fail_physics_authority(&fault);
            physics.deactivate();
            Err(fault)
        }
    }
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
    if ticker.terminal_drain || ticker.has_unresolved_position_authority_change() {
        ticker.refresh_outbox_reconciliation();
        return Ok(0);
    }
    if !ticker.outbox.is_empty() && evidence_context.is_none() {
        return Err(MovementSendError::MissingEvidenceContext);
    }

    let mut sent = 0;
    for _ in 0..budget {
        if ticker.tick_evidence.len() == OUTBOX_CAPACITY {
            ticker.fail_physics_authority(&PhysicsAuthorityFault::OutboxOverflow);
            break;
        }
        let Some(sample) = ticker.pop_pending() else {
            break;
        };
        let packet = player_auth_input(sample.snapshot).map_err(MovementSendError::Encode)?;
        let identity = ticker.next_send_identity(&sample);
        ticker.note_command_admitted(
            identity,
            sample,
            evidence_context.expect("nonempty outbox requires staged evidence context"),
        );
        if let Err(error) = send(identity, packet) {
            let sample = ticker
                .restore_admitted(identity)
                .map_err(|_| MovementSendError::RestoreOverflow)?;
            ticker
                .retry_front(sample)
                .map_err(|_| MovementSendError::RestoreOverflow)?;
            ticker.outbox_reconciliation = MovementOutboxReconciliation::TransportRestored;
            return Err(MovementSendError::Transport(error));
        }
        sent += 1;
    }
    ticker.refresh_outbox_reconciliation();
    Ok(sent)
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod correction_tests;
#[cfg(test)]
mod effects_tests;
#[cfg(test)]
mod integration_tests;
