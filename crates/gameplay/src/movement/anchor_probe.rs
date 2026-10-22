//! Bounded spawn-anchor depenetration probing (PROVISIONAL recovery policy).
//!
//! Hard movement anchors (and Replay corrections) can be installed without a
//! collision check. Before the first tick that allows travel in a freshly anchored
//! epoch this module probes the anchor against the contextual collision the
//! player's own move would resolve against
//! ([`sim::mover_collision_boxes_with_provenance`]) and, when a bounded
//! minimal-translation walk exists, moves the anchor out of the overlap. A
//! positional push-out is applied to the anchor's position only; it never
//! becomes reported movement or velocity.
//!
//! Vanilla has no instantaneous push-out; its closest-space push nudges
//! horizontal velocity against the same contextual shapes. Scaffolding the
//! body is inside, scaffolding a descent sinks through, and powder snow the
//! player sinks into therefore never count as an overlap here.
//!
//! When no bounded push-out exists (a sealed pocket, or an embedment deeper
//! than the displacement budget) the tick simply proceeds. The simulator's
//! per-axis resolution reports zero inputless horizontal motion from an
//! embedded start, so proceeding streams a stable pose rather than the
//! oscillating drift a minimal-translation ejection into transmitted velocity
//! used to produce. A failed probe optionally renders one diagnostic marker
//! (see below) but changes no transmission decision.
//!
//! Every constant here is explicitly provisional pending version-matched
//! native Bedrock measurement (VPA-109 family); none is a vanilla parity
//! claim. Unloaded chunks and unknown runtime IDs keep today's existing
//! transient-blocked behavior exactly: a probe that cannot see collision
//! data stays pending and the ordinary tick path reproduces the error.
//!
//! Failure-evidence markers (`CINNABAR_ANCHOR_PROBE` family): each failed
//! probe attempt may render ONE bounded single-line stdout marker naming the
//! sealing colliders. Emission is opt-in via the registered environment
//! literal the configured anchor-probe marker with value exactly
//! `1`; any other value, or unset, keeps every decision and allocation
//! byte-identical to the un-gated build. Formatting lives in the pure
//! [`super::anchor_probe_evidence`] module.

use std::ffi::OsStr;
use std::sync::OnceLock;

use super::anchor_probe_evidence as evidence;
use super::locomotion::{ModeIntent, ModeObservation, ModeTracker};
use super::trace::write_trace_line;

use sim::{Aabb, CollisionWorld, ProvenancedCollider, Vec3};

/// PROVISIONAL maximum number of iterative minimal-translation applications
/// one probe may spend before declaring the embedment unresolved.
pub const ANCHOR_PROBE_MAX_ITERATIONS: usize = 8;

/// PROVISIONAL maximum net feet displacement one probe may claim while
/// walking an anchor out of solids.
pub const ANCHOR_PROBE_MAX_DISPLACEMENT_BLOCKS: f64 = 1.5;

const _: () = assert!(
    ANCHOR_PROBE_MAX_ITERATIONS > 0 && ANCHOR_PROBE_MAX_DISPLACEMENT_BLOCKS > 0.0,
    "the anchor probe budget must be able to act"
);

/// What the first tick that allows travel after a fresh anchor must do.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum BeforeTick {
    /// Run the tick from the current position.
    Proceed,
    /// Move the anchor to a cleared feet origin first, then run the tick.
    Adjust(Vec3),
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

/// The exact value of the configured anchor-probe marker that enables failure-evidence
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
    *ENABLED.get_or_init(|| {
        enabled_for_env_value(
            super::diagnostics_config::value(|config| config.anchor_probe).as_deref(),
        )
    })
}

