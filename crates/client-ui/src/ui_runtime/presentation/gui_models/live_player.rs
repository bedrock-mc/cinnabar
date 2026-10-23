//! The HUD and live player renderers use the same geometry and evaluated bones as the world player.

use super::super::{UiPresentationRuntime, player_preview::PreviewView};
use glam::{Affine3A, Quat, Vec3, Vec4};
use render_model::{ActorRigGeometry, ActorVertex, EntityRigId, RenderBoneTransform};

type GeometryIdentity = (u32, Option<[u8; 32]>, Option<[u8; 32]>);

#[derive(Default)]
pub(super) struct LivePlayer {
    source: Option<GeometryIdentity>,
    // Retain the allocation so its address cannot be reused as a cache identity.
    skin: Option<std::sync::Arc<assets::SkinGeometry>>,
    geometry: Option<ActorRigGeometry>,
    pub(super) vertices: Vec<ActorVertex>,
    pub(super) cape_vertices: Vec<ActorVertex>,
    pub(super) hat_vertices: std::collections::HashMap<cosmetics::Item, Vec<ActorVertex>>,
    hat_pose: std::collections::HashMap<cosmetics::Item, render_model::hats::HatPoseCache>,
    pub(super) hat_motion: std::collections::HashMap<cosmetics::Item, render_model::hats::Motion>,
    bones: Vec<RenderBoneTransform>,
    pub(super) emitter_owner: Option<(u64, u64, u64)>,
    pub(super) emitters: Vec<(cosmetics::Item, usize, render_model::ActorEmitterTransform)>,
    pub(super) parts: [Option<Affine3A>; 6],
    pub(super) fire_size: Option<[f32; 2]>,
    pub(super) outer_y: f32,
    pub(super) overlay_color: [f32; 4],
    pub(super) fire: super::fire::FirePlayback,
}

/// The capture `view` poses from: the HUD doll's, or a live renderer's once captured.
/// Other views, and a live renderer without a capture, use the static menu model.
pub(super) fn capture_for<'a>(
    view: PreviewView,
    hud: &'a LivePlayer,
    screen: &'a LivePlayer,
) -> Option<&'a LivePlayer> {
    match view {
        PreviewView::Hud => Some(hud),
        PreviewView::Live { .. } => Some(screen).filter(|live| !live.vertices.is_empty()),
        _ => None,
    }
}

