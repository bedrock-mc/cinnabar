//! Real held-item meshes in native hand-bone frames, projected with their owner.

use bevy::math::{Quat, Vec3, Vec4};
use render_model::{ActorVertex, RenderBoneTransform};
use ui::{UI_STYLE_GLINT, UiBlendMode, UiMeshBatch, UiMeshVertex};

use render_model::equipment::{
    attach_to_bone, held_block_display_for_hand, held_sprite_display_for_hand,
};

use super::super::PreviewHeldPlacement;
use super::{PREVIEW_HEIGHT, PREVIEW_WIDTH, PreviewHeldModel, Rig, atlas_edge, lighting};

pub(super) fn append(
    vertices: &mut Vec<UiMeshVertex>,
    batches: &mut Vec<UiMeshBatch>,
    rig: &Rig,
    model: &PreviewHeldModel,
    hand: usize,
    fancy: bool,
) -> Option<()> {
    if model.vertices.is_empty() || !model.vertices.len().is_multiple_of(3) {
        return None;
    }
    let (bone, pivot) = placement(model, hand)?;
    let rotation = Quat::from_vec4(Vec4::from_array(bone.rotation).try_normalize()?);
    let scale =
        Vec3::from_array([0, 1, 2].map(|axis| bone.axis_scale[axis] * bone.translation_scale[3]));
    let origin = Vec3::from_array(bone.translation_scale[..3].try_into().ok()?);
    let part = if hand == 0 { 2 } else { 3 };
    let start = u32::try_from(vertices.len()).ok()?;
    let [left, top, right, bottom] = model.source.uv;
    if left >= right || top >= bottom {
        return None;
    }
    for source in &*model.vertices {
        if source.bone_index != 0 {
            // Multi-bone attachables require their evaluated native pose, not
            // an invented single-bone flattening of the authored geometry.
            return None;
        }
        let native = origin + rotation * ((Vec3::from_array(source.position) - pivot) * scale);
        let native_normal = rotation * (Vec3::from_array(source.normal) * scale);
        // The standard preview's viewer-facing biped is the native actor rig
        // under its fixed half-turn. Apply that basis once to points/normals.
        let local = Vec3::new(-native.x, native.y, -native.z);
        let normal = Vec3::new(-native_normal.x, native_normal.y, -native_normal.z);
        let actor = ActorVertex {
            position: local.to_array(),
            uv: source.uv,
            part,
        };
        let projected = rig.project(actor);
        let tip = rig.project(ActorVertex {
            position: (local + normal).to_array(),
            ..actor
        });
        let normal =
            (Vec3::from_array(tip.world) - Vec3::from_array(projected.world)).try_normalize()?;
        let model_light = if fancy {
            lighting::fancy_intensity(normal.to_array())
        } else {
            1.0
        };
        if projected
            .world
            .iter()
            .chain(source.uv.iter())
            .any(|value| !value.is_finite())
        {
            return None;
        }
        vertices.push(UiMeshVertex {
            position: [
                projected.screen[0] / PREVIEW_WIDTH as f32,
                projected.screen[1] / PREVIEW_HEIGHT as f32,
            ],
            clip_z: projected.world[2],
            clip_w: 1.0,
            uv: [
                atlas_edge(left, right, source.uv[0])?,
                atlas_edge(top, bottom, source.uv[1])?,
            ],
            color: [255; 4],
            model_light,
            overlay_color: [0.0; 4],
            style_flags: if model.source.glint {
                UI_STYLE_GLINT
            } else {
                0
            },
            alpha_test: false,
        });
    }
    batches.push(UiMeshBatch {
        texture_page: model.source.page,
        index_range: start..u32::try_from(vertices.len()).ok()?,
        blend: UiBlendMode::Alpha,
        depth_test: true,
        depth_write: true,
        alpha_cutoff: Some(0.5),
    });
    Some(())
}

fn placement(model: &PreviewHeldModel, hand: usize) -> Option<(RenderBoneTransform, Vec3)> {
    let origin = *model.hand_pivots.get(hand)?;
    let parent = RenderBoneTransform {
        rotation: Quat::IDENTITY.to_array(),
        translation_scale: [origin[0], origin[1], origin[2], 1.0],
        axis_scale: render_model::UNIT_AXIS_SCALE,
    };
    let (bone, pivot) = match *model.placements.get(hand)? {
        PreviewHeldPlacement::Sprite { hand_equipped } => (
            attach_to_bone(
                parent,
                held_sprite_display_for_hand(hand_equipped, hand == 1),
            )?,
            Vec3::ZERO,
        ),
        PreviewHeldPlacement::Block => (
            attach_to_bone(parent, held_block_display_for_hand(hand == 1))?,
            Vec3::ZERO,
        ),
        PreviewHeldPlacement::Authored { mut bone, pivot } => {
            for (axis, origin) in origin.into_iter().enumerate() {
                bone.translation_scale[axis] += origin;
            }
            (bone, Vec3::from_array(pivot))
        }
    };
    bone.is_finite().then_some((bone, pivot))
}

#[cfg(test)]
mod tests;

#[cfg(any(test, feature = "test-support"))]
pub mod test_support;
