//! Borrowed facts supplied at the caller's existing ordered frame boundary.

use assets::NetworkIdMode;
use chunk_pipeline::WorldStream;
use semantic_input::{Action, ActionPhase, ActionSnapshot};
use sim::{CollisionRegistry, PlayerState, WorldCollisionIdentity};

/// The committed stream observed by presentation without owning session transport.
#[derive(Clone, Copy, Default)]
pub struct WorldObservation<'a> {
    pub stream: Option<&'a WorldStream>,
}

/// Read-only physics facts; the gameplay owner keeps prediction and reconciliation.
pub trait PhysicsObservation {
    /// Returns the last completed simulation state.
    fn state(&self) -> Option<&PlayerState>;
    /// Returns the sneak and sprint flags of the last completed tick.
    fn latest_sneak_sprint(&self) -> Option<(bool, bool)>;
    /// Returns the pose selected by the completed simulation tick.
    fn mode(&self) -> sim::MovementMode;
    /// Returns the consecutive gliding ticks of the completed simulation.
    fn fall_fly_ticks(&self) -> u32;
    /// Returns the collision identity used by the completed tick.
    fn last_world_identity(&self) -> Option<&WorldCollisionIdentity>;
    /// Reports whether gameplay currently owns player translation.
    fn is_active(&self) -> bool;
    /// How far the frame sits between the last two completed ticks.
    fn tick_alpha(&self) -> f32 {
        1.0
    }
}

/// Collision facts used by camera obstruction and actor surface queries.
pub trait CollisionLookup {
    /// Borrows the registry matching the stream's runtime ID mode.
    fn registry(&self, mode: NetworkIdMode) -> &CollisionRegistry;
    /// Returns the canonical block state used by render-side material sampling.
    fn block_canonical_state(&self, mode: NetworkIdMode, runtime_id: u32) -> Option<&str>;
    /// Resolves a runtime block ID without mutating gameplay state.
    fn block_identifier(&self, mode: NetworkIdMode, runtime_id: u32) -> Option<&str>;
}

/// Current admitted item-use state.
pub trait ItemUseObservation {
    /// Reports whether gameplay has admitted continuous item use.
    fn is_using(&self) -> bool;
}

/// Borrowed semantic actions after input authority is finalized.
#[derive(Clone, Copy, Default)]
pub struct InputObservation<'a>(pub Option<&'a ActionSnapshot>);
impl InputObservation<'_> {
    /// Borrows the finalized action snapshot.
    pub fn snapshot(&self) -> Option<&ActionSnapshot> {
        self.0
    }
    /// Returns the finalized movement axes, or idle axes before the first frame.
    pub fn movement(&self) -> [f32; 2] {
        self.0.map_or([0.0; 2], |value| value.movement)
    }
    /// Returns the routed look delta.
    pub fn look_delta(&self) -> [f32; 2] {
        self.0.map_or([0.0; 2], |value| value.look_delta)
    }
    /// Returns the action's finalized phase.
    pub fn phase(&self, action: Action) -> ActionPhase {
        self.0.map_or(ActionPhase::default(), |value| {
            value.phases[action as usize]
        })
    }
}

/// The current local session generation, sampled without retaining its owner.
#[derive(Clone, Copy)]
pub struct SessionObservation(pub u64);
impl SessionObservation {
    /// Returns the session generation supplied for this system invocation.
    pub const fn session_generation(self) -> u64 {
        self.0
    }
}

/// The UI authority's current cursor policy.
#[derive(Clone, Copy)]
pub struct CursorPolicy {
    /// Focus ownership authorizes capture only after an explicit gameplay return.
    pub capture_allowed: bool,
    pub consent: bool,
    pub absorbs_input: bool,
    pub steals_mouse: Option<bool>,
    /// A developer controller owns input: leave the OS cursor and held input alone.
    pub driven: bool,
}
