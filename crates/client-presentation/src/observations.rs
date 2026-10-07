//! Borrowed facts supplied at the caller's existing ordered frame boundary.

use assets::NetworkIdMode;
use chunk_pipeline::WorldStream;
use client_world::ActorStatusNotice;
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
    /// Returns the collision identity used by the completed tick.
    fn last_world_identity(&self) -> Option<&WorldCollisionIdentity>;
    /// Reports whether gameplay currently owns player translation.
    fn is_active(&self) -> bool;
    /// Visits completed motion ticks after the cursor, or only the latest tick to prime a new observer.
    fn visit_motion_ticks(
        &self,
        after: Option<u64>,
        visit: &mut dyn FnMut(u64, crate::audio::local::MotionSample),
    );
    /// How far the frame sits between the last two completed ticks.
    fn tick_alpha(&self) -> f32 {
        1.0
    }
}

/// Collision facts used by camera obstruction and sound material queries.
pub trait CollisionLookup {
    /// Borrows the registry matching the stream's runtime ID mode.
    fn registry(&self, mode: NetworkIdMode) -> &CollisionRegistry;
    /// Returns the canonical block state used by render-side material sampling.
    fn block_canonical_state(&self, mode: NetworkIdMode, runtime_id: u32) -> Option<&str>;
    /// Resolves a runtime block ID without mutating gameplay state.
    fn block_identifier(&self, mode: NetworkIdMode, runtime_id: u32) -> Option<&str>;
}

/// Mining target observations used only to schedule local sound cues.
pub trait MiningObservation {
    /// Returns the currently admitted mining cell and face.
    fn destroying_target(&self) -> Option<([i32; 3], u8)>;
}

/// Current admitted item-use state.
pub trait ItemUseObservation {
    /// Reports whether gameplay has admitted continuous item use.
    fn is_using(&self) -> bool;
}

/// Separate particle trigger copies that the audio lane may consume.
pub trait ParticleAudioObservation {
    /// Drains the audio copy of actor status notices.
    fn take_status_audio(&mut self) -> Vec<ActorStatusNotice>;
    /// Drains the audio copy of level effects.
    fn take_level_audio(&mut self) -> Vec<(i32, [f32; 3], i32)>;
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
    pub consent: bool,
    pub absorbs_input: bool,
    pub steals_mouse: Option<bool>,
    /// A developer controller owns input: leave the OS cursor and held input alone.
    pub driven: bool,
}
