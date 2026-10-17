use super::java;
use actor_animation::ActorRigSnapshot;
use bevy::math::Mat4;
use bevy::prelude::{Projection, Transform};
use client_world::ActorSnapshot;
use render_model::ActorCullView;

#[derive(Clone, Copy)]
pub(super) struct ActorPublicationViews {
    pub(super) main: ActorCullView,
    pub(super) publication: ActorCullView,
    pub(super) casts_shadows: bool,
}

impl ActorPublicationViews {
    /// Views for the gameplay camera; only Enhanced shadow views widen publication.
    pub(super) fn for_camera(
        (transform, projection, enhanced): (
            &Transform,
            &Projection,
            Option<&render_scene::EnhancedRendering>,
        ),
    ) -> Self {
        let casts_shadows = render_model::ENHANCED_RENDERING_ENABLED
            && enhanced.is_some_and(|settings| settings.shadows && !settings.reflection_capture);
        Self::new(transform, projection, casts_shadows)
    }

    pub(super) fn new(transform: &Transform, projection: &Projection, casts_shadows: bool) -> Self {
        let main = ActorCullView {
            clip_from_world: projection.get_clip_from_view() * transform.to_matrix().inverse(),
            camera_position: transform.translation,
            max_distance: render_model::MAX_ACTOR_RENDER_DISTANCE_BLOCKS,
        };
        let publication = if casts_shadows {
            let radius = main.max_distance;
            ActorCullView {
                clip_from_world: glam::camera::rh::proj::directx::orthographic(
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
    frame: Option<&mut actor_animation::ActorRenderFrame<'_>>,
    occluded: impl Fn([f32; 3], [f32; 3]) -> bool,
) -> bool {
    crate::presentation::actors::rig_may_be_visible(
        rig,
        actor,
        partial_tick,
        views.map(|views| views.publication),
        frame,
        |low, high| !views.is_some_and(|views| views.casts_shadows) && occluded(low, high),
    )
}

pub(super) fn animation_view(
    transform: &Transform,
    projection: &Projection,
    views: Option<ActorPublicationViews>,
) -> Option<actor_animation::ActorAnimationView> {
    let Some(views) = views.filter(|views| views.casts_shadows) else {
        return super::views::animation_view(transform, projection);
    };
    let clip = views.publication.clip_from_world;
    let [x, y, z, w] = [0, 1, 2, 3].map(|row| clip.row(row));
    Some(actor_animation::ActorAnimationView {
        planes: [w + x, w - x, w + y, w - y, z, w - z].map(|plane| plane.to_array()),
        camera: transform.translation.to_array(),
        player_distance: views.publication.max_distance,
        entity_radius: render_model::ACTOR_CANDIDATE_RADIUS_BLOCKS,
    })
}

pub(super) fn shadow_only_local(
    mut local: crate::presentation::actors::ActorRigPresentation,
) -> crate::presentation::actors::ActorRigPresentation {
    local.submission.make_shadow_only();
    local
}

/// Replaces the local presentation with its shadow-only world body when `enabled`.
pub(super) fn with_local_shadow_body(
    local: Option<crate::presentation::actors::ActorRigPresentation>,
    enabled: bool,
    stream: Option<&chunk_pipeline::WorldStream>,
    caches: (
        &mut crate::presentation::actors::PoseConversions,
        &mut java::PoseScratch,
        &mut Vec<java::Posed>,
    ),
    animation: (bool, &crate::presentation::equipment::ActorEquipmentInput),
    partial_tick: f32,
) -> Option<crate::presentation::actors::ActorRigPresentation> {
    if !enabled {
        return local;
    }
    local.and_then(|local| local_shadow_body(local, stream, caches, animation, partial_tick))
}

/// Samples the selected third-person animation at the local render feet for shadow publication.
fn local_shadow_body(
    mut local: crate::presentation::actors::ActorRigPresentation,
    stream: Option<&chunk_pipeline::WorldStream>,
    caches: (
        &mut crate::presentation::actors::PoseConversions,
        &mut java::PoseScratch,
        &mut Vec<java::Posed>,
    ),
    animation: (bool, &crate::presentation::equipment::ActorEquipmentInput),
    partial_tick: f32,
) -> Option<crate::presentation::actors::ActorRigPresentation> {
    let (poses, java_poses, java_posed) = caches;
    let (java_mode, equipment) = animation;
    match local.submission.route {
        render_model::ActorRigRoute::NoDraw => return None,
        render_model::ActorRigRoute::Diagnostic => {}
        _ => {
            let rig = stream?
                .authority()
                .actor_world_body_rig(local.submission.input.identity.runtime_id)?;
            let actor = stream?.authority().actor(rig.actor.runtime_id)?;
            let java_pose = java_mode
                .then(|| {
                    java::third_person(
                        stream?,
                        &rig,
                        actor,
                        Some(equipment),
                        partial_tick,
                        java_poses,
                    )
                })
                .flatten();
            let selected = java_pose.as_ref().map_or(&rig, |pose| &pose.rig);
            if java_pose.is_some() {
                local.world_yaw_degrees = crate::presentation::actors::transform::lerp_degrees(
                    selected.previous_body_yaw,
                    selected.body_yaw,
                    partial_tick.clamp(0.0, 1.0),
                );
            }
            if !poses.apply_pose(&mut local, selected, actor, partial_tick) {
                return None;
            }
            if let Some(pose) = java_pose {
                java::apply_pose(&mut local, &pose.bones, true, actor, partial_tick);
                java::lift(
                    &mut local.submission.world_from_actor,
                    actor.is_sneaking(),
                    true,
                );
                java_posed.push(pose.posed);
            }
        }
    }
    Some(shadow_only_local(local))
}

/// Keeps all drawable layers of a shadow-only body behind visible actors in capacity admission.
pub(super) fn shadow_only_caster_layers(
    batch: &mut crate::presentation::actors::ActorPresentationBatch,
) {
    let mut casters: Vec<_> = batch
        .submissions
        .iter()
        .filter(|submission| {
            submission.input.identity.layer == render_model::ACTOR_LAYER_BODY
                && submission.route == render_model::ActorRigRoute::ShadowOnly
        })
        .map(|submission| submission.input.identity.runtime_id)
        .collect();
    casters.sort_unstable();
    for submission in &mut batch.submissions {
        if casters
            .binary_search(&submission.input.identity.runtime_id)
            .is_ok()
        {
            submission.make_shadow_only();
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
