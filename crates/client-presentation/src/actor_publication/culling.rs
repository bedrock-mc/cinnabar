use bevy::math::Mat4;
use bevy::prelude::{Projection, Transform};
use client_world::{ActorRigSnapshot, ActorSnapshot};
use render::ActorCullView;

#[derive(Clone, Copy)]
pub(super) struct ActorPublicationViews {
    pub(super) main: ActorCullView,
    pub(super) publication: ActorCullView,
    pub(super) casts_shadows: bool,
}

impl ActorPublicationViews {
    pub(super) fn new(transform: &Transform, projection: &Projection, casts_shadows: bool) -> Self {
        let main = ActorCullView {
            clip_from_world: projection.get_clip_from_view() * transform.to_matrix().inverse(),
            camera_position: transform.translation,
            max_distance: render::MAX_ACTOR_RENDER_DISTANCE_BLOCKS,
        };
        let publication = if casts_shadows {
            let radius = main.max_distance;
            ActorCullView {
                clip_from_world: Mat4::orthographic_rh(
                    -radius, radius, -radius, radius, -radius, radius,
                ) * Mat4::from_translation(-transform.translation),
                ..main
            }
        } else {
            main
        };
        Self {
            main,
            publication,
            casts_shadows,
        }
    }
}

pub(super) fn rig_may_be_published(
    rig: &ActorRigSnapshot<'_>,
    actor: &ActorSnapshot,
    partial_tick: f32,
    views: Option<ActorPublicationViews>,
    occluded: impl Fn([f32; 3], [f32; 3]) -> bool,
) -> bool {
    crate::presentation::actors::rig_may_be_visible(
        rig,
        actor,
        partial_tick,
        views.map(|views| views.publication),
        |low, high| !views.is_some_and(|views| views.casts_shadows) && occluded(low, high),
    )
}

pub(super) fn animation_view(
    transform: &Transform,
    projection: &Projection,
    views: Option<ActorPublicationViews>,
) -> Option<client_world::ActorAnimationView> {
    let Some(views) = views.filter(|views| views.casts_shadows) else {
        return super::animation_view(transform, projection);
    };
    let clip = views.publication.clip_from_world;
    let [x, y, z, w] = [0, 1, 2, 3].map(|row| clip.row(row));
    Some(client_world::ActorAnimationView {
        planes: [w + x, w - x, w + y, w - y, z, w - z].map(|plane| plane.to_array()),
        camera: transform.translation.to_array(),
        player_distance: views.publication.max_distance,
        entity_radius: render::ACTOR_CANDIDATE_RADIUS_BLOCKS,
    })
}

pub(super) fn shadow_only_local(
    mut local: crate::presentation::actors::ActorRigPresentation,
) -> crate::presentation::actors::ActorRigPresentation {
    if local.submission.route == render::ActorRigRoute::Diagnostic {
        local.submission.input.rig = render_model::DIAGNOSTIC_RIG_ID;
    }
    local.submission.route = render::ActorRigRoute::ShadowOnly;
    local
}

pub(super) fn local_shadow_body(
    mut local: crate::presentation::actors::ActorRigPresentation,
    stream: Option<&chunk_pipeline::WorldStream>,
    poses: &mut crate::presentation::actors::PoseConversions,
) -> Option<crate::presentation::actors::ActorRigPresentation> {
    match local.submission.route {
        render::ActorRigRoute::NoDraw => return None,
        render::ActorRigRoute::Diagnostic => {}
        _ => {
            let rig = stream?
                .authority()
                .actor_world_body_rig(local.submission.input.identity.runtime_id)?;
            if !poses.apply_pose(&mut local, &rig) {
                return None;
            }
        }
    }
    Some(shadow_only_local(local))
}

pub(super) fn shadow_only_local_layers(
    batch: &mut crate::presentation::actors::ActorPresentationBatch,
    runtime_id: u64,
) {
    for submission in &mut batch.submissions {
        if submission.input.identity.runtime_id == runtime_id
            && submission.route != render::ActorRigRoute::NoDraw
        {
            if submission.route == render::ActorRigRoute::Diagnostic {
                submission.input.rig = render_model::DIAGNOSTIC_RIG_ID;
            }
            submission.route = render::ActorRigRoute::ShadowOnly;
        }
    }
}

pub(super) fn world_rig<'a>(
    stream: &'a chunk_pipeline::WorldStream,
    runtime_id: u64,
    local_runtime_id: u64,
    local_shadows: bool,
) -> Option<ActorRigSnapshot<'a>> {
    if local_shadows && runtime_id == local_runtime_id {
        stream.authority().actor_world_body_rig(runtime_id)
    } else {
        stream.authority().actor_rig(runtime_id)
    }
}

#[cfg(test)]
mod tests;
