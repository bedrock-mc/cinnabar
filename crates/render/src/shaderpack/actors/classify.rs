//! How a rig instance draws: its draw kind, Iris's per-draw values, Java's glint texture
//! matrix and the hand projection.

use assets::{EntityRenderMaterial, EntityRenderMaterialState};
use bevy::math::{Mat4, Vec3, Vec4};

use crate::shaderpack::draws::DrawKind;
use crate::shaderpack::uniforms::{GL_NEAR, HostValue, gl_perspective};

/// Java's glint texture-matrix scale for worn and entity glint, and for held items.
pub(super) const ENTITY_GLINT_SCALE: f32 = 0.16;
pub(super) const ITEM_GLINT_SCALE: f32 = 8.0;

/// Applies the instance's configured glint scroll with Java's texture rotation and scale.
pub(crate) fn glint_texture_matrix(glint: [f32; 6], scale: f32) -> Mat4 {
    Mat4::from_translation(Vec3::new(glint[0] * glint[3], glint[1] * glint[3], 0.0))
        * Mat4::from_rotation_z(10f32.to_radians())
        * Mat4::from_scale(Vec3::splat(scale))
}

/// The first-person hand's projection: Java's perspective at the hand's field of view with
/// its clip depth scaled by `MC_HAND_DEPTH`, which keeps hand pixels at window depths around
/// one half, where packs recognise them.
pub(crate) fn hand_projection(fov_radians: f32, screen: [u32; 2], far: f32) -> Mat4 {
    let focal_y = 1.0 / (fov_radians * 0.5).tan();
    let aspect = screen[0] as f32 / screen[1].max(1) as f32;
    let projection = gl_perspective([focal_y / aspect, focal_y], GL_NEAR, far);
    Mat4::from_scale(Vec3::new(
        1.0,
        1.0,
        shaderpack::Environment::default().hand_depth,
    )) * projection
}

/// The per-draw values Iris sets around each entity, block entity and item.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct DrawValues {
    pub entity_id: i32,
    pub block_entity_id: i32,
    pub item_id: i32,
    pub entity_color: Vec4,
    /// `gl_TextureMatrix[0]`: identity except for the glint's scroll.
    pub texture_matrix: Mat4,
}

impl DrawValues {
    pub(crate) const NONE: Self = Self {
        entity_id: -1,
        block_entity_id: -1,
        item_id: -1,
        entity_color: Vec4::ZERO,
        texture_matrix: Mat4::IDENTITY,
    };

    /// The uniform members these values patch.
    pub(crate) fn uniforms(&self) -> [(&'static str, HostValue); 6] {
        [
            ("entityId", HostValue::Int(self.entity_id)),
            ("blockEntityId", HostValue::Int(self.block_entity_id)),
            ("currentRenderedItemId", HostValue::Int(self.item_id)),
            ("entityColor", HostValue::Vec(self.entity_color, 4)),
            ("iris_TextureMat[0]", HostValue::Mat4(self.texture_matrix)),
            ("textureMatrix", HostValue::Mat4(self.texture_matrix)),
        ]
    }
}

/// How a rig instance draws: its camera draw kind and the work it needs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RigClass {
    pub kind: DrawKind,
    /// Casts a shadow (`shadow_entities`).
    pub shadow: bool,
    /// Its material culls authored back faces.
    pub one_sided: bool,
    /// Draws the enchantment glint over itself.
    pub glint: bool,
}

/// The draw kind and state of a rig instance with `material` (an actor-pass material word),
/// or `None` for depth-only helpers. `blend` overrides the material's blending (held items);
/// `hand` selects the first-person kinds.
pub(crate) fn classify(
    material: u32,
    item_extent: f32,
    blend: Option<bool>,
    hand: bool,
) -> Option<RigClass> {
    let kind_bits = material & EntityRenderMaterialState::KIND_MASK;
    if kind_bits == EntityRenderMaterial::DissolveDepth as u32 {
        return None;
    }
    let state = crate::actor::material::state(material);
    let blended = blend.unwrap_or(state.is_some_and(|state| state.blend));
    let additive = state.is_some_and(|state| state.blend && state.additive);
    let kind = match (hand, additive, blended) {
        (true, _, true) => DrawKind::HandWater,
        (true, _, false) => DrawKind::Hand,
        (false, true, _) => DrawKind::SpiderEyes,
        (false, false, true) => DrawKind::EntitiesTranslucent,
        (false, false, false) => DrawKind::Entities,
    };
    Some(RigClass {
        kind,
        shadow: !hand && !additive,
        one_sided: state.is_some_and(|state| state.cull),
        glint: kind_bits == EntityRenderMaterial::Glint as u32 || item_extent != 0.0,
    })
}

/// A little-endian RGBA8 word as unit floats.
fn unpack_rgba8(word: u32) -> Vec4 {
    Vec4::from_array(word.to_le_bytes().map(|byte| f32::from(byte) / 255.0))
}

/// The vertex colour of an instance: its dye (`change_color`), white without one; blended
/// materials also take the dye's alpha, as the actor pass multiplies them by it.
pub(crate) fn vertex_colour(tint: u32, blended: bool) -> u32 {
    if tint == 0 {
        return u32::MAX;
    }
    if blended { tint } else { tint | 0xff00_0000 }
}

/// The pack's `entityColor` of an instance: its overlay, none where the material disables it.
pub(crate) fn entity_colour(overlay: u32, material: u32) -> Vec4 {
    if crate::actor::material::state(material).is_some_and(|state| state.disable_overlay) {
        return Vec4::ZERO;
    }
    unpack_rgba8(overlay)
}
