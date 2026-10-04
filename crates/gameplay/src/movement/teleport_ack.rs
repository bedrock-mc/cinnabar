//! Acknowledges local MovePlayer teleports on the next transmitted input.
//!
//! Vanilla Player::handleMovePlayerPacket, mode 2,
//! sets the action that setFromComponent maps to HandledTeleport.
//! Correction-snap and respawn routes remain provisional and require the
//! opt-in marker the configured teleport-ack marker with value exactly `1`.
//!
//! Failed writes preserve the assertion for retry. The 40-admitted-tick
//! expiry remains provisional. Queue-clearing boundaries clear pending state.

use std::ffi::OsStr;

use protocol::PlayerInputFlags;

use super::{
    MovementTicker, PhysicsCorrectionOutcome, QueuedPhysicsSample, trace::write_trace_line,
};

/// Enables the provisional correction-snap and respawn acknowledgement routes.
const ENABLED_VALUE: &str = "1";

/// Pure enablement rule for one environment-variable observation.
pub(super) fn enabled_for_env_value(value: Option<&OsStr>) -> bool {
    value == Some(OsStr::new(ENABLED_VALUE))
}

/// Whether this session enables provisional teleport acknowledgement routes.
pub fn enabled_from_env() -> bool {
    enabled_for_env_value(super::diagnostics_config::value(|config| config.teleport_ack).as_deref())
}

const MARKER_PREFIX: &str = "TELEPORT_ACK=";
const SCHEMA_TAG: &str = "rust-mcbe-movement-teleport-ack-v1";

/// PROVISIONAL number of admitted completed ticks one armed assertion may
/// outlive without finding a transmission before it expires. At the fixed
/// 20 Hz tick this bounds the assertion to roughly two seconds of streaming;
/// pending version-matched native Bedrock measurement.
///
/// Exact expiry boundary: admissions one through forty each charge the
/// budget, and admission forty-one observes `remaining_admitted_ticks == 0`
/// in [`MovementTicker::observe_admitted_tick_for_teleport_ack`], expiring
/// the assertion, counting it, and emitting one bounded stdout marker. The
/// eventual native-evidence gate must carry these admission-41 semantics
/// verbatim rather than restating the budget as "forty ticks".
pub const TELEPORT_ACK_ADMITTED_TICK_BUDGET: u64 = 40;
/// Which observed event reached the acknowledgement state machine.
///
/// MovePlayer is verified; correction-snap and respawn remain opt-in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServerTeleportKind {
    /// A committed correction classified beyond the teleport displacement
    /// bound (`CorrectionShape::TeleportSnap`).
    CorrectionSnap,
    /// A local-player `MovePlayer` whose event carried `teleported == true`.
    MovePlayer,
    /// A committed respawn anchor.
    Respawn,
}

/// One armed, unacknowledged teleport assertion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct TeleportAckPending {
    pub(super) remaining_admitted_ticks: u64,
}

/// Renders the exact bounded single-line stdout marker for one expiry.
pub(super) fn expired_marker() -> String {
    format!(
        "{MARKER_PREFIX}{{\"schema\":\"{SCHEMA_TAG}\",\"phase\":\"expired\",\"budget_admitted_ticks\":{TELEPORT_ACK_ADMITTED_TICK_BUDGET}}}"
    )
}

impl MovementTicker {
    /// Arms one single-shot assertion on the next transmitted sample.
    ///
    /// Called ONLY from the three qualifying sites in the world-stream
    /// reconciliation; arming while already armed is a bounded no-op so a
    /// burst of teleports cannot queue multiple assertions.
    pub fn note_server_teleport(&mut self, kind: ServerTeleportKind) {
        if (!self.teleport_ack_enabled && kind != ServerTeleportKind::MovePlayer)
            || self.pending_teleport_ack.is_some()
        {
            return;
        }
        self.pending_teleport_ack = Some(TeleportAckPending {
            remaining_admitted_ticks: TELEPORT_ACK_ADMITTED_TICK_BUDGET,
        });
    }

