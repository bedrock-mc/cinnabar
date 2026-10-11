use glam::{Mat4, Vec3};
pub const MAX_ACTOR_RENDER_DISTANCE_BLOCKS: f32 = 192.0;

/// Vanilla gathers non-player render candidates no farther than this from the camera on any
/// axis (`min(radius, 72)`); players are added apart.
pub const ACTOR_CANDIDATE_RADIUS_BLOCKS: f32 = 72.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ActorCullView {
    pub clip_from_world: Mat4,
    pub camera_position: Vec3,
    pub max_distance: f32,
}

impl ActorCullView {
    /// Tests actor distance independently of model scale, failing open for invalid views.
    pub fn contains_distance(&self, feet: [f32; 3]) -> bool {
        if !self.is_valid() {
            return true;
        }
        let distance = (Vec3::from_array(feet) + Vec3::Y).distance_squared(self.camera_position);
        distance.partial_cmp(&(self.max_distance * self.max_distance))
            != Some(std::cmp::Ordering::Greater)
    }

    /// Whether the view can safely reject an actor using distance or clip bounds.
    pub(super) fn is_valid(&self) -> bool {
        self.clip_from_world.is_finite()
            && self.camera_position.is_finite()
            && self.max_distance.is_finite()
            && self.max_distance > 0.0
    }
}
