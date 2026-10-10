use std::collections::VecDeque;

use bevy::prelude::Resource;
use semantic_input::{InputMode, PerspectiveMode};

use diagnostics::markers::{
    self, PHASE3_EVENT, PHASE3_FRAME, PHASE3_IDENTITY, PHASE3_TERMINAL, PHASE3_VIOLATION,
};

pub const MAX_PHASE3_FRAME_RECORDS: usize = 12_000;
pub const MAX_PHASE3_EVENT_RECORDS: usize = 256;
pub const MAX_PHASE3_FAULT_RECORDS: usize = 32;
const MAX_EVIDENCE_LOOK_DELTA: f32 = 64.0;
const MAX_EVIDENCE_CORRECTION_MAGNITUDE: f32 = 100_000_000.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Phase3EvidenceIdentityError {
    #[error("Phase 3 evidence build commit is not an exact lowercase 40-hex identity")]
    InvalidBuildCommit,
    #[error("Phase 3 evidence session generation is zero")]
    ZeroSessionGeneration,
    #[error("Phase 3 evidence PREG or BREG identity is zero")]
    ZeroRegistryIdentity,
    #[error("Phase 3 evidence build is missing {environment}")]
    MissingBuildCommit { environment: &'static str },
    #[error(
        "Phase 3 evidence run was not launched with {environment}=false from an explicitly clean source tree"
    )]
    DirtyOrUnattributedBuild { environment: &'static str },
    #[error("Phase 3 evidence run is missing or has invalid {environment}")]
    InvalidRunIdentity { environment: &'static str },
}

/// Supplies a stable target label without making evidence depend on app argument types.
pub trait Phase3TargetLabel {
    /// Returns the existing target spelling used by the evidence schema.
    fn evidence_label(self) -> &'static str;
}

impl Phase3TargetLabel for &'static str {
    /// Uses a label already observed by the runtime adapter.
    fn evidence_label(self) -> &'static str {
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Phase3EvidenceIdentity {
    build_commit: String,
    target: &'static str,
    session_generation: u64,
    preg_sha256: [u8; 32],
    breg_sha256: [u8; 32],
    candidate_physics: bool,
    source_dirty: bool,
    run_id: String,
    endpoint: String,
    bridge_endpoint: String,
    core_sha256: String,
    core_process_id: u32,
    app_process_id: u32,
}

impl Phase3EvidenceIdentity {
    pub const fn candidate_physics(&self) -> bool {
        self.candidate_physics
    }

    pub fn new(
        build_commit: impl Into<String>,
        target: impl Phase3TargetLabel,
        session_generation: u64,
        preg_sha256: [u8; 32],
        breg_sha256: [u8; 32],
        candidate_physics: bool,
    ) -> Result<Self, Phase3EvidenceIdentityError> {
        let target = target.evidence_label();
        let build_commit = build_commit.into();
        if build_commit.len() != 40
            || !build_commit
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(Phase3EvidenceIdentityError::InvalidBuildCommit);
        }
        if session_generation == 0 {
            return Err(Phase3EvidenceIdentityError::ZeroSessionGeneration);
        }
        if preg_sha256 == [0; 32] || breg_sha256 == [0; 32] {
            return Err(Phase3EvidenceIdentityError::ZeroRegistryIdentity);
        }
        Ok(Self {
            build_commit,
            target,
            session_generation,
            preg_sha256,
            breg_sha256,
            candidate_physics,
            source_dirty: false,
            run_id: "0123456789abcdef0123456789abcdef".to_owned(),
            endpoint: "127.0.0.1:19132".to_owned(),
            bridge_endpoint: "127.0.0.1:19133".to_owned(),
            core_sha256: "33".repeat(32),
            core_process_id: 41,
            app_process_id: 42,
        })
    }

    fn bind_run(
        mut self,
        run_id: String,
        endpoint: String,
        bridge_endpoint: String,
        core_sha256: String,
        core_process_id: u32,
    ) -> Result<Self, Phase3EvidenceIdentityError> {
        validate_run_identity(
            &run_id,
            &endpoint,
            &bridge_endpoint,
            &core_sha256,
            core_process_id,
        )?;
        self.run_id = run_id;
        self.endpoint = endpoint;
        self.bridge_endpoint = bridge_endpoint;
        self.core_sha256 = core_sha256;
        self.core_process_id = core_process_id;
        self.app_process_id = std::process::id();
        Ok(self)
    }

    fn marker(self) -> String {
        format!(
            "{PHASE3_IDENTITY}={}",
            serde_json::json!({
                "schema": "rust-mcbe-phase3-identity-v1",
                "build_commit": self.build_commit,
                "target": self.target,
                "protocol": protocol::PROTOCOL_VERSION,
                "session_generation": self.session_generation,
                "preg_sha256": digest_hex(self.preg_sha256),
                "breg_sha256": digest_hex(self.breg_sha256),
                "candidate_physics": self.candidate_physics,
                "source_dirty": self.source_dirty,
                "run_id": self.run_id,
                "endpoint": self.endpoint,
                "bridge_endpoint": self.bridge_endpoint,
                "core_sha256": self.core_sha256,
                "core_process_id": self.core_process_id,
                "app_process_id": self.app_process_id,
            })
        )
    }
}

