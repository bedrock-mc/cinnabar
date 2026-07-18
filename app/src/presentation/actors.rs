use std::{collections::BTreeMap, sync::Arc};

use assets::EntityRigFallback;
use client_world::{ActorRigSnapshot, ActorSnapshot, PlayerProfile};
use protocol::{ActorKind, PlayerSkin};
use render::{
    ActorCullView, ActorRenderFrame, ActorRenderIdentity, ActorRenderScene, ActorRigRenderInput,
    ActorRigRoute, ActorRigSubmission, ActorSkinPixels, EntityRigId, MAX_RENDERED_PLAYERS,
    RenderBoneTransform, actor_rig_submission_is_visible, default_actor_skin_rgba8,
    normalize_actor_skin,
};

#[derive(Clone, Debug)]
pub(crate) struct ActorRigPresentation {
    pub(crate) submission: ActorRigSubmission,
    pub(crate) skin_rgba8: Option<Arc<[u8]>>,
}

#[derive(Debug)]
pub(crate) struct ActorPresentationBatch {
    pub(crate) submissions: Vec<ActorRigSubmission>,
    pub(crate) skins_rgba8: Arc<[u8]>,
}

pub(crate) fn update_actor_rig_scene(
    scene: &mut ActorRenderScene,
    partial_tick: f32,
    batch: ActorPresentationBatch,
) -> &ActorRenderFrame {
    // The app adapter has already applied the renderer's exact culling helper
    // to remotes before enforcing capacity. Passing no second cull view keeps
    // Phase 3's visible local reservation unconditional in both third-person
    // modes while the render-owned builder still validates every other field.
    scene.update_rigs(partial_tick, None, batch.submissions, batch.skins_rgba8)
}

pub(crate) fn actor_rig_presentation(
    rig: &ActorRigSnapshot<'_>,
    actor: &ActorSnapshot,
    profile: Option<&PlayerProfile>,
    partial_tick: f32,
) -> Option<ActorRigPresentation> {
    if rig.actor.runtime_id != actor.runtime_id
        || rig.actor.spawn_revision != actor.spawn_revision
        || rig.actor.session_id == 0
        || rig.actor.runtime_id == 0
        || rig.actor.spawn_revision == 0
        || rig.completed_tick == 0
        || rig.reset_generation == 0
        || rig.previous.is_empty()
        || rig.previous.len() != rig.current.len()
        || !partial_tick.is_finite()
    {
        return None;
    }

    let previous_bones = convert_bones(rig.previous)?;
    let current_bones = convert_bones(rig.current)?;
    let alpha = partial_tick.clamp(0.0, 1.0);
    let position = interpolated_position(actor, alpha)?;
    let yaw = lerp_degrees(actor.previous_pose.yaw, actor.yaw, alpha);
    if !yaw.is_finite() {
        return None;
    }
    let (sine, cosine) = yaw.to_radians().sin_cos();
    let identity = ActorRenderIdentity {
        session_id: rig.actor.session_id,
        dimension: rig.actor.dimension,
        runtime_id: rig.actor.runtime_id,
        spawn_revision: rig.actor.spawn_revision,
        ingress_sequence: actor.spawn_revision.max(actor.movement_revision),
        source_tick: actor.source_tick,
        movement_revision: actor.movement_revision,
        pose_generation: rig.completed_tick,
    };
    if !identity.is_exact() {
        return None;
    }

    let (route, skin_rgba8) = player_route_and_skin(actor, profile, rig.fallback);
    Some(ActorRigPresentation {
        submission: ActorRigSubmission {
            input: ActorRigRenderInput {
                identity,
                rig: EntityRigId(rig.rig.0),
                previous_bones,
                current_bones,
                completed_tick: rig.completed_tick,
                reset_generation: rig.reset_generation,
            },
            world_from_actor: [
                [cosine, 0.0, sine, position[0]],
                [0.0, 1.0, 0.0, position[1]],
                [-sine, 0.0, cosine, position[2]],
            ],
            texture_layer: u32::MAX,
            route,
        },
        skin_rgba8,
    })
}