    /// Classifies one committed correction outcome for the acknowledgement
    /// state machine: only a teleport-shaped snap is a server teleport; an
    /// ordinary replay stays counter-only. Confirmed outcomes never reach a
    /// call site (they mutate nothing upstream).
    pub fn note_committed_correction_outcome(&mut self, outcome: PhysicsCorrectionOutcome) {
        match outcome {
            PhysicsCorrectionOutcome::Snapped { .. } => {
                self.note_server_teleport(ServerTeleportKind::CorrectionSnap);
            }
            PhysicsCorrectionOutcome::Replayed { .. } => self.note_replayed_correction(),
        }
    }

    /// Counter-only observation of a Replay-shaped correction: replays are
    /// ordinary reconciliations and must never arm the assertion.
    pub fn note_replayed_correction(&mut self) {
        if !self.teleport_ack_enabled {
            return;
        }
        self.replayed_corrections_observed = self.replayed_corrections_observed.saturating_add(1);
    }

    /// Counter-only observation of a local MovePlayer without
    /// `teleported == true`; mode-only or rotation-only moves are not
    /// server teleports.
    pub fn note_unmarked_local_move_player(&mut self) {
        if !self.teleport_ack_enabled {
            return;
        }
        self.unmarked_move_players_observed = self.unmarked_move_players_observed.saturating_add(1);
    }

    /// Silently clears any armed assertion. Used by every queue-clearing
    /// boundary (session reset, deactivation, authority fault, FreeCamera
    /// source transitions, dimension changes); none of those may leak a stale
    /// assertion into a later transmission.
    pub fn clear_pending_teleport_ack(&mut self) {
        self.pending_teleport_ack = None;
    }

    /// Charges one admitted completed tick against the armed budget.
    ///
    /// An assertion that survives its whole budget without finding a
    /// transmission expires on the next admission: cleared, counted, and
    /// reported through one bounded stdout marker.
    pub(super) fn observe_admitted_tick_for_teleport_ack(&mut self) {
        let Some(pending) = self.pending_teleport_ack.as_mut() else {
            return;
        };
        if pending.remaining_admitted_ticks == 0 {
            self.pending_teleport_ack = None;
            self.teleport_acks_expired = self.teleport_acks_expired.saturating_add(1);
            write_trace_line(&expired_marker());
            return;
        }
        pending.remaining_admitted_ticks -= 1;
    }

    /// Projects the armed assertion onto the sample popped for encoding,
    /// immediately before `player_auth_input` serialization.
    ///
    /// The mutation happens on the popped record, so the staged admission
    /// copy carries the bit and every restore/retry path preserves it. Returns
    /// whether this sample carries the assertion; the caller must consume the
    /// pending state only after the transport accepts the packet.
    pub(super) fn project_pending_teleport_ack(&self, sample: &mut QueuedPhysicsSample) -> bool {
        sample.snapshot.flags = sample
            .snapshot
            .flags
            .with_mask(PlayerInputFlags::HANDLED_TELEPORT, false);
        if self.pending_teleport_ack.is_none() {
            return false;
        }
        sample.snapshot.flags |= PlayerInputFlags::HANDLED_TELEPORT;
        true
    }

    /// Consumes the armed assertion after the transport accepted the flagged
    /// packet. Never called on pop or on send failure, so a failed write
    /// retries with the assertion still armed.
    pub(super) fn consume_pending_teleport_ack(&mut self) {
        self.pending_teleport_ack = None;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn testing_set_teleport_ack(&mut self, enabled: bool) {
        self.teleport_ack_enabled = enabled;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn pending_teleport_ack_admitted_ticks(&self) -> Option<u64> {
        self.pending_teleport_ack
            .as_ref()
            .map(|pending| pending.remaining_admitted_ticks)
    }

    #[cfg(any(test, feature = "test-support"))]
    pub const fn teleport_acks_expired(&self) -> u64 {
        self.teleport_acks_expired
    }

    #[cfg(any(test, feature = "test-support"))]
    pub const fn replayed_corrections_observed(&self) -> u64 {
        self.replayed_corrections_observed
    }

    #[cfg(any(test, feature = "test-support"))]
    pub const fn unmarked_move_players_observed(&self) -> u64 {
        self.unmarked_move_players_observed
    }
}