#[derive(Resource, Debug, Clone)]
pub struct Phase3EvidenceIdentitySource {
    build_commit: String,
    target: &'static str,
    preg_sha256: [u8; 32],
    breg_sha256: [u8; 32],
    candidate_physics: bool,
    run_id: String,
    endpoint: String,
    bridge_endpoint: String,
    core_sha256: String,
    core_process_id: u32,
}

impl Phase3EvidenceIdentitySource {
    /// Reads the commit and clean-source flag from the launch environment so commits never
    /// rebuild the client; the launcher builds, hashes and starts that exact binary.
    pub fn from_build(
        target: &'static str,
        candidate_physics: bool,
        preg_sha256: [u8; 32],
        breg_sha256: [u8; 32],
    ) -> Result<Self, Phase3EvidenceIdentityError> {
        let build_commit = std::env::var(diagnostics::markers::BUILD_COMMIT).map_err(|_| {
            Phase3EvidenceIdentityError::MissingBuildCommit {
                environment: diagnostics::markers::BUILD_COMMIT,
            }
        })?;
        Phase3EvidenceIdentity::new(
            build_commit.as_str(),
            target,
            1,
            preg_sha256,
            breg_sha256,
            candidate_physics,
        )?;
        validate_phase3_build_source(
            std::env::var(diagnostics::markers::SOURCE_DIRTY)
                .ok()
                .as_deref(),
        )?;
        let run_id = required_run_environment(diagnostics::markers::PHASE3_RUN_ID)?;
        let endpoint = required_run_environment(diagnostics::markers::PHASE3_ENDPOINT)?;
        let bridge_endpoint =
            required_run_environment(diagnostics::markers::PHASE3_BRIDGE_ENDPOINT)?;
        let core_sha256 = required_run_environment(diagnostics::markers::PHASE3_CORE_SHA256)?;
        let core_process_id =
            required_run_environment(diagnostics::markers::PHASE3_CORE_PROCESS_ID)?
                .parse::<u32>()
                .ok()
                .filter(|process_id| *process_id != 0)
                .ok_or(Phase3EvidenceIdentityError::InvalidRunIdentity {
                    environment: diagnostics::markers::PHASE3_CORE_PROCESS_ID,
                })?;
        validate_run_identity(
            &run_id,
            &endpoint,
            &bridge_endpoint,
            &core_sha256,
            core_process_id,
        )?;
        Ok(Self {
            build_commit,
            target,
            preg_sha256,
            breg_sha256,
            candidate_physics,
            run_id,
            endpoint,
            bridge_endpoint,
            core_sha256,
            core_process_id,
        })
    }