impl UiPresentationRuntime {
    /// Shares the actor frame's evaluated motion without advancing a second animation clock.
    pub fn set_cosmetic_motion(
        &mut self,
        motion: &[(cosmetics::Item, render_model::hats::Motion)],
    ) {
        let motion: std::collections::HashMap<_, _> = motion.iter().copied().collect();
        if self.gui_models.live_player.hat_motion != motion {
            self.gui_models.screen_player.hat_motion = motion.clone();
            self.gui_models.live_player.hat_motion = motion;
            self.last_frame = None;
        }
    }

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
        emote: Option<(cosmetics::emotes::Item, f64)>,
    ) {
        let mut live = std::mem::take(&mut self.gui_models.live_player);
        self.capture_player(&mut live, stream, swimming, emote, None);
        self.gui_models.live_player = live;
    }

    /// Projects the same evaluation for a screen's `live_player_renderer`, separately from the
    /// HUD doll. Its head looks toward the pointer offset the renderer last painted with.
    pub fn capture_screen_player(&mut self, stream: Option<&chunk_pipeline::WorldStream>) {
        let offset = match self.player_preview_view {
            PreviewView::Live { offset } => offset,
            _ => [0.0; 2],
        };
        let look = PreviewView::Live { offset }.head_look();
        let mut live = std::mem::take(&mut self.gui_models.screen_player);
        // Only the HUD lifts a swimming actor's frame.
        self.capture_player(&mut live, stream, false, None, Some(look));
        self.gui_models.screen_player = live;
    }

    /// Projects the UI evaluation into `live`, optionally replacing the head's look.
    fn capture_player(
        &self,
        live: &mut LivePlayer,
        stream: Option<&chunk_pipeline::WorldStream>,
        swimming: bool,
        emote: Option<(cosmetics::emotes::Item, f64)>,
        look: Option<[f32; 2]>,
    ) {
        let fire_frames = self.gui_models.fire.frames.len();
        let draw_cape =
            self.menu_preview_model.cape_key.is_some() && !self.gui_models.wearing_elytra;
        live.vertices.clear();
        live.cape_vertices.clear();
        live.hat_vertices.clear();
        live.emitters.clear();
        live.emitter_owner = None;
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
        live.emitter_owner = stream.authority().actor(id).map(|actor| {
            (
                stream.authority().actor_session_id(),
                id,
                actor.spawn_revision,
            )
        });
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
            actor_animation::sample_custom_emote(&rig, emote, phase, phase)
        });
        let rig = emote_pose.as_ref().map_or(rig, |pose| pose.snapshot(rig));
        let Some(mut pose) = emote_pose
            .as_ref()
            .map(|pose| pose.current.as_ref())
            .or_else(|| stream.authority().actor_ui_pose(id))
        else {
            return;
        };
        let looked;
        if let Some(look) = look
            && let Some(hierarchy) = stream.authority().actor_bone_hierarchy(id)
        {
            let mut turned = pose.to_vec();
            look_toward(&mut turned, &rig, hierarchy, look);
            looked = turned;
            pose = &looked;
        }
        let basis = Affine3A::from_scale(Vec3::new(-1., 1., -1.));
        let swim_offset = live.outer_y / super::super::player_preview::PLAYER_MODEL_SCALE;
        let offset = Affine3A::from_translation(Vec3::Y * swim_offset);
        let actor_scale = Affine3A::from_scale(
            Vec3::from_array(rig.axis_scale)
                * (rig.scale / super::super::player_preview::PLAYER_MODEL_SCALE),
        );
        for (part, name) in render_model::STANDARD_BIPED_BONES.into_iter().enumerate() {
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
        live.bones.clear();
        for bone in pose {
            let Some(bone) = render_bone(bone) else {
                return;
            };
            live.bones.push(bone);
        }
        let frame = offset * actor_scale * basis;
        project_vertices(geometry, &live.bones, frame, &mut live.vertices);
        live.hat_pose
            .retain(|item, _| self.menu_preview_model.cosmetics.contains(item));
        for &hat in &self.menu_preview_model.cosmetics {
            if render_model::hats::rig(hat).is_ground_effect() {
                continue;
            }
            let cache = live.hat_pose.entry(hat).or_default();
            let Some(index) = cache.anchor(hat, rig.attachments, rig.bone_names) else {
                continue;
            };
            let Some(head) = live.bones.get(index) else {
                continue;
            };
            let pose = cache.pose(
                hat,
                *head,
                live.hat_motion.get(&hat).copied().unwrap_or_default(),
            );
            let world = frame.matrix3.to_cols_array_2d();
            let translation = frame.translation.to_array();
            let world = std::array::from_fn(|row| {
                [
                    world[0][row],
                    world[1][row],
                    world[2][row],
                    translation[row],
                ]
            });
            for index in 0..render_model::hats::rig(hat).emitters().len() {
                if let Some(transform) =
                    render_model::hats::rig(hat).emitter_transform(index, &pose, &pose, world, 1.0)
                {
                    live.emitters.push((hat, index, transform));
                }
            }
            let vertices = live.hat_vertices.entry(hat).or_default();
            project_vertices(
                &render_model::hats::rig(hat).geometry,
                &pose,
                frame,
                vertices,
            );
            discard_collapsed_faces(vertices);
        }
        if draw_cape && let Some(cape) = &self.gui_models.cape {
            let bindings = rig
                .attachments
                .and_then(|bindings| bindings.cape_for(cape))
                .map_or_else(
                    || std::borrow::Cow::Owned(cape.bind(rig.bone_names)),
                    std::borrow::Cow::Borrowed,
                );
            let pose = cape.pose_bound(
                &bindings,
                |index| rig.rest.get(index).and_then(render_bone),
                &live.bones,
                None,
            );
            project_vertices(&cape.geometry, &pose, frame, &mut live.cape_vertices);
            discard_collapsed_faces(&mut live.cape_vertices);
        }
    }
}

