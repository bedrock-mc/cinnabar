//! Borrowed world and gameplay inputs used by audio.
use assets::NetworkIdMode;
use chunk_pipeline::WorldStream;
use client_world::ActorStatusNotice;
use sim::CollisionRegistry;

/// The committed stream observed by audio without owning session transport.
#[derive(Clone, Copy, Default)]
pub struct WorldObservation<'a> {
    pub stream: Option<&'a WorldStream>,
}

/// Completed physics ticks used to schedule local motion sounds.
pub trait PhysicsObservation {
    /// Returns the last completed simulation state.
    fn state(&self) -> Option<&sim::PlayerState>;
    /// Visits completed motion ticks after the cursor, or only the latest tick to prime a new observer.
    fn visit_motion_ticks(
        &self,
        after: Option<u64>,
        visit: &mut dyn FnMut(u64, crate::audio::local::MotionSample),
    );
}

/// Collision facts used by sound material queries.
pub trait CollisionLookup {
    /// Borrows the registry matching the stream's runtime ID mode.
    fn registry(&self, mode: NetworkIdMode) -> &CollisionRegistry;
    /// Resolves a runtime block ID without mutating gameplay state.
    fn block_identifier(&self, mode: NetworkIdMode, runtime_id: u32) -> Option<&str>;
    /// Resolves a custom block's `minecraft:sound` component, when one was advertised.
    fn block_sound_material(&self, _mode: NetworkIdMode, _runtime_id: u32) -> Option<&str> {
        None
    }
}

/// Mining target observations used only to schedule local sound cues.
pub trait MiningObservation {
    /// Returns the currently admitted mining cell and face.
    fn destroying_target(&self) -> Option<([i32; 3], u8)>;
}

/// Separate particle trigger copies that the audio lane may consume.
pub trait ParticleAudioObservation {
    /// Drains the audio copy of actor status notices.
    fn take_status_audio(&mut self) -> Vec<ActorStatusNotice>;
    /// Drains the audio copy of level effects.
    fn take_level_audio(&mut self) -> Vec<(i32, [f32; 3], i32)>;
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
