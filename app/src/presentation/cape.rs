//! Player capes: the cape geometry drawn with the player's own pose and a cape raster carried
//! in the skin layer payload.
use std::sync::Arc;

use assets::RuntimeEntityAssets;
use client_world::{ActorRigSnapshot, PlayerProfile};
use protocol::PlayerSkin;
use render::{
    ACTOR_LAYER_BODY, ActorRigGeometry, ActorRigRoute, ActorRigSubmission, EntityRigId,
    MAX_RENDERED_PLAYERS, RenderBoneTransform, STANDARD_SKIN_BYTES, STANDARD_SKIN_SIDE,
    entity_geometry, equipment_rig_id, find_geometry_index, geometry_bone_names,
};

use super::actors::ActorPresentationBatch;

/// Render layer of a player's cape, below the extra texture layers.
pub(crate) const ACTOR_LAYER_CAPE: u8 = 24;

const CAPE_GEOMETRY: &str = "geometry.cape";
/// Half turn about the vertical axis: the cape geometry's authored rest rotation.
const REST_TURN: [f32; 4] = [0.0, 1.0, 0.0, 0.0];

/// The cape geometry and its bone order, resolved once from the entity catalog.
#[derive(Clone)]
pub(crate) struct CapeRig {
    pub(crate) id: EntityRigId,
    pub(crate) geometry: ActorRigGeometry,
    bone_names: Vec<Box<str>>,
}

impl CapeRig {
    pub(crate) fn resolve(assets: &RuntimeEntityAssets) -> Option<Self> {
        let index = find_geometry_index(assets, CAPE_GEOMETRY)?;
        let id = equipment_rig_id(index);
        Some(Self {
            id,
            geometry: entity_geometry(assets, index as usize, id).ok()?,
            bone_names: geometry_bone_names(assets, index as usize)?,
        })
    }
}

/// The cape rig, resolved on first use; an absent geometry leaves capes undrawn.
#[derive(Default)]
pub(crate) struct CapeState {
    resolved: bool,
    rig: Option<CapeRig>,
}

impl CapeState {
    pub(crate) fn rig(&mut self, assets: Option<&RuntimeEntityAssets>) -> Option<&CapeRig> {
        if !self.resolved
            && let Some(assets) = assets
        {
            self.resolved = true;
            self.rig = CapeRig::resolve(assets);
        }
        self.rig.as_ref()
    }
}

/// Resamples a cape raster into one standard skin layer; the cape geometry's texture
/// coordinates are normalised, so any cape size maps onto the layer exactly.
pub(crate) fn cape_layer(width: u32, height: u32, rgba8: &[u8]) -> Option<Arc<[u8]>> {
    let (width, height) = (width as usize, height as usize);
    let expected = width.checked_mul(height)?.checked_mul(4)?;
    if width == 0 || height == 0 || rgba8.len() != expected {
        return None;
    }
    let side = STANDARD_SKIN_SIDE;
    let mut layer = Vec::with_capacity(STANDARD_SKIN_BYTES);
    for y in 0..side {
        let source_y = y * height / side;
        for x in 0..side {
            let source_x = x * width / side;
            let offset = (source_y * width + source_x) * 4;
            layer.extend_from_slice(&rgba8[offset..offset + 4]);
        }
    }
    Some(layer.into())
}

fn multiply(left: [f32; 4], right: [f32; 4]) -> [f32; 4] {
    let ([lx, ly, lz, lw], [rx, ry, rz, rw]) = (left, right);
    [
        lw * rx + lx * rw + ly * rz - lz * ry,
        lw * ry - lx * rz + ly * rw + lz * rx,
        lw * rz + lx * ry - ly * rx + lz * rw,
        lw * rw - lx * rx - ly * ry - lz * rz,
    ]
}

/// The cape geometry's bones posed from the body's bones of the same name.
fn cape_pose(
    cape: &CapeRig,
    body_names: &[Box<str>],
    body: &[RenderBoneTransform],
) -> Arc<[RenderBoneTransform]> {
    cape.bone_names
        .iter()
        .map(|name| {
            let pose = body_names
                .iter()
                .position(|candidate| candidate.eq_ignore_ascii_case(name))
                .and_then(|index| body.get(index).copied());
            match pose {
                Some(mut pose) if name.eq_ignore_ascii_case("cape") => {
                    pose.rotation = multiply(REST_TURN, pose.rotation);
                    pose
                }
                Some(pose) => pose,
                None => RenderBoneTransform {
                    rotation: [0.0, 0.0, 0.0, 1.0],
                    translation_scale: [0.0; 4],
                    axis_scale: render::UNIT_AXIS_SCALE,
                },
            }
        })
        .collect()
}

