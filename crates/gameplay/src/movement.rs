use std::collections::VecDeque;

#[cfg(test)]
use protocol::PlayerInputMode;
use protocol::{PlayerAuthInputSnapshot, PlayerInputFlags};

mod anchor_probe;
mod anchor_probe_evidence;
mod authority;
mod collision_registries;
pub mod control_modes;
mod control_trace;
pub mod coordination;
mod correction_shape;
mod diagnostics;
pub mod diagnostics_config;
mod effects;
mod encoding;
mod input_state;

pub use input_state::TickInput;
mod evidence;
pub mod local_facts;
mod locomotion;
mod outbox;
mod physics;
mod prediction_sync;
mod respawn;
mod speed_authority;
mod state;
mod teleport_ack;
pub mod trace;
pub use authority::PhysicsSendIdentity;
pub use authority::{PhysicsAuthorityFault, PhysicsAuthorityFaultRecord, PhysicsAuthorityGate};
pub use collision_registries::{PhysicsCollisionRegistries, PhysicsCollisionRegistryError};
pub use control_trace::{trace_local_attributes, trace_server_control};
pub use coordination::physics_authority_fault_for_frame;
pub use correction_shape::CorrectionShape;
pub use correction_shape::{
    PhysicsAnchor, reconcile_candidate_physics_correction, reconcile_physics_anchor,
};
pub use correction_shape::{
    reconcile_committed_correction, reconcile_move_player_teleport,
    reconcile_prediction_correction, reconcile_timeline_rewind,
};
pub use diagnostics::{CorrectionKind, note_correction, note_motion};
pub use effects::{LocalMovementEffectTimeline, MiningEffects};
use encoding::{HeldInput, input_flags, normalize_move_vector};
use evidence::PhysicsTickSampleEvidence;
pub use evidence::{PhysicsTickEvidence, PhysicsTickEvidenceContext};
pub use locomotion::{ModeIntent, RideKind};
pub use outbox::MovementSendError;
pub use outbox::OUTBOX_CAPACITY;
#[cfg(any(test, feature = "test-support"))]
pub use outbox::flush_player_auth_inputs;
pub use outbox::{
    InteractionPacketGuard, MovementOutboxReconciliation, UnsentSampleView,
    flush_player_auth_inputs_guarded,
};
use physics::PhysicsCorrectionConfirmation;
pub use physics::{
    LocalPhysicsController, LocalPhysicsFrame, MAX_LOCAL_PHYSICS_TICKS_PER_FRAME,
    PhysicsCorrectionMode, PhysicsCorrectionOutcome, PhysicsMotionSample, PhysicsMovementSample,
    PhysicsSampleContext, physics_movement_input,
};
pub use prediction_sync::{PredictionSyncState, send_movement_prediction_sync};
use sim::WorldCollisionIdentity;
pub use speed_authority::LocalMovementSpeedAuthority;
pub use state::ProcessedMovementState;
pub use teleport_ack::ServerTeleportKind;
use tokio::sync::watch;
pub use trace::note_click_drop;
#[cfg(any(test, feature = "test-support"))]
pub use trace::trace_line_if;
pub use trace::{pending_trace_line, write_trace_line};

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

#[derive(Debug, Clone, PartialEq)]
struct QueuedPhysicsSample {
    session_generation: u64,
    snapshot: PlayerAuthInputSnapshot,
    displacement: [f32; 3],
    world_identity: WorldCollisionIdentity,
    evidence: PhysicsTickSampleEvidence,
    mining: Option<crate::mining::QueuedMiningInteraction>,
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
/// let _ticker = gameplay::movement::MovementTicker::default();
/// ```
#[derive(Debug, Clone)]
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
    remote_closed: bool,
    pending_fault: Option<PhysicsAuthorityFaultRecord>,
    next_admission_id: u64,
    reanchor_epoch: u64,
    terminal_drain: bool,
    pending_control_fence: bool,
    teleport_ack_enabled: bool,
    pending_teleport_ack: Option<teleport_ack::TeleportAckPending>,
    teleport_acks_expired: u64,
    replayed_corrections_observed: u64,
    unmarked_move_players_observed: u64,
    epoch_publisher: watch::Sender<u64>,
    mining_epoch_publisher: watch::Sender<u64>,
    held_release: Option<outbox::HeldRelease>,
}