    pub fn for_session(
        &self,
        session_generation: u64,
    ) -> Result<Phase3EvidenceIdentity, Phase3EvidenceIdentityError> {
        Phase3EvidenceIdentity::new(
            self.build_commit.clone(),
            self.target,
            session_generation,
            self.preg_sha256,
            self.breg_sha256,
            self.candidate_physics,
        )
        .and_then(|identity| {
            identity.bind_run(
                self.run_id.clone(),
                self.endpoint.clone(),
                self.bridge_endpoint.clone(),
                self.core_sha256.clone(),
                self.core_process_id,
            )
        })
    }
}

pub fn validate_phase3_build_source(
    source_dirty: Option<&str>,
) -> Result<(), Phase3EvidenceIdentityError> {
    if source_dirty == Some("false") {
        Ok(())
    } else {
        Err(Phase3EvidenceIdentityError::DirtyOrUnattributedBuild {
            environment: diagnostics::markers::SOURCE_DIRTY,
        })
    }
}

fn required_run_environment(
    environment: &'static str,
) -> Result<String, Phase3EvidenceIdentityError> {
    std::env::var(environment)
        .ok()
        .filter(|value| !value.is_empty())
        .ok_or(Phase3EvidenceIdentityError::InvalidRunIdentity { environment })
}

fn validate_run_identity(
    run_id: &str,
    endpoint: &str,
    bridge_endpoint: &str,
    core_sha256: &str,
    core_process_id: u32,
) -> Result<(), Phase3EvidenceIdentityError> {
    if run_id.len() != 32
        || !run_id
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(Phase3EvidenceIdentityError::InvalidRunIdentity {
            environment: diagnostics::markers::PHASE3_RUN_ID,
        });
    }
    for (value, environment) in [
        (endpoint, diagnostics::markers::PHASE3_ENDPOINT),
        (
            bridge_endpoint,
            diagnostics::markers::PHASE3_BRIDGE_ENDPOINT,
        ),
    ] {
        let Some((host, port)) = value.rsplit_once(':') else {
            return Err(Phase3EvidenceIdentityError::InvalidRunIdentity { environment });
        };
        if host.is_empty()
            || host.bytes().any(|byte| byte.is_ascii_whitespace())
            || port.parse::<u16>().ok().filter(|port| *port != 0).is_none()
        {
            return Err(Phase3EvidenceIdentityError::InvalidRunIdentity { environment });
        }
    }
    if core_sha256.len() != 64
        || !core_sha256
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(Phase3EvidenceIdentityError::InvalidRunIdentity {
            environment: diagnostics::markers::PHASE3_CORE_SHA256,
        });
    }
    if core_process_id == 0 {
        return Err(Phase3EvidenceIdentityError::InvalidRunIdentity {
            environment: diagnostics::markers::PHASE3_CORE_PROCESS_ID,
        });
    }
    Ok(())
}

/// Immutable authority-fault fields observed at the gameplay boundary.
pub struct AuthorityFaultObservation {
    pub session_generation: u64,
    pub next_tick: u64,
    pub pending_count: usize,
    pub fault: &'static str,
    pub detail: serde_json::Value,
}

impl AuthorityFaultObservation {
    /// Formats the existing fault schema after the runtime has classified the domain error.
    fn evidence_marker(self) -> String {
        format!(
            "{PHASE3_EVENT}={}",
            serde_json::json!({
                "schema": "rust-mcbe-phase3-event-v1",
                "kind": "authority_fault",
                "session_generation": self.session_generation,
                "next_tick": self.next_tick,
                "pending_count": self.pending_count,
                "fault": self.fault,
                "detail": self.detail,
            })
        )
    }
}

