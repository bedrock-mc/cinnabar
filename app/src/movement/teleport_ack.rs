//! Bounded HandledTeleport acknowledgement (PROVISIONAL, opt-in only).
//!
//! Vanilla Bedrock asserts the `HandledTeleport` input flag on outbound
//! `PlayerAuthInput` after it has accepted a server-driven teleport, so the
//! server can stop re-sending the anchor. Cinnabar learns qualifying server
//! teleports from exactly three production sites: a committed correction
//! classified [`super::CorrectionShape::TeleportSnap`], a local-player
//! `MovePlayer` whose event carries `teleported == true`, and a committed
//! respawn. Dimension changes, StartGame resets, client-derived surface-spawn
//! resolves, and Replay/Confirmed corrections never arm the assertion.
//!
//! The whole feature is gated behind the registered opt-in environment
//! marker [`markers::TELEPORT_ACK`] (value exactly `1`), evaluated once per
//! ticker construction (the production ticker is built once per session).
//! With any other value, or unset, every method here is a complete no-op: no
//! state is armed, no counter advances, no flag bit is ever projected, and
//! the outbound stream stays byte-identical to the un-gated build. Nothing
//! here is measured against a version-matched native client, so both the
//! 40-admitted-tick expiry budget and the single-shot consume policy are
//! explicitly provisional, not vanilla parity claims.

use std::ffi::OsStr;

use protocol::PlayerInputFlags;

use super::{
    MovementTicker, PhysicsCorrectionOutcome, QueuedPhysicsSample, trace::write_trace_line,
};
use crate::acceptance::markers;

/// The exact value of the registered opt-in marker that enables the feature;
/// every other value, or an unset variable, keeps today's exact bytes.
const ENABLED_VALUE: &str = "1";

/// Pure enablement rule for one environment-variable observation.
pub(super) fn enabled_for_env_value(value: Option<&OsStr>) -> bool {
    value == Some(OsStr::new(ENABLED_VALUE))
}

/// Whether this session arms teleport acknowledgements at all.
pub(crate) fn enabled_from_env() -> bool {
    enabled_for_env_value(std::env::var_os(markers::TELEPORT_ACK).as_deref())
}

const MARKER_PREFIX: &str = "TELEPORT_ACK=";
const SCHEMA_TAG: &str = "rust-mcbe-movement-teleport-ack-v1";

/// PROVISIONAL number of admitted completed ticks one armed assertion may
/// outlive without finding a transmission before it expires. At the fixed
/// 20 Hz tick this bounds the assertion to roughly two seconds of streaming;
/// pending version-matched native Bedrock measurement.
pub(super) const TELEPORT_ACK_ADMITTED_TICK_BUDGET: u64 = 40;
/// Which observed event reached the acknowledgement state machine.
///
/// The discriminator exists so call sites stay auditable and a future
/// per-kind policy can land without another signature change; today's
/// provisional policy treats every qualifying server teleport identically.
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
    pub(crate) fn note_server_teleport(&mut self, kind: ServerTeleportKind) {
        if !self.teleport_ack_enabled || self.pending_teleport_ack.is_some() {
            return;
        }
        let _ = kind;
        self.pending_teleport_ack = Some(TeleportAckPending {
            remaining_admitted_ticks: TELEPORT_ACK_ADMITTED_TICK_BUDGET,
        });
    }

    /// Classifies one committed correction outcome for the acknowledgement
    /// state machine: only a teleport-shaped snap is a server teleport; an
    /// ordinary replay stays counter-only. Confirmed outcomes never reach a
    /// call site (they mutate nothing upstream).
    pub(crate) fn note_committed_correction_outcome(&mut self, outcome: PhysicsCorrectionOutcome) {
        match outcome {
            PhysicsCorrectionOutcome::Snapped { .. } => {
                self.note_server_teleport(ServerTeleportKind::CorrectionSnap);
            }
            PhysicsCorrectionOutcome::Replayed { .. } => self.note_replayed_correction(),
        }
    }

    /// Counter-only observation of a Replay-shaped correction: replays are
    /// ordinary reconciliations and must never arm the assertion.
    pub(crate) fn note_replayed_correction(&mut self) {
        if !self.teleport_ack_enabled {
            return;
        }
        self.replayed_corrections_observed = self.replayed_corrections_observed.saturating_add(1);
    }

    /// Counter-only observation of a local MovePlayer without
    /// `teleported == true`; mode-only or rotation-only moves are not
    /// server teleports.
    pub(crate) fn note_unmarked_local_move_player(&mut self) {
        if !self.teleport_ack_enabled {
            return;
        }
        self.unmarked_move_players_observed = self.unmarked_move_players_observed.saturating_add(1);
    }

    /// Silently clears any armed assertion. Used by every queue-clearing
    /// boundary (session reset, deactivation, authority fault, FreeCamera
    /// source transitions, dimension changes); none of those may leak a stale
    /// assertion into a later transmission.
    pub(crate) fn clear_pending_teleport_ack(&mut self) {
        self.pending_teleport_ack = None;
    }

    /// Charges one admitted completed tick against the armed budget.
    ///
    /// An assertion that survives its whole budget without finding a
    /// transmission expires on the next admission: cleared, counted, and
    /// reported through one bounded stdout marker.
    pub(super) fn observe_admitted_tick_for_teleport_ack(&mut self) {
        if !self.teleport_ack_enabled {
            return;
        }
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
        if !self.teleport_ack_enabled || self.pending_teleport_ack.is_none() {
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

    #[cfg(test)]
    pub(crate) fn testing_set_teleport_ack(&mut self, enabled: bool) {
        self.teleport_ack_enabled = enabled;
    }

    #[cfg(test)]
    pub(crate) fn pending_teleport_ack_admitted_ticks(&self) -> Option<u64> {
        self.pending_teleport_ack
            .as_ref()
            .map(|pending| pending.remaining_admitted_ticks)
    }

    #[cfg(test)]
    pub(crate) const fn teleport_acks_expired(&self) -> u64 {
        self.teleport_acks_expired
    }

    #[cfg(test)]
    pub(crate) const fn replayed_corrections_observed(&self) -> u64 {
        self.replayed_corrections_observed
    }

    #[cfg(test)]
    pub(crate) const fn unmarked_move_players_observed(&self) -> u64 {
        self.unmarked_move_players_observed
    }
}