/// One bounded depenetration probe of an anchored feet origin.
///
/// Any bounded collision-query failure (unloaded chunk, unknown runtime ID,
/// or another query bound) propagates so the caller can keep today's
/// transient-blocked behavior exactly. A probe that exhausts its iteration
/// or displacement budget reports [`AnchorResolution::Unresolvable`] instead
/// of a partially moved anchor.
fn probe_anchor(
    world: &(impl CollisionWorld + ?Sized),
    mover: AnchorMover<'_>,
    feet: Vec3,
    height: f64,
) -> Result<AnchorResolution, sim::WorldQueryError> {
    world.ensure_movement_origin(feet)?;
    // The query grows by the full displacement budget so every collider the
    // probe could ever reach is visible in one bounded query. The shapes are
    // the mover's contextual collision at the anchored box, so scaffolding and
    // powder snow only count where the player's own move would collide.
    let player = Aabb::player_with_height_at(feet, height);
    let query = player.grown(ANCHOR_PROBE_MAX_DISPLACEMENT_BLOCKS);
    let instances =
        sim::mover_collision_boxes_with_provenance(world, mover.state, mover.input, player, query)?;
    let boxes = instances
        .value
        .iter()
        .map(|entry| entry.aabb)
        .collect::<Vec<_>>();
    Ok(
        match sim::depenetrate_player(
            feet,
            height,
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

/// The mover facts contextual collision reads at an anchor: the retained
/// player state (climber contact and fall distance) and this tick's input.
#[derive(Debug, Clone, Copy)]
pub(super) struct AnchorMover<'a> {
    pub state: &'a sim::PlayerState,
    pub input: &'a sim::MovementInput,
}

/// Per-controller spawn-anchor probe state. One epoch per fresh anchor.
#[derive(Debug, Clone, Copy)]
pub(super) struct AnchorProbeState {
    pending: bool,
    evidence_enabled: bool,
}

impl AnchorProbeState {
    pub(super) fn new() -> Self {
        Self {
            pending: false,
            evidence_enabled: evidence_enabled_from_env(),
        }
    }

    /// Arms a fresh probe epoch for a hard anchor.
    pub(super) fn note_hard_anchor(&mut self) {
        self.pending = true;
    }

    /// Re-arms one probe for the next tick after a Replay correction whose
    /// landing may be embedded, without disturbing evidence configuration.
    pub(super) fn rearm(&mut self) {
        self.pending = true;
    }

    /// Whether the next tick needs a pose-aware recovery probe at its new anchor.
    pub(super) const fn pending(&self) -> bool {
        self.pending
    }

    /// Clears all probe state with movement authority itself.
    pub(super) fn reset(&mut self) {
        *self = Self::new();
    }

    /// Applies pending anchor recovery before either a live tick or a replayed tick selects its pose.
    pub(super) fn prepare_tick(
        &mut self,
        world: &(impl CollisionWorld + ?Sized),
        modes: ModeTracker,
        intent: ModeIntent,
        observed: &mut ModeObservation,
        state: &sim::PlayerState,
        input: sim::MovementInput,
    ) {
        if input.immobile || input.skip_travel || !self.pending() {
            return;
        }
        if input.spectator || input.no_clip || intent.ride.is_some() {
            self.reset();
            return;
        }
        let mut preview = modes;
        // Preview only a pose that no longer fits; the real selector advances once afterward.
        if !sim::pose_fits(world, observed.feet, modes.mode(), modes.sneaking()).unwrap_or(true) {
            let _ = preview.select(intent, *observed, world);
        }
        let mover = AnchorMover {
            state,
            input: &input,
        };
        if let BeforeTick::Adjust(feet) =
            self.before_tick(world, mover, observed.feet, preview.contact_height())
        {
            observed.feet = feet;
        }
    }

    /// Decides whether pending recovery must adjust the anchor before travel.
    ///
    /// Collision-data unavailability keeps today's transient-blocked behavior
    /// exactly: the probe stays pending and the ordinary tick reproduces the
    /// query error. An unresolvable embedment proceeds — the simulator reports
    /// no inputless horizontal motion from an embedded start — after optionally
    /// rendering one diagnostic marker.
    /// `height` is the pose selected at the new anchor, so a fitting crouch or
    /// crawl is not pushed out as if the player were still standing; `mover`
    /// supplies the facts the player's contextual collision reads.
    pub(super) fn before_tick(
        &mut self,
        world: &(impl CollisionWorld + ?Sized),
        mover: AnchorMover<'_>,
        feet: Vec3,
        height: f64,
    ) -> BeforeTick {
        if !self.pending {
            return BeforeTick::Proceed;
        }
        match probe_anchor(world, mover, feet, height) {
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
                // Bounded, purely additive diagnostic: when disabled this
                // returns an empty vector before formatting anything, so
                // decisions and allocations are byte-identical either way.
                for line in evidence::failure_marker_lines(
                    self.evidence_enabled,
                    feet,
                    &colliders,
                    ANCHOR_PROBE_MAX_ITERATIONS,
                    ANCHOR_PROBE_MAX_DISPLACEMENT_BLOCKS,
                    false,
                ) {
                    write_trace_line(&line);
                }
                BeforeTick::Proceed
            }
        }
    }

    #[cfg(test)]
    pub(super) fn testing_set_evidence_enabled(&mut self, enabled: bool) {
        self.evidence_enabled = enabled;
    }
}
