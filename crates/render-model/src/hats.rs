//! Catalog attachments shared by world players and wardrobe previews.
use crate::{ActorRigGeometry, ActorVertex, EntityRigId, RenderBoneTransform};
use cosmetics::Item;
use glam::{Quat, Vec3};
use render_api::{CapeImage, SkinRgba8};
use std::sync::{Arc, RwLock};

/// First cosmetic attachment layer, following animated skins and world flames.
pub const ACTOR_LAYER_HAT: u8 = crate::ACTOR_LAYER_FLAME + 1;
const _: () = assert!(cosmetics::SLOT_COUNT <= (u8::MAX - ACTOR_LAYER_HAT) as usize);
/// Exclusive end of the cosmetic range; authored controller layers start here.
pub const ACTOR_LAYER_HAT_END: u8 = ACTOR_LAYER_HAT + cosmetics::SLOT_COUNT as u8;

/// Distinguishes every supported cosmetic slot from skins, flames and equipment.
pub const fn is_cosmetic_layer(layer: u8) -> bool {
    layer >= ACTOR_LAYER_HAT && layer < ACTOR_LAYER_HAT_END
}

pub struct HatRig {
    pub geometry: ActorRigGeometry,
    parents: Vec<Option<usize>>,
    flex: Vec<f32>,
    rest_rotations: Vec<Quat>,
    head: usize,
    offset: [f32; 3],
    wind: Option<cosmetics_animation::Wind>,
    playback: Option<cosmetics_animation::Playback>,
    pub rest_vertices: Arc<[ActorVertex]>,
    preview_part: u32,
    preview_frame: std::sync::OnceLock<thumbnail::Frame>,
    pub texture: CapeImage,
    pub pixels: SkinRgba8,
    world_pixels: Option<SkinRgba8>,
    emitters: Box<[emitters::CosmeticEmitter]>,
    cloth: Option<cloth::ClothRig>,
}

static RIGS: RwLock<Vec<(Item, &'static HatRig)>> = RwLock::new(Vec::new());

/// Resolves immutable geometry for a handle admitted with all of its verified assets.
pub fn rig(item: Item) -> &'static HatRig {
    RIGS.read()
        .unwrap_or_else(|error| error.into_inner())
        .iter()
        .find(|(key, _)| *key == item)
        .expect("admitted cosmetic rig")
        .1
}

/// Installs verified content once, bounding immutable per-process model allocations.
pub fn admit(
    descriptor: cosmetics::Descriptor,
    geometry: &[u8],
    texture: &[u8],
    animation: &[u8],
) -> Option<Item> {
    if !cosmetics::supported(&descriptor)
        || !descriptor.assets.geometry.verifies(geometry)
        || !descriptor.assets.texture.verifies(texture)
        || descriptor
            .assets
            .animation
            .as_ref()
            .is_some_and(|asset| !asset.verifies(animation))
    {
        return None;
    }
    let mut rigs = RIGS.write().unwrap_or_else(|error| error.into_inner());
    if let Some((item, _)) = rigs
        .iter()
        .find(|(item, _)| item.descriptor() == &descriptor)
    {
        return Some(*item);
    }
    let mut rig = model::build(
        &descriptor,
        std::str::from_utf8(geometry).ok()?,
        texture,
        animation,
    )?;
    let item = Item::admit(descriptor)?;
    rig.geometry.id = EntityRigId(0xDFFF_FFFF - u32::from(item.index()));
    rigs.push((item, Box::leak(Box::new(rig))));
    Some(item)
}

impl HatRig {
    /// Whether the item declares a fabric surface needing per-actor simulation history.
    pub fn has_cloth(&self) -> bool {
        self.cloth.is_some()
    }
    /// Whether an authored animation changes any attachment joints.
    pub fn bends_in_wind(&self) -> bool {
        self.wind.is_some() && self.flex.iter().any(|weight| *weight > 0.0)
    }

