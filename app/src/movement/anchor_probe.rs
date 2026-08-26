//! Bounded spawn-anchor depenetration probing (PROVISIONAL recovery policy).
//!
//! Live third-party evidence (2026-08-22/25): hard movement anchors are
//! installed without any collision check, and when the anchor overlaps solids
//! the pinned tick resolution turns depenetration minimal-translation vectors
//! into genuine oscillating position AND velocity under zero input — exactly
//! the inputless-drift signature third-party anti-cheats reject as
//! "movement cheats". After every hard anchor this module probes the anchor
//! against the same collision [`sim::PaletteWorld`] the frame already builds
//! and, while a bounded fix exists, moves the anchor out of any overlap
//! before the first simulated tick runs.
//!
//! When no bounded fix exists (a sealed pocket, or an embedment deeper than
//! the displacement budget), the state machine degrades honestly instead of
//! manufacturing garbage motion:
//!
//! 1. the first failed probe freezes simulation advancement inside an
//!    embedded-anchor hold (`settle::SpawnSettleGate::enter_embedded_hold`)
//!    that withholds transmission until a server correction, MovePlayer,
//!    respawn, or StartGame snap re-anchors and re-probes;
//! 2. the unchanged provisional fail-open cap bounds each hold episode;
//! 3. after [`EMBEDDED_HOLD_MAX_FAILED_PROBES_PER_EPOCH`] failed probes in
//!    one re-anchor epoch probing stops entirely and today's fail-open
//!    streaming behavior resumes, so a server that keeps snapping the player
//!    into geometry can never create an unbounded correction fight-loop.
//!
//! Every constant here is explicitly provisional pending version-matched
//! native Bedrock measurement (VPA-109 family); none is a vanilla parity
//! claim. Unloaded chunks and unknown runtime IDs keep today's existing
//! transient-blocked behavior exactly: a probe that cannot see collision
//! data stays pending and the ordinary tick path reproduces the error.
//!
//! Failure-evidence markers (`RUST_MCBE_ANCHOR_PROBE` family): each failed
//! probe attempt may additionally render ONE bounded single-line stdout
//! marker naming the sealing colliders, plus one `phase:"degraded"` marker
//! when that attempt spends the per-epoch budget. Emission is opt-in via
//! the registered environment literal [`crate::acceptance::markers::
//! ANCHOR_PROBE`] with value exactly `1`; any other value, or unset, keeps
//! every decision and allocation byte-identical to the un-gated build.
//! Formatting lives in the pure [`super::anchor_probe_evidence`] module;
//! during an embedded hold nothing is ever emitted because this state
//! machine short-circuits before any evidence path.

use std::ffi::OsStr;
use std::sync::OnceLock;

use super::anchor_probe_evidence as evidence;
use super::trace::write_trace_line;
use crate::acceptance::markers;
use sim::{Aabb, CollisionWorld, ProvenancedCollider, Vec3};

/// PROVISIONAL maximum number of iterative minimal-translation applications
/// one probe may spend before declaring the embedment unresolved.
pub(crate) const ANCHOR_PROBE_MAX_ITERATIONS: usize = 8;

/// PROVISIONAL maximum net feet displacement one probe may claim while
/// walking an anchor out of solids.
pub(crate) const ANCHOR_PROBE_MAX_DISPLACEMENT_BLOCKS: f64 = 1.5;

/// PROVISIONAL number of failed probes allowed per re-anchor epoch. Once
/// spent, probing stops for that epoch and the pre-existing fail-open
/// streaming behavior resumes unchanged.
const EMBEDDED_HOLD_MAX_FAILED_PROBES_PER_EPOCH: u32 = 3;

const _: () = assert!(
    ANCHOR_PROBE_MAX_ITERATIONS > 0 && ANCHOR_PROBE_MAX_DISPLACEMENT_BLOCKS > 0.0,
    "the anchor probe budget must be able to act"
);

/// What the first simulated tick after a hard anchor must do.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum BeforeTick {
    /// Run the tick from the current position.
    Proceed,
    /// Move the anchor to a cleared feet origin first, then run the tick.
    Adjust(Vec3),
    /// Enter the embedded-anchor hold instead of simulating garbage motion.
    Hold,
}

enum AnchorResolution {
    Cleared(Vec3),
    /// The colliders the failed query already fetched — with their exact
    /// palette provenance when the world surface can supply it — so evidence
    /// can name the sealing geometry without a second world query.
    Unresolvable {
        colliders: Vec<ProvenancedCollider>,
    },
}

/// The exact value of [`markers::ANCHOR_PROBE`] that enables failure-evidence
/// markers; every other value, or an unset variable, keeps today's exact
/// behavior with no formatting or allocation.
const EVIDENCE_ENABLED_VALUE: &str = "1";

/// Pure enablement rule for one environment-variable observation.
fn enabled_for_env_value(value: Option<&OsStr>) -> bool {
    value == Some(OsStr::new(EVIDENCE_ENABLED_VALUE))
}

/// Whether this process emits anchor-probe failure evidence. Evaluated at
/// most once; repeated state constructions read the cached result.
fn evidence_enabled_from_env() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED
        .get_or_init(|| enabled_for_env_value(std::env::var_os(markers::ANCHOR_PROBE).as_deref()))
}

