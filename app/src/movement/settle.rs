//! Provisional post-spawn transmission-settle gate for production movement.
//!
//! Live third-party evidence (2026-08-22): a colliding lobby spawn produced
//! sustained inputless horizontal displacement that server anti-cheats
//! reject ("movement cheats") or silently drop after idle timeouts. After
//! each session spawn anchor this gate withholds the outbound
//! `PlayerAuthInput` transport hand-off until prediction reports
//! [`SETTLED_TICKS`] consecutive admitted samples that are grounded and free
//! of horizontal collisions, bounded by [`SETTLE_TIMEOUT_TICKS`] suppressed
//! admissions before failing open so a permanently weird spawn cannot starve
//! the server's input stream.
//!
//! Simulation, admission, and tick scheduling are unchanged while the gate
//! withholds transmission: only the hand-off is delayed, suppressed ticks
//! are never replayed, and every teleport-style anchor starts a fresh
//! bounded episode. Both constants are explicitly provisional pending
//! version-matched native Bedrock measurement (VPA-109 family); they make no
//! vanilla parity claim.

use super::{PhysicsMovementSample, write_trace_line};

/// Provisional number of consecutive admitted samples that must report
/// grounded movement without a horizontal collision before transmission
/// resumes. At the fixed 20 Hz tick this is a one-second stability window.
///
/// Provisional pending version-matched native Bedrock measurement (VPA-109
/// family); not a vanilla parity claim.
pub(super) const SETTLED_TICKS: u64 = 20;

/// Provisional maximum number of suppressed admitted ticks in one episode
/// before the gate fails open and resumes transmission regardless of
/// stability. At the fixed 20 Hz tick this bounds one suppression episode to
/// ten seconds, so a permanently colliding spawn cannot silence the outbound
/// stream into an idle timeout.
///
/// Provisional pending version-matched native Bedrock measurement (VPA-109
/// family); not a vanilla parity claim.
pub(super) const SETTLE_TIMEOUT_TICKS: u64 = 200;

const _: () = assert!(
    SETTLED_TICKS > 0 && SETTLE_TIMEOUT_TICKS > SETTLED_TICKS,
    "the settle window must be reachable strictly inside its fail-open cap"
);

const SCHEMA_TAG: &str = "rust-mcbe-movement-tx-gate-v1";
const MARKER_PREFIX: &str = "MOVEMENT_TX_GATE=";

/// Why one suppression episode began or ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GateReason {
    /// A spawn anchor armed the window, or the required clean run completed.
    SpawnSettle,
    /// The episode exhausted its suppression cap and failed open.
    Timeout,
    /// A teleport-style reanchor ended the episode before it resolved.
    Reanchor,
}

impl GateReason {
    const fn as_str(self) -> &'static str {
        match self {
            Self::SpawnSettle => "spawn_settle",
            Self::Timeout => "timeout",
            Self::Reanchor => "reanchor",
        }
    }
}

/// Renders the exact single-line stdout marker for one gate transition.
fn settle_marker(phase: &str, reason: GateReason, ticks_suppressed: u64) -> String {
    format!(
        "{MARKER_PREFIX}{{\"schema\":\"{SCHEMA_TAG}\",\"phase\":\"{phase}\",\"reason\":\"{}\",\"ticks_suppressed\":{ticks_suppressed}}}",
        reason.as_str(),
    )
}

fn emit_marker(phase: &str, reason: GateReason, ticks_suppressed: u64) {
    write_trace_line(&settle_marker(phase, reason, ticks_suppressed));
}

/// Bounded post-spawn suppression state for one movement session.
///
/// Episodes are started by spawn anchors ([`MovementTicker::reset`],
/// surface-spawn resolve, correction snaps), advanced by admitted completed
/// samples, ended by settling, the fail-open cap, a later anchor, or silent
/// teardown when authority itself ends.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct SpawnSettleGate {
    suppressing: bool,
    consecutive_settled_samples: u64,
    suppressed_admitted_ticks: u64,
}

impl SpawnSettleGate {
    pub(super) const fn suppressing(&self) -> bool {
        self.suppressing
    }

    /// Starts a fresh suppression episode for a newly anchored spawn.
    ///
    /// An episode that was still active is reported as lifted by reanchor
    /// before the fresh window begins, keeping one bounded marker per
    /// transition.
    pub(super) fn engage(&mut self) {
        if self.suppressing {
            emit_marker(
                "lifted",
                GateReason::Reanchor,
                self.suppressed_admitted_ticks,
            );
        }
        *self = Self::default();
        self.suppressing = true;
        emit_marker("engaged", GateReason::SpawnSettle, 0);
    }

    /// Silently ends any episode without a marker.
    ///
    /// Used where movement authority itself ends: the fatal/teardown paths
    /// already report their own terminal state, and no further transmission
    /// exists to describe.
    pub(super) fn disengage(&mut self) {
        *self = Self::default();
    }

    /// Observes one newly admitted completed sample.
    ///
    /// Returns whether this admission just lifted the gate, either because
    /// the required consecutive settled run completed (`spawn_settle`) or
    /// because the suppression cap was exhausted (`timeout`). The caller
    /// must discard every sample withheld during the episode before handing
    /// off again so resumed transmission never replays suppressed ticks.
    pub(super) fn observe_admitted_sample(&mut self, sample: &PhysicsMovementSample) -> bool {
        if !self.suppressing {
            return false;
        }
        self.suppressed_admitted_ticks = self.suppressed_admitted_ticks.saturating_add(1);
        let settled = sample.grounded_after_tick && !sample.horizontal_collision;
        self.consecutive_settled_samples = if settled {
            self.consecutive_settled_samples.saturating_add(1)
        } else {
            0
        };
        if self.consecutive_settled_samples >= SETTLED_TICKS {
            self.lift(GateReason::SpawnSettle);
            return true;
        }
        if self.suppressed_admitted_ticks >= SETTLE_TIMEOUT_TICKS {
            self.lift(GateReason::Timeout);
            return true;
        }
        false
    }