    /// Attaches authored joints to the evaluated bone with catalog offsets and bounded motion.
    pub fn pose(
        &self,
        mut attachment: RenderBoneTransform,
        motion: Motion,
    ) -> Arc<[RenderBoneTransform]> {
        let rotation = Quat::from_array(attachment.rotation);
        let scale = Vec3::from_array(
            [0, 1, 2].map(|i| attachment.axis_scale[i] * attachment.translation_scale[3]),
        );
        let position = Vec3::from_slice(&attachment.translation_scale[..3])
            + rotation * (Vec3::from_array(self.offset) * scale);
        attachment.translation_scale[..3].copy_from_slice(&position.to_array());
        let curvature = self
            .playback
            .as_ref()
            .map_or(0.0, |clip| clip.curvature(motion.seconds));
        let mut poses: Vec<RenderBoneTransform> = Vec::with_capacity(self.parents.len());
        for index in 0..self.parents.len() {
            let mut pose = attachment;
            if index == self.head {
                let delta = self
                    .playback
                    .as_ref()
                    .map_or(Vec3::ZERO, |clip| clip.translation(index, motion.seconds));
                let position = position + rotation * (delta * scale);
                pose.translation_scale[..3].copy_from_slice(&position.to_array());
                pose.rotation = (rotation
                    * self.rest_rotations[index]
                    * self
                        .playback
                        .as_ref()
                        .map_or(Quat::IDENTITY, |clip| clip.rotation(index, motion.seconds)))
                .to_array();
            }
            if index != self.head {
                let parent = self.parents[index].expect("admitted attachment parent");
                let parent_pose = poses[parent];
                let offset = Vec3::from_array(self.geometry.bone_pivots[index])
                    - Vec3::from_array(self.geometry.bone_pivots[parent]);
                let (delta, span_rotation) = self
                    .playback
                    .as_ref()
                    .map_or((Vec3::ZERO, Quat::IDENTITY), |clip| {
                        clip.span(index, curvature)
                    });
                let position = Vec3::from_slice(&parent_pose.translation_scale[..3])
                    + Quat::from_array(parent_pose.rotation)
                        * ((offset
                            + delta
                            + self.playback.as_ref().map_or(Vec3::ZERO, |clip| {
                                clip.translation(index, motion.seconds)
                            }))
                            * scale);
                pose.translation_scale[..3].copy_from_slice(&position.to_array());
                let axis = self
                    .wind
                    .as_ref()
                    .map_or(Vec3::X, |wind| match wind.axis.as_str() {
                        "y" => Vec3::Y,
                        "z" => Vec3::Z,
                        _ => Vec3::X,
                    });
                pose.rotation = (Quat::from_array(parent_pose.rotation)
                    * self.rest_rotations[index]
                    * Quat::from_axis_angle(
                        axis,
                        motion.bend * (self.flex[index] - self.flex[parent]),
                    )
                    * self
                        .playback
                        .as_ref()
                        .map_or(Quat::IDENTITY, |clip| clip.rotation(index, motion.seconds))
                    * span_rotation)
                    .to_array();
            }
            poses.push(pose);
        }
        poses.into()
    }
}

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
    /// Evaluates catalog motion using caller-owned forward speed and elapsed seconds.
    pub fn advance(&mut self, item: Item, speed: f32, seconds: f32) -> f32 {
        let Some(wind) = rig(item).wind.as_ref() else {
            self.bend = 0.0;
            return 0.0;
        };
        let speed = if speed.is_finite() {
            speed.max(0.0)
        } else {
            0.0
        };
        let target = (speed / wind.speed).clamp(0.0, 1.0) * wind.max_degrees.to_radians();
        if seconds.is_finite() && seconds > 0.0 {
            let response = if target > self.bend {
                wind.rise
            } else {
                wind.fall
            };
            self.bend += (target - self.bend) * (1.0 - (-response * seconds).exp());
            if (target - self.bend).abs() < 0.0001 {
                self.bend = target;
            }
        }
        self.bend
    }
}

/// Evaluated cosmetic motion shared by world and HUD renderers.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Motion {
    pub bend: f32,
    pub seconds: f32,
}
impl Motion {
    /// Advances caller-owned playback time; invalid frame deltas leave the phase unchanged.
    pub fn advance(&mut self, item: Item, seconds: f32) {
        self.seconds = match &rig(item).playback {
            Some(clip) if seconds.is_finite() && seconds > 0.0 => {
                clip.normalize(f64::from(self.seconds) + f64::from(seconds))
            }
            Some(_) => self.seconds,
            None => 0.0,
        };
    }
}

#[derive(Default)]
pub struct HatPoseCache {
    anchor: Option<(Item, Arc<crate::ActorAttachmentBindings>, Option<usize>)>,
    key: Option<(Item, RenderBoneTransform, Motion)>,
    pose: Arc<[RenderBoneTransform]>,
}
impl HatPoseCache {
    /// Rebinds an equipped anchor only when its item or retained appearance changes.
    /// Newly admitted items use the same skeleton index without rebuilding the appearance.
    pub fn anchor(
        &mut self,
        item: Item,
        bindings: Option<&Arc<crate::ActorAttachmentBindings>>,
        names: &[Box<str>],
    ) -> Option<usize> {
        let Some(bindings) = bindings else {
            #[cfg(test)]
            crate::actor_bindings::ANCHOR_LOOKUPS.with(|count| count.set(count.get() + 1));
            return names
                .iter()
                .position(|name| name.eq_ignore_ascii_case(&item.descriptor().attachment_bone));
        };
        if let Some((previous, appearance, bone)) = &self.anchor
            && *previous == item
            && Arc::ptr_eq(appearance, bindings)
        {
            return *bone;
        }
        let bone = bindings.anchor(item);
        self.anchor = Some((item, Arc::clone(bindings), bone));
        bone
    }

    /// Reuses immutable poses when the attachment bone and its evaluated motion are unchanged.
    pub fn pose(
        &mut self,
        item: Item,
        head: RenderBoneTransform,
        mut motion: Motion,
    ) -> Arc<[RenderBoneTransform]> {
        let model = rig(item);
        if !model.bends_in_wind() || !motion.bend.is_finite() {
            motion.bend = 0.0;
        }
        motion.seconds = model
            .playback
            .as_ref()
            .filter(|_| motion.seconds.is_finite())
            .map_or(0.0, |clip| clip.normalize(f64::from(motion.seconds)));
        let key = (item, head, motion);
        if self.key != Some(key) {
            self.pose = model.pose(head, motion);
            self.key = Some(key);
        }
        self.pose.clone()
    }
}

#[cfg(test)]
mod animation_tests;
#[cfg(test)]
mod catalog_tests;
mod clip_playback;
mod cosmetics_animation;
#[cfg(test)]
mod emitter_tests;
mod emitters;
pub use emitters::{
    COSMETIC_EFFECT_NAMESPACE, COSMETIC_PARTICLE_TEXTURE, COSMETIC_SPRITE_SIDE, CosmeticEmitter,
    CosmeticSprite, MAX_COSMETIC_EMITTERS,
};
pub mod cloth;
#[cfg(any(test, feature = "test-support"))]
pub mod fixture;
#[cfg(test)]
mod flight_tests;
mod model;
mod preview;
#[cfg(test)]
mod tests;
pub mod thumbnail;