/// Result of a gameplay correction observed by evidence.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CorrectionObservation {
    Replayed {
        corrected_tick: u64,
        replayed_ticks: usize,
    },
    Snapped {
        tick: u64,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase3EvidenceEventKind {
    Dimension,
    Session,
}

impl Phase3EvidenceEventKind {
    const ALL: [Self; 2] = [Self::Dimension, Self::Session];

    const fn index(self) -> usize {
        match self {
            Self::Dimension => 0,
            Self::Session => 1,
        }
    }

    const fn name(self) -> &'static str {
        match self {
            Self::Dimension => "dimension",
            Self::Session => "session",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Phase3EvidenceFrame {
    pub session_generation: u64,
    pub fifo_sequence: u64,
    pub physics_tick: u64,
    pub pose_generation: u64,
    pub dimension: i32,
    pub network_position: [f32; 3],
    pub input_mode: InputMode,
    pub perspective: PerspectiveMode,
    pub camera_blocked: bool,
    pub camera_fallback: bool,
    pub local_avatar_visible: bool,
    pub movement: [f32; 2],
    pub look_delta: [f32; 2],
    pub jump_held: bool,
    pub outbound_authorized: bool,
    pub outbox_depth: usize,
    pub outbox_drops: u64,
    pub free_camera_packet_count: u64,
    pub grounded_before_tick: bool,
    pub grounded_after_tick: bool,
    pub jump_started: bool,
    pub jump_repeated: bool,
    pub jump_released: bool,
}

impl Phase3EvidenceFrame {
    fn is_valid(self, outbox_capacity: usize) -> bool {
        self.session_generation != 0
            && self.pose_generation != 0
            && self.network_position.into_iter().all(f32::is_finite)
            && self
                .movement
                .into_iter()
                .all(|axis| axis.is_finite() && (-1.0..=1.0).contains(&axis))
            && self.look_delta.into_iter().all(|axis| {
                axis.is_finite()
                    && (-MAX_EVIDENCE_LOOK_DELTA..=MAX_EVIDENCE_LOOK_DELTA).contains(&axis)
            })
            && self.outbound_authorized
            && self.local_avatar_visible == (self.perspective != PerspectiveMode::FirstPerson)
            && !(self.camera_blocked && self.camera_fallback)
            && self.outbox_depth <= outbox_capacity
            && self.outbox_drops == 0
            && self.free_camera_packet_count == 0
            && !(self.jump_started && self.jump_released)
            && (!self.jump_repeated
                || (self.jump_started && self.jump_held && self.grounded_before_tick))
            && (!self.jump_released || !self.jump_held)
    }

    fn frame_marker(self) -> String {
        format!(
            "{PHASE3_FRAME}={}",
            serde_json::json!({
                "schema": "rust-mcbe-phase3-frame-v2",
                "session_generation": self.session_generation,
                "fifo_sequence": self.fifo_sequence,
                "physics_tick": self.physics_tick,
                "pose_generation": self.pose_generation,
                "dimension": self.dimension,
                "network_position": self.network_position,
                "input_mode": input_mode_name(self.input_mode),
                "perspective": perspective_name(self.perspective),
                "camera_blocked": self.camera_blocked,
                "camera_fallback": self.camera_fallback,
                "local_avatar_visible": self.local_avatar_visible,
                "movement": self.movement,
                "look_delta": self.look_delta,
                "jump_held": self.jump_held,
                "outbound_authorized": self.outbound_authorized,
                "outbox_depth": self.outbox_depth,
                "outbox_drops": self.outbox_drops,
                "free_camera_packet_count": self.free_camera_packet_count,
                "grounded_before_tick": self.grounded_before_tick,
                "grounded_after_tick": self.grounded_after_tick,
                "jump_started": self.jump_started,
                "jump_repeated": self.jump_repeated,
                "jump_released": self.jump_released,
            })
        )
    }

    fn event_marker(self, kind: Phase3EvidenceEventKind, event_sequence: u64) -> String {
        format!(
            "{PHASE3_EVENT}={}",
            serde_json::json!({
                "schema": "rust-mcbe-phase3-event-v1",
                "kind": kind.name(),
                "event_sequence": event_sequence,
                "session_generation": self.session_generation,
                "fifo_sequence": self.fifo_sequence,
                "physics_tick": self.physics_tick,
                "dimension": self.dimension,
            })
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Phase3CorrectionEvidence {
    outcome: CorrectionObservation,
    magnitude: f32,
}

/// Previous/current monotonicity witnesses captured when the emitter raises
/// `non_monotonic_frame`. They ride once on that violation marker so a
/// rejected run identifies exactly which frame identity regressed; every
/// other violation reason stays an exact two-key object.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct NonMonotonicFrameIdentity {
    previous_session_generation: u64,
    current_session_generation: u64,
    previous_physics_tick: u64,
    current_physics_tick: u64,
    previous_dimension: i32,
    current_dimension: i32,
    previous_fifo_sequence: u64,
    current_fifo_sequence: u64,
    previous_pose_generation: u64,
    current_pose_generation: u64,
}

impl Phase3CorrectionEvidence {
    fn event_marker(self, frame: Phase3EvidenceFrame, event_sequence: u64) -> String {
        let (outcome, corrected_tick, replayed_ticks) = match self.outcome {
            CorrectionObservation::Replayed {
                corrected_tick,
                replayed_ticks,
            } => ("replayed", corrected_tick, replayed_ticks),
            CorrectionObservation::Snapped { tick } => ("snapped", tick, 0),
        };
        format!(
            "{PHASE3_EVENT}={}",
            serde_json::json!({
                "schema": "rust-mcbe-phase3-event-v1",
                "kind": "correction",
                "event_sequence": event_sequence,
                "session_generation": frame.session_generation,
                "fifo_sequence": frame.fifo_sequence,
                "physics_tick": frame.physics_tick,
                "dimension": frame.dimension,
                "correction_outcome": outcome,
                "corrected_tick": corrected_tick,
                "replayed_ticks": replayed_ticks,
                "correction_magnitude": self.magnitude,
            })
        )
    }
}

#[derive(Resource, Debug, Default)]
pub struct Phase3EvidenceEmitter {
    identity: Option<Phase3EvidenceIdentity>,
    identity_conflict_emitted: bool,
    last_frame_identity: Option<(u64, u64, i32, u64, u64)>,
    non_monotonic_detail: Option<NonMonotonicFrameIdentity>,
    pending_events: [bool; 2],
    pending_corrections: VecDeque<Phase3CorrectionEvidence>,
    frame_records: usize,
    event_records: usize,
    fault_records: usize,
    next_event_sequence: u64,
    frame_overflow_emitted: bool,
    event_overflow_emitted: bool,
    fault_overflow_emitted: bool,
    violation_reason: Option<&'static str>,
    violation_emitted: bool,
    terminal_emitted: bool,
}

impl Phase3EvidenceEmitter {
    pub fn observe_identity(&mut self, identity: Phase3EvidenceIdentity) -> Vec<String> {
        match self.identity.as_ref() {
            None => {
                self.identity = Some(identity.clone());
                vec![identity.marker()]
            }
            Some(current) if current == &identity => Vec::new(),
            Some(_) if !self.identity_conflict_emitted => {
                self.identity_conflict_emitted = true;
                vec![identity.marker()]
            }
            Some(_) => Vec::new(),
        }
    }

    pub fn note_event(&mut self, kind: Phase3EvidenceEventKind) {
        self.pending_events[kind.index()] = true;
    }

    pub fn note_correction(&mut self, outcome: CorrectionObservation, magnitude: f32) {
        if !magnitude.is_finite()
            || !(0.0..=MAX_EVIDENCE_CORRECTION_MAGNITUDE).contains(&magnitude)
            || self.pending_corrections.len() >= MAX_PHASE3_EVENT_RECORDS
        {
            self.record_violation("invalid_correction");
            return;
        }
        self.pending_corrections
            .push_back(Phase3CorrectionEvidence { outcome, magnitude });
    }

    pub fn observe_authority_fault(&mut self, fault: AuthorityFaultObservation) -> Vec<String> {
        let mut markers = Vec::with_capacity(2);
        if self.fault_records >= MAX_PHASE3_FAULT_RECORDS {
            if self.fault_overflow_emitted {
                return self.take_violation_marker();
            }
            self.fault_overflow_emitted = true;
            self.record_violation("authority_fault_overflow");
            return self.take_violation_marker();
        }
        self.fault_records += 1;
        markers.push(fault.evidence_marker());
        self.record_violation("authority_fault");
        markers.extend(self.take_violation_marker());
        markers
    }

    pub fn observe(&mut self, frame: Phase3EvidenceFrame, outbox_capacity: usize) -> Vec<String> {
        if self.violation_reason.is_some() {
            return self.take_violation_marker();
        }
        if !frame.is_valid(outbox_capacity) {
            self.record_violation("invalid_frame");
            return self.take_violation_marker();
        }
        if let Some((session, tick, dimension, fifo, pose)) = self.last_frame_identity {
            let dimension_changed = dimension != frame.dimension;
            if session != frame.session_generation
                || (!dimension_changed && frame.physics_tick != tick.saturating_add(1))
                || frame.fifo_sequence < fifo
                || frame.pose_generation < pose
            {
                self.non_monotonic_detail = Some(NonMonotonicFrameIdentity {
                    previous_session_generation: session,
                    current_session_generation: frame.session_generation,
                    previous_physics_tick: tick,
                    current_physics_tick: frame.physics_tick,
                    previous_dimension: dimension,
                    current_dimension: frame.dimension,
                    previous_fifo_sequence: fifo,
                    current_fifo_sequence: frame.fifo_sequence,
                    previous_pose_generation: pose,
                    current_pose_generation: frame.pose_generation,
                });
                self.record_violation("non_monotonic_frame");
                return self.take_violation_marker();
            }
        }
        self.last_frame_identity = Some((
            frame.session_generation,
            frame.physics_tick,
            frame.dimension,
            frame.fifo_sequence,
            frame.pose_generation,
        ));

        if self.frame_records >= MAX_PHASE3_FRAME_RECORDS {
            if self.frame_overflow_emitted {
                return Vec::new();
            }
            self.frame_overflow_emitted = true;
            self.record_violation("frame_overflow");
            return self.take_violation_marker();
        }

        self.frame_records += 1;
        let mut markers = Vec::with_capacity(4 + self.pending_corrections.len());
        markers.push(frame.frame_marker());
        while let Some(correction) = self.pending_corrections.pop_front() {
            if self.event_records >= MAX_PHASE3_EVENT_RECORDS {
                if !self.event_overflow_emitted {
                    self.event_overflow_emitted = true;
                    self.record_violation("event_overflow");
                    markers.extend(self.take_violation_marker());
                }
                self.pending_corrections.clear();
                break;
            }
            self.event_records += 1;
            let event_sequence = self.take_event_sequence();
            markers.push(correction.event_marker(frame, event_sequence));
        }
        for kind in Phase3EvidenceEventKind::ALL {
            if !self.pending_events[kind.index()] {
                continue;
            }
            self.pending_events[kind.index()] = false;
            if self.event_records >= MAX_PHASE3_EVENT_RECORDS {
                if !self.event_overflow_emitted {
                    self.event_overflow_emitted = true;
                    self.record_violation("event_overflow");
                    markers.extend(self.take_violation_marker());
                }
                continue;
            }
            self.event_records += 1;
            let event_sequence = self.take_event_sequence();
            markers.push(frame.event_marker(kind, event_sequence));
        }
        markers
    }

    fn take_event_sequence(&mut self) -> u64 {
        let sequence = self.next_event_sequence;
        self.next_event_sequence = self.next_event_sequence.saturating_add(1);
        sequence
    }

    fn record_violation(&mut self, reason: &'static str) {
        if self.violation_reason.is_none() {
            self.violation_reason = Some(reason);
        }
    }

    pub fn take_violation_marker(&mut self) -> Vec<String> {
        let Some(reason) = self.violation_reason else {
            return Vec::new();
        };
        if self.violation_emitted {
            return Vec::new();
        }
        self.violation_emitted = true;
        // The captured identity is cleared with this single emission and is
        // included only for its own reason; every other reason keeps exactly
        // the two-key object.
        let detail = self.non_monotonic_detail.take();
        vec![format!(
            "{PHASE3_VIOLATION}={}",
            match detail {
                Some(identity) if reason == "non_monotonic_frame" => serde_json::json!({
                    "schema": "rust-mcbe-phase3-violation-v2",
                    "reason": reason,
                    "frame_identity": identity,
                }),
                _ => serde_json::json!({
                    "schema": "rust-mcbe-phase3-violation-v2",
                    "reason": reason,
                }),
            }
        )]
    }

    pub fn observe_terminal(
        &mut self,
        identity: Phase3EvidenceIdentity,
        source_name: &'static str,
        physics_packet_count: u64,
        free_camera_packet_count: u64,
        pending_outbox_depth: usize,
        outbox_reconciliation: &str,
    ) -> Vec<String> {
        if self.terminal_emitted {
            return self.take_violation_marker();
        }
        let session_generation = identity.session_generation;
        let candidate_physics = identity.candidate_physics;
        let mut markers = self.observe_identity(identity);
        if !self.pending_corrections.is_empty() {
            self.pending_corrections.clear();
            self.record_violation("terminal_pending_correction");
        }
        if free_camera_packet_count != 0 || candidate_physics != (source_name == "Physics") {
            self.record_violation("terminal_source_or_packet_mismatch");
        }
        // A remote-initiated close (RemoteClosed) is not a client-authority
        // fault: candidate terminals may finish either fully drained or with
        // the outbound stream healthy when the REMOTE side terminated the
        // transport. Every other reconciliation state still violates.
        let outbox_settled = if candidate_physics {
            matches!(outbox_reconciliation, "Drained" | "RemoteClosed")
        } else {
            outbox_reconciliation == "NotAuthoritative"
        };
        if pending_outbox_depth != 0 || !outbox_settled {
            self.record_violation("terminal_outbox_not_drained");
        }
        markers.extend(self.take_violation_marker());
        self.terminal_emitted = true;
        markers.push(format!(
            "{PHASE3_TERMINAL}={}",
            serde_json::json!({
                "schema": "rust-mcbe-phase3-terminal-v1",
                "session_generation": session_generation,
                "source": source_name,
                "physics_packet_count": physics_packet_count,
                "free_camera_packet_count": free_camera_packet_count,
                "pending_outbox_depth": pending_outbox_depth,
                "outbox_reconciliation": outbox_reconciliation,
            })
        ));
        markers
    }
}

fn digest_hex(digest: [u8; 32]) -> String {
    use std::fmt::Write as _;

    let mut encoded = String::with_capacity(64);
    for byte in digest {
        write!(encoded, "{byte:02x}").expect("writing to String cannot fail");
    }
    encoded
}

const fn input_mode_name(mode: InputMode) -> &'static str {
    match mode {
        InputMode::KeyboardMouse => "KeyboardMouse",
        InputMode::GamePad => "GamePad",
        InputMode::Touch => "Touch",
    }
}

const fn perspective_name(mode: PerspectiveMode) -> &'static str {
    match mode {
        PerspectiveMode::FirstPerson => "FirstPerson",
        PerspectiveMode::ThirdPersonBack => "ThirdPersonBack",
        PerspectiveMode::ThirdPersonFront => "ThirdPersonFront",
    }
}
