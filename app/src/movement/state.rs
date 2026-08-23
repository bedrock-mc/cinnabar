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
    /// correction replay rebuilds it from retained initiations plus the
    /// replayed grounded states.
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