    fn lift(&mut self, reason: GateReason) {
        self.suppressing = false;
        let suppressed = self.suppressed_admitted_ticks;
        emit_marker("lifted", reason, suppressed);
    }
}

#[cfg(test)]
mod tests {
    use super::{GateReason, SETTLE_TIMEOUT_TICKS, SETTLED_TICKS, SpawnSettleGate, settle_marker};
    use crate::movement::settle_tests::{colliding_sample, settled_sample};

    #[test]
    fn markers_render_the_exact_bounded_single_line_schema() {
        assert_eq!(
            settle_marker("engaged", GateReason::SpawnSettle, 0),
            "MOVEMENT_TX_GATE={\"schema\":\"rust-mcbe-movement-tx-gate-v1\",\"phase\":\"engaged\",\"reason\":\"spawn_settle\",\"ticks_suppressed\":0}"
        );
        assert_eq!(
            settle_marker("lifted", GateReason::SpawnSettle, 20),
            "MOVEMENT_TX_GATE={\"schema\":\"rust-mcbe-movement-tx-gate-v1\",\"phase\":\"lifted\",\"reason\":\"spawn_settle\",\"ticks_suppressed\":20}"
        );
        assert_eq!(
            settle_marker("lifted", GateReason::Timeout, 200),
            "MOVEMENT_TX_GATE={\"schema\":\"rust-mcbe-movement-tx-gate-v1\",\"phase\":\"lifted\",\"reason\":\"timeout\",\"ticks_suppressed\":200}"
        );
        assert_eq!(
            settle_marker("lifted", GateReason::Reanchor, 7),
            "MOVEMENT_TX_GATE={\"schema\":\"rust-mcbe-movement-tx-gate-v1\",\"phase\":\"lifted\",\"reason\":\"reanchor\",\"ticks_suppressed\":7}"
        );
    }

    #[test]
    fn engage_starts_a_fresh_window_and_reanchor_reports_the_interrupted_episode() {
        let mut gate = SpawnSettleGate::default();
        assert!(!gate.suppressing());

        gate.engage();
        assert!(gate.suppressing());

        // Progress toward settling must not survive a fresh anchor.
        for tick in 1..SETTLED_TICKS - 1 {
            assert!(
                !gate.observe_admitted_sample(&settled_sample(tick, [0.0; 3])),
                "the window stays engaged below the settled threshold"
            );
        }
        gate.engage();
        assert!(gate.suppressing());
        assert!(
            !gate.observe_admitted_sample(&settled_sample(0, [0.0; 3])),
            "a re-engaged window counts only its own samples"
        );
    }

    #[test]
    fn unstable_samples_reset_the_consecutive_run() {
        let mut gate = SpawnSettleGate::default();
        gate.engage();
        for _ in 0..(SETTLED_TICKS - 1) {
            assert!(!gate.observe_admitted_sample(&settled_sample(0, [0.0; 3])));
        }
        assert!(!gate.observe_admitted_sample(&colliding_sample(0, [0.0; 3])));
        assert!(
            !gate.observe_admitted_sample(&settled_sample(0, [0.0; 3])),
            "one unstable sample forces the clean run to restart"
        );
        assert!(gate.suppressing());
    }

    #[test]
    fn the_required_clean_run_lifts_with_spawn_settle() {
        let mut gate = SpawnSettleGate::default();
        gate.engage();
        for _ in 0..SETTLED_TICKS {
            assert!(gate.suppressing(), "the window holds until the full run");
            gate.observe_admitted_sample(&settled_sample(0, [0.0; 3]));
        }
    }

    #[test]
    fn the_cap_fails_open_before_an_incomplete_clean_run() {
        let mut gate = SpawnSettleGate::default();
        gate.engage();
        // Alternate settled and unstable samples so the consecutive run can
        // never complete before the cap does.
        for tick in 0..SETTLE_TIMEOUT_TICKS {
            let sample = if tick % 2 == 0 {
                settled_sample(tick, [0.0; 3])
            } else {
                colliding_sample(tick, [0.0; 3])
            };
            if tick + 1 == SETTLE_TIMEOUT_TICKS {
                assert!(
                    gate.observe_admitted_sample(&sample),
                    "the final capped admission fails open"
                );
                assert!(!gate.suppressing());
            } else {
                assert!(!gate.observe_admitted_sample(&sample));
            }
        }
    }

    #[test]
    fn disengage_clears_state_without_reporting_a_transition() {
        let mut gate = SpawnSettleGate::default();
        gate.engage();
        gate.disengage();
        assert_eq!(gate, SpawnSettleGate::default());
        assert!(!gate.suppressing());
    }

    #[test]
    fn observations_after_a_lift_are_ignored_until_the_next_anchor() {
        let mut gate = SpawnSettleGate::default();
        gate.engage();
        for _ in 0..SETTLED_TICKS {
            gate.observe_admitted_sample(&settled_sample(0, [0.0; 3]));
        }
        assert!(!gate.suppressing());
        assert!(
            !gate.observe_admitted_sample(&colliding_sample(0, [0.0; 3])),
            "a lifted window must not re-arm itself from sample observation"
        );
        assert!(!gate.suppressing());
    }
}