pub(crate) fn local_diagnostic_presentation(
    actor_session_id: u64,
    dimension: i32,
    runtime_id: u64,
    pose_generation: u64,
    position: [f32; 3],
    yaw_degrees: f32,
    pitch_degrees: f32,
) -> Option<ActorRigPresentation> {
    if actor_session_id == 0
        || runtime_id == 0
        || pose_generation == 0
        || position.iter().any(|value| !value.is_finite())
        || !yaw_degrees.is_finite()
        || !pitch_degrees.is_finite()
    {
        return None;
    }
    let head_rotation = quaternion_from_euler_degrees([pitch_degrees, 0.0, 0.0]);
    let pivots = [
        [0.0, 1.75, 0.0],
        [0.0, 1.5, 0.0],
        [-0.3125, 1.375, 0.0],
        [-0.11875, 0.75, 0.0],
        [0.3125, 1.375, 0.0],
        [0.11875, 0.75, 0.0],
    ];
    let mut bones = pivots.map(|pivot| RenderBoneTransform {
        rotation: [0.0, 0.0, 0.0, 1.0],
        translation_scale: [pivot[0], pivot[1], pivot[2], 1.0],
    });
    bones[0].rotation = head_rotation;
    let (sine, cosine) = yaw_degrees.to_radians().sin_cos();
    Some(ActorRigPresentation {
        submission: ActorRigSubmission {
            input: ActorRigRenderInput {
                identity: ActorRenderIdentity {
                    session_id: actor_session_id,
                    dimension,
                    runtime_id,
                    spawn_revision: actor_session_id,
                    ingress_sequence: pose_generation,
                    source_tick: None,
                    movement_revision: pose_generation,
                    pose_generation,
                },
                rig: EntityRigId(u32::MAX),
                previous_bones: Arc::from(bones),
                current_bones: Arc::from(bones),
                completed_tick: pose_generation,
                reset_generation: actor_session_id,
            },
            world_from_actor: [
                [cosine, 0.0, sine, position[0]],
                [0.0, 1.0, 0.0, position[1]],
                [-sine, 0.0, cosine, position[2]],
            ],
            texture_layer: u32::MAX,
            route: ActorRigRoute::Diagnostic,
        },
        skin_rgba8: Some(default_actor_skin_rgba8()),
    })
}

#[cfg(test)]
pub(crate) fn select_actor_presentations(
    local_runtime_id: u64,
    local_visible: bool,
    local: Option<ActorRigPresentation>,
    remotes: impl IntoIterator<Item = ActorRigPresentation>,
) -> ActorPresentationBatch {
    select_actor_presentations_for_view(local_runtime_id, local_visible, local, remotes, None)
}