/// Turns the head toward `[pitch, yaw]` degrees relative to the body, the angles the
/// player's look animation reads, carrying every descendant bone with it.
///
/// The live player renderer re-renders the actor with its rotations replaced, so the look
/// animation's entity-relative head rotation follows the pointer. The replacement is exact for
/// that animation alone; other head rotations layered on it are not separated.
fn look_toward(
    pose: &mut [actor_animation::BoneTransform],
    rig: &actor_animation::ActorRigSnapshot<'_>,
    hierarchy: actor_animation::BoneHierarchy<'_>,
    [pitch, yaw]: [f32; 2],
) {
    let Some(head) = rig
        .bone_names
        .iter()
        .position(|name| name.as_ref() == "head")
    else {
        return;
    };
    let Some(&current) = pose.get(head) else {
        return;
    };
    let rest = rig
        .rest
        .get(head)
        .and_then(|bone| unit_rotation(bone.rotation))
        .unwrap_or(Quat::IDENTITY);
    // Animation angles turn against the right-hand rule in the mirrored model frame.
    let target = Quat::from_rotation_y(-yaw.to_radians())
        * Quat::from_rotation_x(-pitch.to_radians())
        * rest;
    let Some(current_rotation) = unit_rotation(current.rotation) else {
        return;
    };
    let correction = target * current_rotation.inverse();
    let pivot = Vec3::from_slice(&current.translation_scale[..3]);
    for (index, bone) in pose.iter_mut().enumerate() {
        if !hierarchy.descends_from(index, head) {
            continue;
        }
        let translation =
            pivot + correction * (Vec3::from_slice(&bone.translation_scale[..3]) - pivot);
        bone.translation_scale[..3].copy_from_slice(&translation.to_array());
        bone.rotation = (correction * Quat::from_array(bone.rotation)).to_array();
    }
}

/// A bone's `[x, y, z, w]` rotation as a unit quaternion, when it has a direction.
fn unit_rotation(rotation: [f32; 4]) -> Option<Quat> {
    Vec4::from_array(rotation)
        .try_normalize()
        .map(Quat::from_vec4)
}

/// Converts a validated bone frame to block coordinates for equipment pose deltas.
fn bone_matrix(bone: &actor_animation::BoneTransform) -> Option<Affine3A> {
    let bone = render_bone(bone)?;
    let scale =
        Vec3::from_array([0, 1, 2].map(|axis| bone.axis_scale[axis] * bone.translation_scale[3]));
    if scale.abs().min_element() < f32::EPSILON {
        return None;
    }
    render_matrix(&bone)
}

/// Converts an evaluated model-space bone to the block coordinates shared by both renderers.
fn render_bone(bone: &actor_animation::BoneTransform) -> Option<RenderBoneTransform> {
    RenderBoneTransform::from_model_space_scaled(
        bone.rotation,
        bone.translation_scale,
        bone.axis_scale,
    )
}

/// Builds a render-space frame, retaining zero scales that hide an authored bone.
fn render_matrix(bone: &RenderBoneTransform) -> Option<Affine3A> {
    let rotation = Vec4::from_array(bone.rotation)
        .try_normalize()
        .map(Quat::from_vec4)?;
    let scale =
        Vec3::from_array([0, 1, 2].map(|axis| bone.axis_scale[axis] * bone.translation_scale[3]));
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

/// Poses one mesh in the shared HUD frame; preposed vertices bypass the preview's part deltas.
fn project_vertices(
    geometry: &ActorRigGeometry,
    pose: &[RenderBoneTransform],
    frame: Affine3A,
    output: &mut Vec<ActorVertex>,
) {
    for vertex in &*geometry.vertices {
        let index = vertex.bone_index as usize;
        let Some(bone) = pose.get(index).and_then(render_matrix) else {
            output.clear();
            return;
        };
        let pivot = Vec3::from_array(geometry.bone_pivots[index]);
        let point = frame
            .transform_point3(bone.transform_point3(Vec3::from_array(vertex.position) - pivot));
        output.push(ActorVertex {
            position: point.to_array(),
            uv: vertex.uv,
            part: vertex.bone_index + 6,
        });
    }
}

/// Removes cape faces with no lighting normal while preserving any remaining visible faces.
fn discard_collapsed_faces(vertices: &mut Vec<ActorVertex>) {
    let mut visible = 0;
    for start in (0..vertices.len()).step_by(3) {
        let [a, b, c] = [0, 1, 2].map(|corner| Vec3::from_array(vertices[start + corner].position));
        if (b - a).cross(c - a).length_squared() == 0.0 {
            continue;
        }
        vertices.copy_within(start..start + 3, visible);
        visible += 3;
    }
    vertices.truncate(visible);
}

#[cfg(test)]
mod cape_tests;
#[cfg(test)]
mod screen_tests;
#[cfg(test)]
mod tests;
