//! Canonical player cape geometry, raster mapping and body-pose binding.
use crate::{
    ActorRigGeometry, EntityRigId, RenderBoneTransform, STANDARD_SKIN_BYTES, STANDARD_SKIN_SIDE,
    entity_geometry, equipment_rig_id, find_geometry_index, geometry_bone_names,
};
use assets::RuntimeEntityAssets;
use std::sync::Arc;
/// Render layer of a player's cape, below the extra texture layers.
pub const ACTOR_LAYER_CAPE: u8 = 24;

const CAPE_GEOMETRY: &str = "geometry.cape";
/// Half turn about the vertical axis: the cape geometry's authored rest rotation.
const REST_TURN: [f32; 4] = [0.0, 1.0, 0.0, 0.0];

/// The cape geometry and its bone order, resolved once from the entity catalog.
#[derive(Clone)]
pub struct CapeRig {
    pub id: EntityRigId,
    pub geometry: ActorRigGeometry,
    bone_names: Vec<Box<str>>,
}

impl CapeRig {
    pub fn resolve(assets: &RuntimeEntityAssets) -> Option<Self> {
        let index = find_geometry_index(assets, CAPE_GEOMETRY)?;
        let id = equipment_rig_id(index);
        Some(Self {
            id,
            geometry: entity_geometry(assets, index as usize, id).ok()?,
            bone_names: geometry_bone_names(assets, index as usize)?,
        })
    }
}

/// Resamples a cape raster into one standard skin layer; the cape geometry's texture
/// coordinates are normalised, so any cape size maps onto the layer exactly.
pub fn cape_layer(width: u32, height: u32, rgba8: &[u8]) -> Option<Arc<[u8]>> {
    let (width, height) = (width as usize, height as usize);
    if width == 0 || height == 0 || rgba8.len() != width * height * 4 {
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

pub fn multiply(left: [f32; 4], right: [f32; 4]) -> [f32; 4] {
    let ([lx, ly, lz, lw], [rx, ry, rz, rw]) = (left, right);
    [
        lw * rx + lx * rw + ly * rz - lz * ry,
        lw * ry - lx * rz + ly * rw + lz * rx,
        lw * rz + lx * ry - ly * rx + lz * rw,
        lw * rw - lx * rx - ly * ry - lz * rz,
    ]
}

/// The cape geometry's bones posed from the body's bones of the same name.
pub fn cape_pose(
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
                    axis_scale: crate::UNIT_AXIS_SCALE,
                },
            }
        })
        .collect()
}
