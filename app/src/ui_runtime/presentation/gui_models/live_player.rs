//! The HUD uses the same geometry and evaluated bones as the world player.

use super::super::UiPresentationRuntime;
use bevy::math::{Affine3A, Quat, Vec3, Vec4};
use render::{ActorRigGeometry, ActorVertex, EntityRigId, RenderBoneTransform};

#[derive(Default)]
pub(super) struct LivePlayer {
    source: Option<(u32, Option<usize>)>,
    // Retain the allocation so its address cannot be reused as a cache identity.
    skin: Option<std::sync::Arc<assets::SkinGeometry>>,
    geometry: Option<ActorRigGeometry>,
    pub(super) vertices: Vec<ActorVertex>,
    pub(super) parts: [Option<Affine3A>; 6],
}

impl UiPresentationRuntime {
    /// Projects the full-body evaluation, never the separate first-person hand skeleton.
    pub(crate) fn capture_hud_player(
        &mut self,
        stream: Option<&client_world::WorldStream>,
        swimming: bool,
    ) {
        let live = &mut self.gui_models.live_player;
        live.vertices.clear();
        live.parts = [None; 6];
        let Some(stream) = stream else {
            live.source = None;
            return;
        };
        let id = stream.local_player_runtime_id();
        let Some(rig) = stream.actor_rig(id) else {
            return;
        };
        let Some(pose) = stream.actor_ui_pose(id) else {
            return;
        };
        let basis = Affine3A::from_scale(Vec3::new(-1., 1., -1.));
        let swim_offset = if swimming {
            0.8 / super::super::player_preview::PLAYER_MODEL_SCALE
        } else {
            0.0
        };
        let offset = Affine3A::from_translation(Vec3::Y * swim_offset);
        let actor_scale = Affine3A::from_scale(
            Vec3::from_array(rig.axis_scale)
                * (rig.scale / super::super::player_preview::PLAYER_MODEL_SCALE),
        );
        for (part, name) in ["head", "body", "rightarm", "leftarm", "rightleg", "leftleg"]
            .into_iter()
            .enumerate()
        {
            if let Some(index) = rig.bone_names.iter().position(|bone| bone.as_ref() == name)
                && let (Some(rest), Some(posed)) = (rig.rest.get(index), pose.get(index))
                && let (Some(rest), Some(posed)) = (bone_matrix(rest), bone_matrix(posed))
            {
                live.parts[part] =
                    Some(offset * actor_scale * basis * posed * rest.inverse() * basis);
            }
        }
        let source = (
            rig.rig.0,
            rig.skin_geometry
                .map(|skin| std::sync::Arc::as_ptr(skin) as usize),
        );
        if live.source != Some(source) {
            live.geometry = if let Some(skin) = rig.skin_geometry {
                render::skin_geometry(skin, EntityRigId(rig.rig.0)).ok()
            } else {
                self.gui_models.entities.as_ref().and_then(|assets| {
                    let binding = assets.rig_geometries().get(rig.rig.0 as usize)?;
                    render::entity_geometry(
                        assets,
                        binding.geometry as usize,
                        EntityRigId(rig.rig.0),
                    )
                    .ok()
                })
            };
            live.source = Some(source);
            live.skin = rig.skin_geometry.cloned();
        }
        let Some(geometry) = &live.geometry else {
            return;
        };
        for vertex in &*geometry.vertices {
            let index = vertex.bone_index as usize;
            let Some(bone) = pose.get(index) else {
                live.vertices.clear();
                return;
            };
            let Some(bone) = RenderBoneTransform::from_model_space_scaled(
                bone.rotation,
                bone.translation_scale,
                bone.axis_scale,
            ) else {
                live.vertices.clear();
                return;
            };
            let Some(rotation) = Vec4::from_array(bone.rotation)
                .try_normalize()
                .map(Quat::from_vec4)
            else {
                live.vertices.clear();
                return;
            };
            let pivot = Vec3::from_array(geometry.bone_pivots[index]);
            let scale = Vec3::from_array(
                [0, 1, 2].map(|axis| bone.axis_scale[axis] * bone.translation_scale[3]),
            );
            let origin = Vec3::new(
                bone.translation_scale[0],
                bone.translation_scale[1],
                bone.translation_scale[2],
            );
            let point = origin + rotation * ((Vec3::from_array(vertex.position) - pivot) * scale);
            // Convert the world rig's fixed half-turn to the existing preview basis once.
            let point = Vec3::new(-point.x, point.y, -point.z)
                * Vec3::from_array(rig.axis_scale)
                * (rig.scale / super::super::player_preview::PLAYER_MODEL_SCALE);
            live.vertices.push(ActorVertex {
                position: (point + Vec3::Y * swim_offset).to_array(),
                uv: vertex.uv,
                part: vertex.bone_index + 6,
            });
        }
    }
}

/// Converts a validated bone frame to block coordinates for equipment pose deltas.
fn bone_matrix(bone: &client_world::BoneTransform) -> Option<Affine3A> {
    let bone = RenderBoneTransform::from_model_space_scaled(
        bone.rotation,
        bone.translation_scale,
        bone.axis_scale,
    )?;
    let rotation = Vec4::from_array(bone.rotation)
        .try_normalize()
        .map(Quat::from_vec4)?;
    let scale =
        Vec3::from_array([0, 1, 2].map(|axis| bone.axis_scale[axis] * bone.translation_scale[3]));
    if scale.abs().min_element() < f32::EPSILON {
        return None;
    }
    Some(Affine3A::from_scale_rotation_translation(
        scale,
        rotation,
        Vec3::new(
            bone.translation_scale[0],
            bone.translation_scale[1],
            bone.translation_scale[2],
        ),
    ))
}