fn cape_of(profile: &PlayerProfile) -> Option<Arc<[u8]>> {
    match &profile.skin {
        PlayerSkin::Standard(skin) => {
            let cape = skin.cape.as_ref()?;
            cape_layer(cape.width, cape.height, &cape.rgba8)
        }
        PlayerSkin::Unavailable(_) => None,
    }
}

/// Appends a cape instance for every drawn player body whose skin carries one; capes past the
/// skin layer budget are dropped rather than invalidating the frame.
pub(crate) fn apply_capes<'a>(
    batch: &mut ActorPresentationBatch,
    cape: &CapeRig,
    rig_of: impl Fn(u64) -> Option<ActorRigSnapshot<'a>>,
    profile_of: impl Fn(u64) -> Option<&'a PlayerProfile>,
) {
    let mut capes: Vec<(Arc<[u8]>, usize)> = Vec::new();
    let mut extras = Vec::new();
    for body in &batch.submissions {
        let identity = body.input.identity;
        if identity.layer != ACTOR_LAYER_BODY
            || body.route == ActorRigRoute::NoDraw
            || batch.artwork.contains_key(&identity)
        {
            continue;
        }
        let Some(cape_pixels) = profile_of(identity.runtime_id).and_then(cape_of) else {
            continue;
        };
        let Some(rig) = rig_of(identity.runtime_id) else {
            continue;
        };
        let layer = match capes.iter().find(|(known, _)| *known == cape_pixels) {
            Some((_, layer)) => *layer,
            None if batch.skin_layers.len() < MAX_RENDERED_PLAYERS => {
                batch.skin_layers.push(Arc::clone(&cape_pixels));
                capes.push((cape_pixels, batch.skin_layers.len() - 1));
                batch.skin_layers.len() - 1
            }
            None => continue,
        };
        let mut submission: ActorRigSubmission = body.clone();
        submission.input.identity.layer = ACTOR_LAYER_CAPE;
        submission.input.rig = cape.id;
        submission.input.previous_bones =
            cape_pose(cape, rig.bone_names, &body.input.previous_bones);
        submission.input.current_bones = cape_pose(cape, rig.bone_names, &body.input.current_bones);
        submission.texture_layer = layer as u32;
        submission.tint = 0;
        submission.overlay_rgba8 = 0;
        extras.push(submission);
    }
    batch.submissions.extend(extras);
}

#[cfg(test)]
mod tests {
    use super::{cape_layer, multiply};

    #[test]
    fn cape_rasters_resample_onto_one_skin_layer() {
        let mut cape = vec![0u8; 64 * 32 * 4];
        cape[..4].copy_from_slice(&[9, 8, 7, 255]);
        let layer = cape_layer(64, 32, &cape).unwrap();
        assert_eq!(layer.len(), render::STANDARD_SKIN_BYTES);
        assert_eq!(&layer[..4], &[9, 8, 7, 255]);
        let row = render::STANDARD_SKIN_SIDE * 4;
        let rows_per_source = render::STANDARD_SKIN_SIDE / 32;
        assert_eq!(&layer[row..row + 4], &[9, 8, 7, 255]);
        let next = rows_per_source * row;
        assert_eq!(&layer[next..next + 4], &[0, 0, 0, 0]);
        assert!(cape_layer(64, 32, &cape[1..]).is_none());
    }

    #[test]
    fn quaternion_product_composes_the_half_turn_with_a_tilt() {
        let turn = [0.0, 1.0, 0.0, 0.0];
        assert_eq!(multiply(turn, [0.0, 0.0, 0.0, 1.0]), turn);
        let twice = multiply(turn, turn);
        assert_eq!(twice, [0.0, 0.0, 0.0, -1.0]);
    }
    #[test]
    fn review_render_cape_rejects_overflowing_dimensions() {
        assert!(cape_layer(1 << 31, 1 << 31, &[]).is_none());
        assert!(cape_layer(u32::MAX, u32::MAX, &[]).is_none());
    }
}
