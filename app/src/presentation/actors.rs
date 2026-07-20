use std::{collections::BTreeMap, sync::Arc};

use assets::EntityRigFallback;
use client_world::{ActorRigSnapshot, ActorSnapshot, PlayerProfile};
use protocol::{ActorKind, PlayerSkin};
use render::{
    ActorCullView, ActorRenderIdentity, ActorRigRenderInput, ActorRigRoute, ActorRigSubmission,
    ActorSkinPixels, EntityRigId, MAX_RENDERED_PLAYERS, RenderBoneTransform,
    actor_rig_submission_is_visible, default_actor_skin_rgba8, normalize_actor_skin,
};
use semantic_input::PerspectiveMode;

#[derive(Clone, Debug)]
pub(crate) struct ActorRigPresentation {
    pub(crate) submission: ActorRigSubmission,
    pub(crate) skin_rgba8: Option<Arc<[u8]>>,
}

impl ActorRigPresentation {
    #[cfg(test)]
    pub(crate) fn from_render_submission(
        submission: ActorRigSubmission,
        skin: ActorSkinPixels,
    ) -> Option<Self> {
        Some(Self {
            submission,
            skin_rgba8: Some(normalize_actor_skin(&skin)?),
        })
    }
}

#[derive(Debug)]
pub(crate) struct ActorPresentationBatch {
    pub(crate) submissions: Vec<ActorRigSubmission>,
    pub(crate) skins_rgba8: Arc<[u8]>,
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
        || !actor.is_render_eligible()
    {
        return None;
    }

    let previous_bones = convert_bones(rig.previous)?;
    let current_bones = convert_bones(rig.current)?;
    let position = interpolated_position(actor, partial_tick.clamp(0.0, 1.0))?;
    let yaw = lerp_degrees(
        actor.previous_pose.yaw,
        actor.yaw,
        partial_tick.clamp(0.0, 1.0),
    );
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

#[cfg(test)]
pub(crate) fn select_actor_presentations(
    perspective: PerspectiveMode,
    local_runtime_id: u64,
    local: Option<ActorRigPresentation>,
    remotes: impl IntoIterator<Item = ActorRigPresentation>,
) -> ActorPresentationBatch {
    select_actor_presentations_for_view(perspective, local_runtime_id, local, remotes, None)
}

pub(crate) fn select_actor_presentations_for_view(
    perspective: PerspectiveMode,
    local_runtime_id: u64,
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

    let local = (perspective != PerspectiveMode::FirstPerson)
        .then_some(local)
        .flatten()
        .filter(|local| local.submission.input.identity.runtime_id == local_runtime_id);
    let mut selected = Vec::with_capacity(MAX_RENDERED_PLAYERS);
    let mut drawable_count = 0usize;
    if let Some(mut local) = local {
        normalize_no_draw_route(&mut local);
        if local.submission.route == ActorRigRoute::NoDraw
            || actor_rig_submission_is_visible(&local.submission, view)
        {
            drawable_count += usize::from(local.submission.route != ActorRigRoute::NoDraw);
            selected.push(local);
        }
    }
    for mut remote in latest.into_values() {
        normalize_no_draw_route(&mut remote);
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
        if presentation.submission.route == ActorRigRoute::NoDraw {
            presentation.submission.texture_layer = u32::MAX;
            submissions.push(presentation.submission);
            continue;
        }
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
    let skins_rgba8 = skin_bytes.into();
    ActorPresentationBatch {
        submissions,
        skins_rgba8,
    }
}

fn normalize_no_draw_route(presentation: &mut ActorRigPresentation) {
    if presentation.skin_rgba8.is_none() {
        presentation.submission.route = ActorRigRoute::NoDraw;
        presentation.submission.texture_layer = u32::MAX;
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
