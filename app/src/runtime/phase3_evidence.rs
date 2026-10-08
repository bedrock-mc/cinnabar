//! Converts gameplay observations into the optional evidence plugin's input records.
#[cfg(test)]
use crate::movement::{MovementOutboxReconciliation, MovementSource};
use crate::{
    acceptance::{AcceptanceRun, mutation::write_stdout_marker},
    movement::{
        MovementTicker, OUTBOX_CAPACITY, PhysicsAuthorityFault, PhysicsAuthorityFaultRecord,
        PhysicsCollisionRegistries, PhysicsCorrectionOutcome, PhysicsTickEvidence,
    },
};
#[cfg(test)]
pub(crate) use acceptance::phase3_evidence::{
    MAX_PHASE3_EVENT_RECORDS, MAX_PHASE3_FAULT_RECORDS, MAX_PHASE3_FRAME_RECORDS,
    Phase3EvidenceIdentity, validate_phase3_build_source,
};
pub(crate) use acceptance::phase3_evidence::{
    Phase3EvidenceEventKind, Phase3EvidenceFrame, Phase3EvidenceIdentityError,
};
use bevy::prelude::{Res, ResMut, Resource};
use semantic_input::InputMode;

/// Owns the emitter while translating gameplay's existing domain records at the boundary.
#[derive(Resource, Default)]
pub(crate) struct Phase3EvidenceEmitter(acceptance::phase3_evidence::Phase3EvidenceEmitter);
impl std::ops::Deref for Phase3EvidenceEmitter {
    type Target = acceptance::phase3_evidence::Phase3EvidenceEmitter;
    /// Exposes the evidence API while the app owns domain-to-observation conversion.
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl std::ops::DerefMut for Phase3EvidenceEmitter {
    /// Lets runtime observations update the evidence owner directly.
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
impl Phase3EvidenceEmitter {
    /// Sends one completed frame with gameplay's authoritative outbox bound.
    pub(crate) fn observe(&mut self, frame: Phase3EvidenceFrame) -> Vec<String> {
        self.0.observe(frame, OUTBOX_CAPACITY)
    }
    /// Converts the correction result without transferring gameplay ownership.
    pub(crate) fn note_correction(&mut self, outcome: PhysicsCorrectionOutcome, magnitude: f32) {
        use acceptance::phase3_evidence::CorrectionObservation;
        let outcome = match outcome {
            PhysicsCorrectionOutcome::Replayed {
                corrected_tick,
                replayed_ticks,
            } => CorrectionObservation::Replayed {
                corrected_tick,
                replayed_ticks,
            },
            PhysicsCorrectionOutcome::Snapped { tick } => CorrectionObservation::Snapped { tick },
        };
        self.0.note_correction(outcome, magnitude);
    }
    /// Emits the immutable fault captured by the gameplay authority.
    pub(crate) fn observe_authority_fault(
        &mut self,
        fault: PhysicsAuthorityFaultRecord,
    ) -> Vec<String> {
        self.0
            .observe_authority_fault(authority_fault_observation(fault))
    }
    /// Converts the current movement terminal state to evidence labels.
    #[cfg(test)]
    pub(crate) fn observe_terminal(
        &mut self,
        identity: Phase3EvidenceIdentity,
        source: MovementSource,
        physics_packet_count: u64,
        free_camera_packet_count: u64,
        pending_outbox_depth: usize,
        outbox_reconciliation: MovementOutboxReconciliation,
    ) -> Vec<String> {
        self.0.observe_terminal(
            identity,
            match source {
                MovementSource::Physics => "Physics",
                MovementSource::FreeCamera => "FreeCamera",
            },
            physics_packet_count,
            free_camera_packet_count,
            pending_outbox_depth,
            outbox_reconciliation.as_str(),
        )
    }
    /// Converts acknowledged tick records into independent evidence frames.
    pub(crate) fn observe_completed_ticks(&mut self, ticks: &[PhysicsTickEvidence]) -> Vec<String> {
        let mut markers = Vec::with_capacity(ticks.len());
        for tick in ticks {
            let context = tick.context;
            markers.extend(self.observe(Phase3EvidenceFrame {
                session_generation: tick.session_generation,
                fifo_sequence: context.fifo_sequence,
                physics_tick: tick.tick,
                pose_generation: context.pose_generation,
                dimension: context.dimension,
                network_position: tick.network_position,
                input_mode: protocol_input_mode(tick.input_mode),
                perspective: context.perspective,
                camera_blocked: context.camera_blocked,
                camera_fallback: context.camera_fallback,
                local_avatar_visible: context.local_avatar_visible,
                movement: tick.movement,
                look_delta: context.look_delta,
                jump_held: tick.jump_held,
                outbound_authorized: context.outbound_authorized,
                outbox_depth: context.outbox_depth,
                outbox_drops: context.outbox_drops,
                free_camera_packet_count: context.free_camera_packet_count,
                grounded_before_tick: tick.grounded_before_tick,
                grounded_after_tick: tick.grounded_after_tick,
                jump_started: tick.jump_started,
                jump_repeated: tick.jump_repeated,
                jump_released: tick.jump_released,
            }));
        }
        markers
    }
}

/// Evidence identity plus the app's collision-registry observation adapter.
#[derive(Resource)]
pub(crate) struct Phase3EvidenceIdentitySource(
    acceptance::phase3_evidence::Phase3EvidenceIdentitySource,
);
impl std::ops::Deref for Phase3EvidenceIdentitySource {
    type Target = acceptance::phase3_evidence::Phase3EvidenceIdentitySource;
    /// Exposes the evidence API while the app owns domain-to-observation conversion.
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl Phase3EvidenceIdentitySource {
    /// Captures the two registry identities after normal startup validation.
    pub(crate) fn from_build(
        target: crate::args::Phase3Target,
        candidate_physics: bool,
        collisions: &PhysicsCollisionRegistries,
    ) -> Result<Self, Phase3EvidenceIdentityError> {
        acceptance::phase3_evidence::Phase3EvidenceIdentitySource::from_build(
            target.as_str(),
            candidate_physics,
            collisions.preg_sha256(),
            collisions.breg_sha256(),
        )
        .map(Self)
    }
}
/// Converts the gameplay fault into evidence fields without extending the gameplay type.
fn authority_fault_observation(
    record: PhysicsAuthorityFaultRecord,
) -> acceptance::phase3_evidence::AuthorityFaultObservation {
    let (fault, detail) = match record.fault {
        PhysicsAuthorityFault::Unauthorized => ("unauthorized", serde_json::Value::Null),
        PhysicsAuthorityFault::IncompleteCollisionRegistry => {
            ("incomplete_collision_registry", serde_json::Value::Null)
        }
        PhysicsAuthorityFault::TickMismatch { expected, actual } => (
            "tick_mismatch",
            serde_json::json!({"expected": expected, "actual": actual}),
        ),
        PhysicsAuthorityFault::OutboxOverflow => ("outbox_overflow", serde_json::Value::Null),
        PhysicsAuthorityFault::InvalidCompletedSample => {
            ("invalid_completed_sample", serde_json::Value::Null)
        }
        PhysicsAuthorityFault::PhysicsSimulationError {
            due,
            tick_index,
            error,
        } => (
            "physics_simulation_error",
            serde_json::json!({
                "due": due,
                "tick_index": tick_index,
                "simulation_error": simulation_error_detail(&error),
            }),
        ),
        PhysicsAuthorityFault::CorrectionNotRetained { tick } => {
            ("correction_not_retained", serde_json::json!({"tick": tick}))
        }
        PhysicsAuthorityFault::CorrectionReplayFailed => {
            ("correction_replay_failed", serde_json::Value::Null)
        }
        PhysicsAuthorityFault::ReplayWorldIdentityMismatch { tick } => (
            "replay_world_identity_mismatch",
            serde_json::json!({"tick": tick}),
        ),
        PhysicsAuthorityFault::PendingWorldIdentityMismatch { tick } => (
            "pending_world_identity_mismatch",
            serde_json::json!({"tick": tick}),
        ),
        PhysicsAuthorityFault::PendingTickMismatch { expected, actual } => (
            "pending_tick_mismatch",
            serde_json::json!({"expected": expected, "actual": actual}),
        ),
        PhysicsAuthorityFault::PendingSessionMismatch { expected, actual } => (
            "pending_session_mismatch",
            serde_json::json!({"expected": expected, "actual": actual}),
        ),
        PhysicsAuthorityFault::IndeterminatePhysicsSend { tick } => (
            "indeterminate_physics_send",
            serde_json::json!({"tick": tick}),
        ),
    };
    acceptance::phase3_evidence::AuthorityFaultObservation {
        session_generation: record.session_generation,
        next_tick: record.next_tick,
        pending_count: record.pending_count,
        fault,
        detail,
    }
}

/// Converts the simulation error into the established serializable evidence detail.
fn simulation_error_detail(error: &sim::SimulationError) -> serde_json::Value {
    match error {
        sim::SimulationError::NonFiniteState { field } => serde_json::json!({
            "kind": "non_finite_state",
            "field": field,
            "message": error.to_string(),
        }),
        sim::SimulationError::NonFiniteInput { field } => serde_json::json!({
            "kind": "non_finite_input",
            "field": field,
            "message": error.to_string(),
        }),
        sim::SimulationError::InvalidItemUseMovementModifier => serde_json::json!({
            "kind": "invalid_item_use_movement_modifier",
            "message": error.to_string(),
        }),
        sim::SimulationError::InvalidMovementSpeed => serde_json::json!({
            "kind": "invalid_movement_speed",
            "message": error.to_string(),
        }),
        sim::SimulationError::InvalidSwimAmount => serde_json::json!({
            "kind": "invalid_swim_amount",
            "message": error.to_string(),
        }),
        sim::SimulationError::InvalidLiquidContactHeight => serde_json::json!({
            "kind": "invalid_liquid_contact_height",
            "message": error.to_string(),
        }),
        sim::SimulationError::World(world_error) => serde_json::json!({
            "kind": "world",
            "message": world_error.to_string(),
            "debug": format!("{world_error:?}"),
        }),
        sim::SimulationError::TickOverflow => serde_json::json!({
            "kind": "tick_overflow",
            "message": error.to_string(),
        }),
    }
}

/// Drains gameplay records every frame, emitting only when evidence is enabled.
pub(crate) fn emit_phase3_evidence(
    acceptance: Res<AcceptanceRun>,
    mut movement: ResMut<MovementTicker>,
    identity_source: Option<Res<Phase3EvidenceIdentitySource>>,
    mut evidence: ResMut<Phase3EvidenceEmitter>,
) {
    // Socket acknowledgements retain one immutable tick record until this
    // system runs. Drain it in normal production too; otherwise a client
    // without an acceptance identity permanently fills the bounded evidence
    // queue after 32 successful movement packets and disables physics.
    let completed_ticks = movement.take_tick_evidence();
    if !acceptance.enabled() {
        return;
    }
    let Some(identity_source) = identity_source else {
        return;
    };
    if let Some(fault) = movement.pending_authority_fault().cloned()
        && let Ok(identity) = identity_source.for_session(fault.session_generation)
    {
        let retained = movement
            .take_authority_fault()
            .expect("observed authority fault remains pending until emission");
        debug_assert_eq!(retained, fault);
        let mut markers = evidence.observe_identity(identity);
        markers.extend(evidence.observe_authority_fault(retained));
        let mut stdout = std::io::stdout().lock();
        for marker in markers {
            write_stdout_marker(&mut stdout, &marker);
        }
    }
    let pending_violations = evidence.take_violation_marker();
    if !pending_violations.is_empty() {
        let mut stdout = std::io::stdout().lock();
        for marker in pending_violations {
            write_stdout_marker(&mut stdout, &marker);
        }
    }
    let identity = match identity_source.for_session(movement.session_generation()) {
        Ok(identity) => identity,
        Err(_) => return,
    };
    let mut markers = evidence.observe_identity(identity);
    markers.extend(evidence.observe_completed_ticks(&completed_ticks));
    if markers.is_empty() {
        return;
    }
    let mut stdout = std::io::stdout().lock();
    for marker in markers {
        write_stdout_marker(&mut stdout, &marker);
    }
}

/// Maps the completed packet input mode to its evidence label.
const fn protocol_input_mode(mode: protocol::PlayerInputMode) -> InputMode {
    match mode {
        protocol::PlayerInputMode::Mouse => InputMode::KeyboardMouse,
        protocol::PlayerInputMode::Touch => InputMode::Touch,
        protocol::PlayerInputMode::GamePad => InputMode::GamePad,
    }
}
