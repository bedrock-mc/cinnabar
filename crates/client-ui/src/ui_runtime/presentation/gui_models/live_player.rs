//! The HUD uses the same geometry and evaluated bones as the world player.

use super::super::UiPresentationRuntime;
use bevy::math::{Affine3A, Quat, Vec3, Vec4};
use render_model::{ActorRigGeometry, ActorVertex, EntityRigId, RenderBoneTransform};

type GeometryIdentity = (u32, Option<[u8; 32]>, Option<[u8; 32]>);

#[derive(Default)]
pub(super) struct LivePlayer {
    source: Option<GeometryIdentity>,
    // Retain the allocation so its address cannot be reused as a cache identity.
    skin: Option<std::sync::Arc<assets::SkinGeometry>>,
    geometry: Option<ActorRigGeometry>,
    pub(super) vertices: Vec<ActorVertex>,
    pub(super) parts: [Option<Affine3A>; 6],
    pub(super) fire_size: Option<[f32; 2]>,
    pub(super) outer_y: f32,
    pub(super) overlay_color: [f32; 4],
    pub(super) fire: super::fire::FirePlayback,
}

impl UiPresentationRuntime {
    /// Projects the full-body evaluation, never the separate first-person hand skeleton.
    pub fn capture_hud_player(
        &mut self,
        stream: Option<&chunk_pipeline::WorldStream>,
        swimming: bool,
    ) {
        self.capture_hud_player_with_emote(stream, swimming, None);
    }

    /// Projects an optional local render-only emote without changing the world rig or hand.
    pub fn capture_hud_player_with_emote(
        &mut self,
        stream: Option<&chunk_pipeline::WorldStream>,
        swimming: bool,
        emote: Option<(client_world::CustomEmote, f64)>,
    ) {
        let fire_frames = self.gui_models.fire.frames.len();
        let live = &mut self.gui_models.live_player;
        live.vertices.clear();
        live.parts = [None; 6];
        live.fire_size = None;
        live.outer_y = if swimming {
            super::super::player_preview::HUD_SWIM_OFFSET
        } else {
            0.0
        };
        live.overlay_color = [0.0; 4];
        let Some(stream) = stream else {
            live.source = None;
            live.fire = Default::default();
            return;
        };
        let id = stream.local_player_runtime_id();
        live.fire_size = stream.authority().actor(id).and_then(|actor| {
            live.fire.observe(
                (
                    stream.authority().actor_session_id(),
                    id,
                    actor.spawn_revision,
                ),
                actor.is_on_fire(),
                fire_frames,
            );
            live.overlay_color = super::fire::native_player_overlay(actor);
            if !actor.is_on_fire() {
                return None;
            }
            let (min, max) = actor.bounding_box()?;
            Some([max[0] - min[0], max[1] - min[1]])
        });
        let Some(rig) = stream.authority().actor_rig(id) else {
            return;
        };
        let emote_pose = emote.and_then(|(emote, phase)| {
            client_world::sample_custom_emote(&rig, emote, phase, phase)
        });
        let rig = emote_pose.as_ref().map_or(rig, |pose| pose.snapshot(rig));
        let Some(pose) = emote_pose
            .as_ref()
            .map(|pose| pose.current.as_ref())
            .or_else(|| stream.authority().actor_ui_pose(id))
        else {
            return;
        };
        let basis = Affine3A::from_scale(Vec3::new(-1., 1., -1.));
        let swim_offset = live.outer_y / super::super::player_preview::PLAYER_MODEL_SCALE;
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
        let catalog = rig.geometry_source();
        let source = (
            rig.rig.0,
            rig.skin_geometry.map(|skin| skin.digest),
            catalog.map(|(assets, _)| assets.source_manifest_sha256()),
        );
        if live.source != Some(source) {
            live.geometry = if let Some(skin) = rig.skin_geometry {
                render_model::skin_geometry(skin, EntityRigId(rig.rig.0)).ok()
            } else {
                catalog.and_then(|(assets, geometry)| {
                    render_model::entity_geometry(assets, geometry, EntityRigId(rig.rig.0)).ok()
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

#[cfg(test)]
mod tests;