/// One bounded depenetration probe of an anchored feet origin.
///
/// Any bounded collision-query failure (unloaded chunk, unknown runtime ID,
/// or another query bound) propagates so the caller can keep today's
/// transient-blocked behavior exactly. A probe that exhausts its iteration
/// or displacement budget reports [`AnchorResolution::Unresolvable`] instead
/// of a partially moved anchor.
fn probe_anchor(
    world: &impl CollisionWorld,
    feet: Vec3,
) -> Result<AnchorResolution, sim::WorldQueryError> {
    // The query grows by the full displacement budget so every collider the
    // probe could ever reach is visible in one bounded query through the
    // same PaletteWorld the frame already builds. The provenance surface
    // shares one emission core with `collision_boxes`, so the ordered box
    // sequence fed to depenetration — and therefore every probe decision —
    // is identical to the established box-only path (pinned by the sim
    // crate's dual-surface regression).
    let query = Aabb::player_at(feet).grown(ANCHOR_PROBE_MAX_DISPLACEMENT_BLOCKS);
    let instances = world.collision_boxes_with_provenance(query)?;
    let boxes = instances
        .value
        .iter()
        .map(|entry| entry.aabb)
        .collect::<Vec<_>>();
    Ok(
        match sim::depenetrate_player(
            feet,
            &boxes,
            ANCHOR_PROBE_MAX_ITERATIONS,
            ANCHOR_PROBE_MAX_DISPLACEMENT_BLOCKS,
        ) {
            Some(clear_feet) => AnchorResolution::Cleared(clear_feet),
            None => AnchorResolution::Unresolvable {
                colliders: instances.value,
            },
        },
    )
}

/// Per-controller spawn-anchor probe state. One epoch per hard anchor.
#[derive(Debug, Clone)]
pub(super) struct AnchorProbeState {
    pending: bool,
    failed_probes: u32,
    holding: bool,
    evidence_enabled: bool,
}

impl AnchorProbeState {
    pub(super) fn new() -> Self {
        Self {
            pending: false,
            failed_probes: 0,
            holding: false,
            evidence_enabled: evidence_enabled_from_env(),
        }
    }

    /// Arms a fresh probe epoch: any prior failure budget or frozen hold is
    /// replaced by the newly anchored position.
    pub(super) fn note_hard_anchor(&mut self) {
        *self = Self {
            pending: true,
            ..Self::new()
        };
    }

    /// Clears all probe state with movement authority itself.
    pub(super) fn reset(&mut self) {
        *self = Self::new();
    }

    pub(super) const fn holding(&self) -> bool {
        self.holding
    }

    /// Decides what must happen before the first simulated tick of a frame.
    ///
    /// Collision-data unavailability keeps today's transient-blocked behavior
    /// exactly: the probe stays pending and the ordinary tick reproduces the
    /// query error.
    pub(super) fn before_tick(&mut self, world: &impl CollisionWorld, feet: Vec3) -> BeforeTick {
        if self.holding || !self.pending {
            return BeforeTick::Proceed;
        }
        match probe_anchor(world, feet) {
            Err(_) => {
                // Unloaded chunks and unknown runtime IDs keep today's
                // existing transient-blocked behavior; retry the probe once
                // collision data returns.
                BeforeTick::Proceed
            }
            Ok(AnchorResolution::Cleared(clear_feet)) => {
                self.pending = false;
                BeforeTick::Adjust(clear_feet)
            }
            Ok(AnchorResolution::Unresolvable { colliders }) => {
                self.pending = false;
                self.failed_probes = self.failed_probes.saturating_add(1);
                let epoch_spent = self.failed_probes >= EMBEDDED_HOLD_MAX_FAILED_PROBES_PER_EPOCH;
                // Bounded, purely additive evidence: when disabled this
                // returns an empty vector before formatting anything, so
                // decisions and allocations are byte-identical either way.
                for line in evidence::failure_marker_lines(
                    self.evidence_enabled,
                    feet,
                    &colliders,
                    ANCHOR_PROBE_MAX_ITERATIONS,
                    ANCHOR_PROBE_MAX_DISPLACEMENT_BLOCKS,
                    epoch_spent,
                ) {
                    write_trace_line(&line);
                }
                if epoch_spent {
                    // Bounded degrade: stop probing this epoch and fall back
                    // to today's fail-open streaming behavior so a server
                    // that keeps snapping the player into geometry cannot
                    // create an unbounded correction fight-loop.
                    BeforeTick::Proceed
                } else {
                    self.holding = true;
                    BeforeTick::Hold
                }
            }
        }
    }

    /// Releases a frozen hold after the gate's cap failed it open, re-arming
    /// one more probe attempt while this epoch still has failure budget.
    pub(super) fn release_after_cap(&mut self) {
        self.holding = false;
        self.pending = self.failed_probes < EMBEDDED_HOLD_MAX_FAILED_PROBES_PER_EPOCH;
    }

    #[cfg(test)]
    pub(super) const fn failed_probes(&self) -> u32 {
        self.failed_probes
    }

    #[cfg(test)]
    pub(super) fn testing_set_evidence_enabled(&mut self, enabled: bool) {
        self.evidence_enabled = enabled;
    }
}
