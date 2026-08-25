//! Processed movement state derived by fixed-tick prediction.
//!
//! Vanilla Bedrock separates the raw button state it receives from the
//! movement states its simulation actually acts on, and the outbound
//! `PlayerAuthInput` flag families mirror that split (raw button carriers
//! versus processed state carriers; VPA-011). Flag identity and wire order are
//! pinned by Mojang's published protocol documentation and the vendored
//! protocol-2168 packet definitions. The exact vanilla lifecycle of each flag
//! has not been measured against a version-matched native client, so every
//! rule below is Cinnabar's explicit provisional contract — recorded here so a
//! future native measurement can replace it deliberately instead of silently.

/// One completed tick's processed movement states.
///
/// Produced by the physics layer alongside each [`super::PhysicsMovementSample`]
/// and consumed by the outbound encoder, so wire flags always describe what
/// the simulator acted on rather than merely which buttons are held.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ProcessedMovementState {
    /// The simulator consumed a jump request from the ground this tick. The
    /// simulator can only act on a jump request while grounded, so an input
    /// edge pressed in mid-air or against a wall never initiates anything.
    pub jump_initiated: bool,
    /// A simulated jump arc is in progress: initiated this tick, or still
    /// carried from an earlier initiation because the simulator has not yet
    /// reported ground contact again. Session/correction resets clear it;
    /// correction replay recomputes both fields from the replayed timeline's
    /// own facts via [`ReplayJumpArcFold`].
    pub jump_arc_active: bool,
    /// Sneaking state fed to the simulator this tick. No pose/mode authority
    /// exists yet (VPA-012), so processed sneak equals held sneak; any future
    /// pose-gated rule lands here without changing callers.
    pub sneaking: bool,
    /// Forward-gated sprint already narrowed by [`super::physics_movement_input`].
    pub sprinting: bool,
}

impl ProcessedMovementState {
    /// Folds one completed tick's facts into the next processed snapshot.
    ///
    /// The jump arc opens on a ground-consumed initiation and stays open
    /// across airborne ticks; the first tick the simulator reports grounded
    /// contact again closes it. This mirrors the discrete ladder-climb rule
    /// already used for axis collisions: motion state describes what the
    /// simulation did, never a guess about what a held button might do.
    #[must_use]
    pub fn next(
        previous_jump_arc_active: bool,
        jump_initiated: bool,
        grounded_after_tick: bool,
        sneaking: bool,
        sprinting: bool,
    ) -> Self {
        Self {
            jump_initiated,
            jump_arc_active: jump_initiated || (!grounded_after_tick && previous_jump_arc_active),
            sneaking,
            sprinting,
        }
    }
}

/// Sequential rebuild of processed jump initiations across a correction
/// replay.
///
/// A correction rewinds simulated outcomes but replays byte-identical
/// retained inputs, so each replayed tick's initiation is a fact of the
/// replayed timeline rather than of the contradicted original prediction.
/// The simulator consumes a request only on a tick that starts grounded with
/// its post-jump cooldown expired, and it clears that cooldown whenever the
/// button is not held. This fold reproduces those pre-tick facts in the
/// simulator's own order — cooldown clear, ground-gated consumption,
/// end-of-tick decrement — starting from the server-corrected anchor state
/// the replay itself starts from, so rebuilt initiations match exactly what
/// the replayed simulation acted on: a server-contradicted takeoff can
/// neither assert a phantom arc nor silence a genuinely replayed one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ReplayJumpArcFold {
    grounded: bool,
    jump_delay: u8,
    arc_active: bool,
}

impl ReplayJumpArcFold {
    /// Seeds the fold with the corrected anchor's post-tick facts.
    ///
    /// `recorded_initiation` and `recorded_arc` are the retained prediction's
    /// states at the anchor tick. They carry into the replayed range only
    /// while the anchor stayed airborne: a server-reported ground contact
    /// outranks a retained initiation (the correction just contradicted this
    /// client's takeoff prediction), mirroring the live fold's landing rule.
    #[must_use]
    pub(crate) const fn seed(
        anchor_grounded: bool,
        anchor_jump_delay: u8,
        recorded_initiation: bool,
        recorded_arc: bool,
    ) -> Self {
        Self {
            grounded: anchor_grounded,
            jump_delay: anchor_jump_delay,
            arc_active: if anchor_grounded {
                false
            } else {
                recorded_initiation || recorded_arc
            },
        }
    }

    /// Folds one replayed tick and returns its rebuilt `(initiation, arc)`.
    ///
    /// `input` must be the exact retained request the replay fed to the
    /// simulator; `grounded_after_tick` is that tick's fresh result.
    pub(crate) fn step(
        &mut self,
        input: &sim::MovementInput,
        grounded_after_tick: bool,
    ) -> (bool, bool) {
        // The simulator clears a retained post-jump cooldown whenever the
        // button is not held, then consumes requests only from pre-tick
        // ground contact with the cleared delay expired, then decrements the
        // retained delay once at end of tick (a consumed request sets it to
        // [`sim::JUMP_DELAY_TICKS`] before that decrement).
        let cleared_delay = if input.jumping { self.jump_delay } else { 0 };
        let initiated = input.jump_pressed && self.grounded && cleared_delay == 0;
        self.jump_delay = if initiated {
            sim::JUMP_DELAY_TICKS.saturating_sub(1)
        } else {
            cleared_delay.saturating_sub(1)
        };
        self.grounded = grounded_after_tick;
        self.arc_active = initiated || (!grounded_after_tick && self.arc_active);
        (initiated, self.arc_active)
    }

    /// The arc carried past the last folded tick.
    #[must_use]
    pub(crate) const fn arc_active(&self) -> bool {
        self.arc_active
    }
}
