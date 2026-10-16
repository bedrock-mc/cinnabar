//! Original headwear geometry shared by world players and wardrobe previews.

use std::sync::{Arc, OnceLock};

use glam::{Quat, Vec3};
use render_api::{CapeImage, Hat, SkinRgba8};

use crate::{ActorRigGeometry, ActorVertex, EntityRigId, RenderBoneTransform};

/// Layer reserved between player capes and animated skin layers.
pub const ACTOR_LAYER_HAT: u8 = 25;

/// A head attachment with optional flexible surface joints.
pub struct HatRig {
    pub geometry: ActorRigGeometry,
    parents: Vec<Option<usize>>,
    flex: Vec<f32>,
    head: usize,
    pub rest_vertices: Arc<[ActorVertex]>,
    pub texture: CapeImage,
    pub pixels: SkinRgba8,
}

/// Resolves original authored assets once, independently of installed game carriers.
pub fn rig(hat: Hat) -> &'static HatRig {
    static RIGS: OnceLock<Vec<HatRig>> = OnceLock::new();
    let index = Hat::ALL
        .iter()
        .position(|entry| *entry == hat)
        .expect("catalogued hat");
    &RIGS.get_or_init(|| Hat::ALL.iter().copied().map(model::build).collect())[index]
}

impl HatRig {
    /// Whether the authored surface contains joints that respond to movement.
    pub fn bends_in_wind(&self) -> bool {
        self.flex.iter().any(|weight| *weight > 0.0)
    }

    /// Attaches a continuous ear surface to the head, curving its upper rings progressively.
    /// Positive bend moves the tips toward model +Z, behind a player facing model -Z.
    pub fn pose(&self, head: RenderBoneTransform, bend: f32) -> Arc<[RenderBoneTransform]> {
        let rotation = Quat::from_array(head.rotation);
        let scale =
            Vec3::from_array([0, 1, 2].map(|i| head.axis_scale[i] * head.translation_scale[3]));
        let mut poses: Vec<RenderBoneTransform> = Vec::with_capacity(self.parents.len());
        for index in 0..self.parents.len() {
            let mut pose = head;
            if index != self.head {
                let parent = self.parents[index].expect("headwear is attached to the head");
                let parent_pose = poses[parent];
                let offset = Vec3::from_array(self.geometry.bone_pivots[index])
                    - Vec3::from_array(self.geometry.bone_pivots[parent]);
                let position = Vec3::from_slice(&parent_pose.translation_scale[..3])
                    + Quat::from_array(parent_pose.rotation) * (offset * scale);
                pose.translation_scale[..3].copy_from_slice(&position.to_array());
                pose.rotation =
                    (rotation * Quat::from_rotation_x(bend * self.flex[index])).to_array();
            }
            poses.push(pose);
        }
        poses.into()
    }
}

/// Smooth, bounded wind response in seconds; sideways and backward motion leave ears upright.
#[derive(Default)]
pub struct EarWind {
    bend: f32,
}

/// Projects native tick displacement onto the player's horizontal look direction.
pub fn forward_speed(velocity_per_tick: [f32; 3], yaw_degrees: f32) -> f32 {
    let (sin, cos) = yaw_degrees.to_radians().sin_cos();
    (-velocity_per_tick[0] * sin + velocity_per_tick[2] * cos)
        * simulation_clock::TICKS_PER_SECOND as f32
}

impl EarWind {
    /// Updates from forward speed in blocks/second and caller-owned elapsed seconds.
    pub fn advance(&mut self, forward_speed: f32, seconds: f32) -> f32 {
        let speed = if forward_speed.is_finite() {
            forward_speed.max(0.0)
        } else {
            0.0
        };
        let target = (speed / 6.0).clamp(0.0, 1.0) * 65.0_f32.to_radians();
        if seconds.is_finite() && seconds > 0.0 {
            let response = if target > self.bend { 12.0 } else { 8.0 };
            self.bend += (target - self.bend) * (1.0 - (-response * seconds).exp());
            if (target - self.bend).abs() < 0.0001 {
                self.bend = target;
            }
        }
        self.bend
    }
}

/// Reuses immutable poses when neither the evaluated head nor the settled wind changed.
#[derive(Default)]
pub struct HatPoseCache {
    key: Option<(Hat, RenderBoneTransform, f32)>,
    pose: Arc<[RenderBoneTransform]>,
}

impl HatPoseCache {
    /// Returns a cached pose or rebuilds exactly this head attachment.
    pub fn pose(
        &mut self,
        hat: Hat,
        head: RenderBoneTransform,
        bend: f32,
    ) -> Arc<[RenderBoneTransform]> {
        let bend = if rig(hat).bends_in_wind() { bend } else { 0.0 };
        let key = (hat, head, bend);
        if self.key != Some(key) {
            self.pose = rig(hat).pose(head, bend);
            self.key = Some(key);
        }
        self.pose.clone()
    }
}

mod model;
#[cfg(test)]
mod tests;
