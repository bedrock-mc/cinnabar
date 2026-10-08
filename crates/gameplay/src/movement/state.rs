//! Processed movement state derived by fixed-tick prediction.
//!
//! Jump initiation and arc tracking are local simulation facts. The wire's
//! Jumping bit follows held processed input; StartJumping follows initiation.
//! Other provisional state rules remain subject to native comparison.

/// Movement facts produced by one completed simulation tick.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ProcessedMovementState {
    /// The simulator consumed a jump request from the ground this tick. The
    /// simulator can only act on a jump request while grounded, so an input
    /// edge pressed in mid-air or against a wall never initiates anything.
    pub jump_initiated: bool,
    /// Local jump-arc evidence, independent of the wire Jumping flag:
    /// initiated this tick, or still
    /// carried from an earlier initiation because the simulator has not yet
    /// reported ground contact again. Session/correction resets clear it;
    /// correction replay recomputes both fields from the replayed timeline's
    /// own facts via [`ReplayJumpArcFold`].
    pub jump_arc_active: bool,
    /// Sneaking pose fed to the simulator, including a low ceiling forcing the pose.
    pub sneaking: bool,
    /// Forward-gated sprint already narrowed by [`super::physics_movement_input`].
    pub sprinting: bool,
    /// Locomotion mode the simulator ran this tick; drives the mode start/stop edges.
    pub mode: sim::MovementMode,
    /// A low ceiling holds the sneak pose although the button is up.
    pub forced_sneak: bool,
    /// The ridden mount class while `mode` is `Riding`.
    pub ride: Option<super::RideKind>,
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
            mode: sim::MovementMode::Walking,
            forced_sneak: false,
            ride: None,
        }
    }
}

/// Rebuilds jump arcs from the simulator's replayed initiation and ground facts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReplayJumpArcFold {
    arc_active: bool,
}

impl ReplayJumpArcFold {
    /// Keeps an anchor's old arc only when the correction leaves it airborne.
    #[must_use]
    pub const fn seed(
        anchor_grounded: bool,
        recorded_initiation: bool,
        recorded_arc: bool,
    ) -> Self {
        Self {
            arc_active: if anchor_grounded {
                false
            } else {
                recorded_initiation || recorded_arc
            },
        }
    }

    /// Folds the simulator's actual initiation, including refused liquid and vehicle jumps.
    pub fn step(&mut self, initiated: bool, grounded_after_tick: bool) -> (bool, bool) {
        self.arc_active = initiated || (!grounded_after_tick && self.arc_active);
        (initiated, self.arc_active)
    }

    /// The arc carried past the last folded tick.
    #[must_use]
    pub const fn arc_active(&self) -> bool {
        self.arc_active
    }
}