#[cfg(test)]
impl Default for MovementTicker {
    fn default() -> Self {
        let (epoch_publisher, _epoch_receiver) = watch::channel(0);
        Self::with_epoch_publisher(epoch_publisher)
    }
}

impl MovementTicker {
    pub fn with_epoch_publisher(epoch_publisher: watch::Sender<u64>) -> Self {
        let (mining_epoch_publisher, _mining_epoch_receiver) = watch::channel(0);
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
            remote_closed: false,
            pending_fault: None,
            next_admission_id: 0,
            reanchor_epoch: 0,
            terminal_drain: false,
            pending_control_fence: false,
            teleport_ack_enabled: teleport_ack::enabled_from_env(),
            pending_teleport_ack: None,
            teleport_acks_expired: 0,
            replayed_corrections_observed: 0,
            unmarked_move_players_observed: 0,
            epoch_publisher,
            mining_epoch_publisher,
            held_release: None,
        }
    }

    pub fn reset(
        &mut self,

        session_generation: u64,
        initial_server_tick: u64,
        initial_position: [f32; 3],
    ) {
        self.position_authority_changed();
        self.held_release = None;
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
        self.remote_closed = false;
        self.pending_fault = None;
        self.next_admission_id = 0;
        self.terminal_drain = false;
        self.pending_control_fence = false;
        self.pending_teleport_ack = None;
    }

    pub fn deactivate(&mut self) {
        self.position_authority_changed();
        self.held_release = None;
        self.session_active = false;
        self.outbox.clear();
        self.pending_sends.clear();
        self.sent_history.clear();
        self.outbox_reconciliation = MovementOutboxReconciliation::NotAuthoritative;
        self.previous_input = HeldInput::default();
        self.terminal_drain = false;
        self.pending_control_fence = false;
        self.pending_teleport_ack = None;
    }

    /// Latches a remote-initiated close of an active, authorized physics
    /// session: [`Self::outbox_reconciliation`] then reports
    /// [`MovementOutboxReconciliation::RemoteClosed`] through teardown and the
    /// deactivated flush path until the next session reset.
    ///
    /// Local shutdowns, send-side failures, pre-session tickers, authority
    /// faults (which already demote the source), and FreeCamera sessions never
    /// latch this classification.
    pub fn note_remote_session_close(&mut self) {
        if self.session_active
            && matches!(self.source, MovementSource::Physics)
            && self.pending_fault.is_none()
        {
            self.remote_closed = true;
        }
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
        if matches!(source, MovementSource::FreeCamera) {
            self.clear_pending_teleport_ack();
        }
        self.terminal_drain = false;
        self.pending_control_fence = false;
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
            || !completed.movement.into_iter().all(f32::is_finite)
            || !completed.velocity.into_iter().all(f32::is_finite)
            || !completed.move_vector.into_iter().all(f32::is_finite)
            || !completed.raw_move_vector.into_iter().all(f32::is_finite)
            || !completed
                .analogue_move_vector
                .into_iter()
                .all(f32::is_finite)
            || !completed.camera_orientation.into_iter().all(f32::is_finite)
            || ![completed.pitch, completed.yaw, completed.head_yaw]
                .into_iter()
                .all(f32::is_finite)
        {
            let fault = PhysicsAuthorityFault::InvalidCompletedSample;
            self.fail_physics_authority(&fault);
            return Err(fault);
        }
        self.observe_admitted_tick_for_teleport_ack();
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
            displacement: completed.movement,
            world_identity: completed.world_identity,
            evidence,
            mining: None,
        });
        Ok(())
    }

    fn fail_physics_authority(&mut self, fault: &PhysicsAuthorityFault) {
        if self.pending_fault.is_none() {
            tracing::warn!(
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
        self.pending_teleport_ack = None;
    }

    fn snapshot(&mut self, sample: &PhysicsMovementSample) -> PlayerAuthInputSnapshot {
        let current_input = HeldInput::from(sample);
        // Samples carry right-positive x; the wire vectors are left-positive.
        let wire = |vector: [f32; 2]| [if vector[0] == 0.0 { 0.0 } else { -vector[0] }, vector[1]];
        let move_vector = encoding::wire_move_vector(sample.move_vector);
        let analogue_move_vector = wire(sample.analogue_move_vector);
        let raw_move_vector = if analogue_move_vector == [0.0; 2] {
            wire(normalize_move_vector(sample.raw_move_vector))
        } else {
            analogue_move_vector
        };
        let snapshot = PlayerAuthInputSnapshot {
            tick: self.next_tick,
            position: sample.position,
            // Vanilla sends the end-of-tick velocity as the position delta.
            delta: sample.velocity,
            move_vector,
            analogue_move_vector,
            raw_move_vector,
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

    pub const fn physics_is_authorized(&self) -> bool {
        self.session_active && matches!(self.source, MovementSource::Physics)
    }

    pub fn enforce_local_physics_authority(&self, local_physics: &mut LocalPhysicsController) {
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

    pub fn acknowledge_physics_send(&mut self, identity: PhysicsSendIdentity) -> bool {
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
            self.confirm_held_release_facing(identity.tick);
        }
        self.sent_physics_packet_count = self.sent_physics_packet_count.saturating_add(1);
        self.tick_evidence.push_back(pending.evidence);
        self.refresh_outbox_reconciliation();
        true
    }

    pub fn resolve_cancelled_physics_send(
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
    pub fn reanchor_surface_spawn(&mut self, tick: u64, position: [f32; 3]) {
        self.position_authority_changed();
        self.next_tick = self.next_tick.max(tick.saturating_add(1));
        self.previous_position = position;
        self.previous_input = HeldInput::default();
        self.outbox.clear();
        self.sent_history.clear();
        self.refresh_outbox_reconciliation();
    }

    pub fn begin_terminal_drain(&mut self) {
        if self.physics_is_authorized() {
            self.terminal_drain = true;
            self.refresh_outbox_reconciliation();
        }
    }

    pub fn accepting_physics_admissions(&self) -> bool {
        self.physics_is_authorized()
            && !self.terminal_drain
            && !self.has_unresolved_position_authority_change()
    }
    /// Pauses new simulation while previously completed inputs can still drain.
    pub fn set_control_fence_pending(&mut self, pending: bool) {
        self.pending_control_fence = pending;
    }

    pub fn can_advance_physics_frame(&self) -> bool {
        !self.pending_control_fence
            && self.accepting_physics_admissions()
            && self.pending_count()
                <= OUTBOX_CAPACITY.saturating_sub(MAX_LOCAL_PHYSICS_TICKS_PER_FRAME)
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
    #[cfg(any(test, feature = "test-support"))]
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

    /// Whether completed inputs still await admission to the network FIFO.
    pub fn has_unsent_inputs(&self) -> bool {
        !self.outbox.is_empty()
    }

    #[must_use]
    pub fn take_tick_evidence(&mut self) -> Vec<PhysicsTickEvidence> {
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
    pub const fn outbox_reconciliation(&self) -> MovementOutboxReconciliation {
        if self.remote_closed {
            MovementOutboxReconciliation::RemoteClosed
        } else {
            self.outbox_reconciliation
        }
    }

    pub fn note_full_restore(&mut self) {
        debug_assert_eq!(
            self.outbox_reconciliation,
            MovementOutboxReconciliation::TransportRestored
        );
        self.outbox_reconciliation = MovementOutboxReconciliation::FullRestored;
    }

    #[must_use]
    #[cfg(any(test, feature = "test-support"))]
    pub const fn next_tick(&self) -> u64 {
        self.next_tick
    }

    /// Last admitted movement tick, shared by held-use timing and its rendered animation.
    pub const fn completed_tick(&self) -> u64 {
        self.next_tick.saturating_sub(1)
    }

    #[must_use]
    pub fn take_authority_fault(&mut self) -> Option<PhysicsAuthorityFaultRecord> {
        self.pending_fault.take()
    }

    #[must_use]
    pub fn pending_authority_fault(&self) -> Option<&PhysicsAuthorityFaultRecord> {
        self.pending_fault.as_ref()
    }

    #[cfg(any(test, feature = "test-support"))]
    pub const fn reanchor_epoch(&self) -> u64 {
        self.reanchor_epoch
    }

    pub fn record_physics_fault(&mut self, fault: PhysicsAuthorityFault) {
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
                for pair in plan.replayed_samples.windows(2) {
                    let expected = pair[0].tick.saturating_add(1);
                    if pair[1].tick != expected {
                        return Err(PhysicsAuthorityFault::PendingTickMismatch {
                            expected,
                            actual: pair[1].tick,
                        });
                    }
                }

                let mut previous_input = plan.anchor_input;
                let rebuilt: Vec<_> = plan
                    .replayed_samples
                    .iter()
                    .map(|sample| {
                        let flags = input_flags(sample, previous_input);
                        previous_input = HeldInput::from(sample);
                        (sample, flags)
                    })
                    .collect();
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
                    let Some((replayed, flags)) =
                        rebuilt.iter().find(|(sample, _)| sample.tick == tick)
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
                    pending.displacement = replayed.movement;
                    pending.snapshot.move_vector = encoding::wire_move_vector(replayed.move_vector);
                    // Tick-bound actions survive; all movement flags come from replay.
                    pending.snapshot.flags = [
                        PlayerInputFlags::HANDLED_TELEPORT,
                        PlayerInputFlags::MISSED_SWING,
                        PlayerInputFlags::START_USING_ITEM,
                    ]
                    .into_iter()
                    .fold(*flags, |flags, bit| {
                        flags.with_mask(bit, pending.snapshot.flags.bits() & bit.bits() != 0)
                    });
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
                self.previous_input = previous_input;
                Ok(())
            }
        }
    }
}

/// Provisional local pre-first-input anchor: public protocol documentation does
/// not define vanilla's initial `PlayerInputTick`, and StartGame world age is a
/// separate clock domain.
const START_GAME_PREDICTION_ANCHOR_TICK: u64 = 0;

pub fn reset_start_game_prediction(
    movement: &mut MovementTicker,
    local_physics: &mut LocalPhysicsController,
    session_generation: u64,
    initial_position: [f32; 3],
) {
    movement.reset(
        session_generation,
        START_GAME_PREDICTION_ANCHOR_TICK,
        initial_position,
    );
    local_physics.reanchor_network_position_before_advance(
        initial_position,
        START_GAME_PREDICTION_ANCHOR_TICK,
        false,
    );
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod anchor_probe_tests;
#[cfg(test)]
mod correction_tests;
#[cfg(test)]
mod effects_tests;
#[cfg(test)]
mod integration_tests;
#[cfg(test)]
mod locomotion_tests;
#[cfg(test)]
mod settle_tests;
#[cfg(test)]
mod state_tests;
#[cfg(test)]
mod teleport_ack_tests;

#[cfg(test)]
mod zeqa_tests;

#[cfg(any(test, feature = "test-support"))]
pub use teleport_ack::TELEPORT_ACK_ADMITTED_TICK_BUDGET;

mod frame;
pub use frame::{LocomotionState, PhysicsFrameHold, PhysicsFrameInput, wire_head_yaw, wire_yaw};

#[cfg(test)]
mod input_state_tests;