pub(crate) fn select_actor_presentations_for_view(
    local_runtime_id: u64,
    local_visible: bool,
    local: Option<ActorRigPresentation>,
    remotes: impl IntoIterator<Item = ActorRigPresentation>,
    view: Option<ActorCullView>,
) -> ActorPresentationBatch {
    let mut latest = BTreeMap::<u64, ActorRigPresentation>::new();
    for remote in remotes {
        let identity = remote.submission.input.identity;
        if identity.runtime_id == 0 || identity.runtime_id == local_runtime_id {
            continue;
        }
        match latest.entry(identity.runtime_id) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(remote);
            }
            std::collections::btree_map::Entry::Occupied(mut entry) => {
                if identity > entry.get().submission.input.identity {
                    entry.insert(remote);
                }
            }
        }
    }

    let local = local_visible
        .then_some(local)
        .flatten()
        .filter(|local| local.submission.input.identity.runtime_id == local_runtime_id);
    let mut selected = Vec::with_capacity(MAX_RENDERED_PLAYERS);
    let mut drawable_count = 0usize;
    if let Some(local) = local {
        drawable_count = 1;
        selected.push(local);
    }
    for remote in latest.into_values() {
        if remote.submission.route == ActorRigRoute::NoDraw {
            selected.push(remote);
            continue;
        }
        if drawable_count == MAX_RENDERED_PLAYERS
            || !actor_rig_submission_is_visible(&remote.submission, view)
        {
            continue;
        }
        drawable_count += 1;
        selected.push(remote);
    }

    let mut skin_families = Vec::<Arc<[u8]>>::new();
    let mut submissions = Vec::with_capacity(selected.len());
    for mut presentation in selected {
        let Some(skin) = presentation.skin_rgba8 else {
            presentation.submission.route = ActorRigRoute::NoDraw;
            presentation.submission.texture_layer = u32::MAX;
            submissions.push(presentation.submission);
            continue;
        };
        let layer = skin_families
            .iter()
            .position(|existing| existing.as_ref() == skin.as_ref())
            .unwrap_or_else(|| {
                skin_families.push(skin);
                skin_families.len() - 1
            });
        presentation.submission.texture_layer =
            u32::try_from(layer).expect("actor skin family count is bounded");
        submissions.push(presentation.submission);
    }
    let mut skin_bytes = Vec::new();
    for skin in skin_families {
        skin_bytes.extend_from_slice(&skin);
    }
    ActorPresentationBatch {
        submissions,
        skins_rgba8: skin_bytes.into(),
    }
}

fn convert_bones(bones: &[client_world::BoneTransform]) -> Option<Arc<[RenderBoneTransform]>> {
    bones
        .iter()
        .map(|bone| RenderBoneTransform::from_model_space(bone.rotation, bone.translation_scale))
        .collect::<Option<Vec<_>>>()
        .map(Arc::from)
}

fn interpolated_position(actor: &ActorSnapshot, partial_tick: f32) -> Option<[f32; 3]> {
    let position = std::array::from_fn(|axis| {
        actor.previous_pose.position[axis]
            + (actor.position[axis] - actor.previous_pose.position[axis]) * partial_tick
    });
    position
        .iter()
        .all(|value| value.is_finite())
        .then_some(position)
}

fn lerp_degrees(start: f32, end: f32, alpha: f32) -> f32 {
    wrap_degrees(start + wrap_degrees(end - start) * alpha)
}

fn wrap_degrees(degrees: f32) -> f32 {
    (degrees + 180.0).rem_euclid(360.0) - 180.0
}

fn quaternion_from_euler_degrees(rotation: [f32; 3]) -> [f32; 4] {
    let [x, y, z] = rotation.map(|value| value.to_radians() * 0.5);
    let (sx, cx) = x.sin_cos();
    let (sy, cy) = y.sin_cos();
    let (sz, cz) = z.sin_cos();
    [
        sx * cy * cz - cx * sy * sz,
        cx * sy * cz + sx * cy * sz,
        cx * cy * sz - sx * sy * cz,
        cx * cy * cz + sx * sy * sz,
    ]
}

fn player_route_and_skin(
    actor: &ActorSnapshot,
    profile: Option<&PlayerProfile>,
    fallback: EntityRigFallback,
) -> (ActorRigRoute, Option<Arc<[u8]>>) {
    let ActorKind::Player { .. } = &actor.kind else {
        return (ActorRigRoute::NoDraw, None);
    };
    let route = match fallback {
        EntityRigFallback::Skip => ActorRigRoute::Compiled,
        EntityRigFallback::GeometryOnly => ActorRigRoute::StaticFallback,
        EntityRigFallback::Diagnostic => ActorRigRoute::Diagnostic,
    };
    let skin = profile
        .filter(|profile| profile.unique_id == actor.unique_id)
        .and_then(|profile| match &profile.skin {
            PlayerSkin::Standard(skin) => normalize_actor_skin(&ActorSkinPixels {
                width: skin.width,
                height: skin.height,
                rgba8: Arc::clone(&skin.rgba8),
            }),
            PlayerSkin::Unavailable(_) => None,
        })
        .unwrap_or_else(default_actor_skin_rgba8);
    (route, Some(skin))
}
