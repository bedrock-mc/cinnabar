//! Samples canonical actor bodies, then places the local body at the visible camera sample.
use super::*;
use crate::local_player::FrozenLocalAvatarVisibility;
use crate::presentation::actors::{
    ActorRigPresentation, local_actor_presentation_for_visibility, local_diagnostic_presentation,
};
use crate::presentation::{actors::PoseConversions, skin_rig::SkinRigCache};
use bevy::math::Vec3;
use render_model::ActorCullView;

/// Captured view facts shared by body and hand sampling.
#[derive(Clone, Copy)]
pub(super) struct PoseView {
    pub partial_tick: f32,
    pub first_person: bool,
    pub java_mode: bool,
    pub sampling_camera: Option<([f32; 2], [f32; 3])>,
}

/// Retained pose conversions and skin registration used while sampling bodies.
pub(super) struct BodyCaches<'a> {
    pub poses: &'a mut PoseConversions,
    pub java_poses: &'a mut java::PoseScratch,
    pub native_pose: &'a mut hand::NativePoseCache,
    pub skin_rigs: &'a mut SkinRigCache,
    pub equipment: Option<&'a mut EquipmentRuntime>,
}

/// Canonical bodies and their sampled animation data, before local visibility placement.
pub(super) struct SampledBodies<'a> {
    pub local_runtime_id: u64,
    pub actor_session_id: u64,
    pub dimension: i32,
    pub remotes: Vec<ActorRigPresentation>,
    pub canonical_local: Option<ActorRigPresentation>,
    pub unrigged_actors: usize,
    pub local_emote_pose: Option<actor_animation::CustomEmotePose>,
    pub java_posed: Vec<java::Posed>,
    pub native_body_sampled: bool,
    pub occlusion: occlusion::PreparationWork,
    pub render_frame: Option<actor_animation::ActorRenderFrame<'a>>,
}

/// Samples visible bodies in authority order, sharing one controller budget for the frame.
pub(super) fn sample<'a>(
    stream: Option<&'a WorldStream>,
    input: &ActorFrameInput,
    view: (PoseView, Option<ActorCullView>),
    artwork: &render_model::ActorArtworkPages,
    caches: BodyCaches<'_>,
    hides_box: impl Fn(&WorldStream, ActorCullView, [f32; 3], [f32; 3]) -> bool,
    new_geometries: &mut Vec<render_model::ActorRigGeometry>,
) -> SampledBodies<'a> {
    let (
        PoseView {
            partial_tick,
            first_person,
            java_mode,
            sampling_camera,
        },
        cull_view,
    ) = view;
    let BodyCaches {
        poses,
        java_poses,
        native_pose,
        skin_rigs,
        mut equipment,
    } = caches;
    let mut java_posed = Vec::new();
    let local_emote_pose = (!first_person)
        .then(|| {
            let stream = stream?;
            let rig = stream
                .authority()
                .actor_rig(stream.local_player_runtime_id())?;
            let (emote, elapsed) = input.custom_emote?;
            actor_animation::sample_custom_emote(&rig, emote, elapsed, elapsed)
        })
        .flatten();
    let mut native_body_sampled = false;
    let mut occlusion_work = occlusion::PreparationWork::default();
    let mut render_frame = stream.map(|stream| stream.authority().actor_render_frame(partial_tick));
    let (local_runtime_id, actor_session_id, dimension, remotes, canonical_local, unrigged_actors) =
        stream
            .map(|stream| {
                let local_runtime_id = stream.local_player_runtime_id();
                let mut remotes = Vec::with_capacity(stream.authority().actor_count());
                let mut canonical_local = None;
                let mut rigged = 0;
                for rig in stream.authority().actor_rigs() {
                    rigged += 1;
                    let Some(actor) = stream.authority().actor(rig.actor.runtime_id) else {
                        continue;
                    };
                    let mut sample_scale = |rig| {
                        render_frame
                            .as_mut()
                            .map_or(rig, |frame| frame.sample_rig_scale(rig))
                    };
                    let rig = if rig.actor.runtime_id == local_runtime_id {
                        sample_scale(rig)
                    } else {
                        let Some(rig) = crate::presentation::actors::sample_candidate_scale(
                            rig,
                            actor,
                            partial_tick,
                            cull_view,
                            sample_scale,
                        ) else {
                            continue;
                        };
                        rig
                    };
                    // Cull at the sampled frame scale; the local rig also drives the hand.
                    let occluded = std::cell::Cell::new(false);
                    occlusion_work.candidates +=
                        u32::from(rig.actor.runtime_id != local_runtime_id);
                    if rig.actor.runtime_id != local_runtime_id
                        && !crate::presentation::actors::rig_may_be_visible(
                            &rig,
                            actor,
                            partial_tick,
                            cull_view,
                            render_frame.as_mut(),
                            |low, high| {
                                cull_view.is_some_and(|view| {
                                    let (low, high) = occlusion::body_and_shadow_bounds(
                                        stream,
                                        &rig,
                                        partial_tick,
                                        low,
                                        high,
                                    );
                                    let hidden = hides_box(stream, view, low, high);
                                    occluded.set(hidden);
                                    hidden
                                })
                            },
                        )
                    {
                        occlusion_work.hidden_by_occlusion += u32::from(occluded.get());
                        continue;
                    }
                    let profile = stream
                        .authority()
                        .actor_player_profile(rig.actor.runtime_id);
                    let presentation = if matches!(actor.kind, protocol::ActorKind::Player { .. }) {
                        let local = rig.actor.runtime_id == local_runtime_id;
                        let java_pose = (java_mode
                            && !(local && (first_person || input.custom_emote.is_some())))
                        .then(|| {
                            let local = local.then_some(&input.local_equipment);
                            java::third_person(stream, &rig, actor, local, partial_tick, java_poses)
                        })
                        .flatten();
                        crate::presentation::actors::actor_rig_presentation_cached(
                            java_pose.as_ref().map_or(&rig, |java_pose| &java_pose.rig),
                            actor,
                            profile,
                            partial_tick,
                            poses,
                        )
                        .map(|mut presentation| {
                            let java_applied = java_pose.is_some();
                            if let Some(java_pose) = java_pose {
                                java::apply_pose(
                                    &mut presentation,
                                    &java_pose.bones,
                                    local,
                                    actor,
                                    partial_tick,
                                );
                                java_posed.push(java_pose.posed);
                            }
                            if local && !first_person && !java_applied && local_emote_pose.is_none()
                            {
                                native_pose.apply_body(
                                    stream,
                                    &mut presentation,
                                    partial_tick,
                                    sampling_camera,
                                );
                                native_body_sampled = true;
                            }
                            preparation::register_player_skin(
                                &mut presentation,
                                &rig,
                                skin_rigs,
                                equipment.as_deref_mut(),
                                new_geometries,
                            );
                            presentation
                        })
                    } else {
                        crate::presentation::actors::entity_rig_presentation_cached(
                            &rig,
                            actor,
                            artwork,
                            partial_tick,
                            Some(poses),
                        )
                    };
                    let Some(presentation) = presentation else {
                        continue;
                    };
                    if rig.actor.runtime_id == local_runtime_id {
                        canonical_local = Some(presentation);
                    } else {
                        remotes.push(presentation);
                    }
                }
                (
                    local_runtime_id,
                    stream.authority().actor_session_id(),
                    stream.current_dimension(),
                    remotes,
                    canonical_local,
                    stream.authority().actor_count().saturating_sub(rigged),
                )
            })
            .unwrap_or((0, 0, 0, Vec::new(), None, 0));
    SampledBodies {
        local_runtime_id,
        actor_session_id,
        dimension,
        remotes,
        canonical_local,
        unrigged_actors,
        local_emote_pose,
        java_posed,
        native_body_sampled,
        occlusion: occlusion_work,
        render_frame,
    }
}

/// Places the visible local body without changing the canonical pose already used by the hand.
pub(super) fn place_local(
    bodies: &mut SampledBodies<'_>,
    stream: Option<&WorldStream>,
    visibility_snapshot: Option<FrozenLocalAvatarVisibility>,
    partial_tick: f32,
    skin_rigs: &mut SkinRigCache,
    mut equipment: Option<&mut EquipmentRuntime>,
    new_geometries: &mut Vec<render_model::ActorRigGeometry>,
) -> (bool, Option<ActorRigPresentation>) {
    let local_runtime_id = bodies.local_runtime_id;
    let actor_session_id = bodies.actor_session_id;
    let dimension = bodies.dimension;
    let java_posed = &bodies.java_posed;
    let local_emote_pose = bodies.local_emote_pose.as_ref();
    let canonical_local = bodies.canonical_local.take();
    let (local_visible, local) = visibility_snapshot.map_or((false, None), |visibility| {
        if visibility.runtime_id() != local_runtime_id {
            return (false, None);
        }
        // Camera and body share the physics render sample while the rig retains its animation.
        let local = canonical_local
            .map(|mut local| {
                place_local_actor_at_render_feet(&mut local, visibility.feet());
                if java::posed(java_posed, local_runtime_id).is_some() {
                    let sneaking = stream
                        .and_then(|stream| stream.authority().actor(local_runtime_id))
                        .is_some_and(|actor| actor.is_sneaking());
                    java::lift(&mut local.submission.world_from_actor, sneaking, true);
                }
                local
            })
            .or_else(|| {
                let (yaw, pitch, _) = visibility.rotation().to_euler(bevy::math::EulerRot::YXZ);
                let yaw_degrees = (180.0 - yaw.to_degrees()).rem_euclid(360.0);
                let pitch_degrees = -pitch.to_degrees();
                let position = visibility.feet();
                let diagnostic = local_diagnostic_presentation(
                    actor_session_id,
                    dimension,
                    visibility.runtime_id(),
                    visibility.pose_generation(),
                    position.to_array(),
                    yaw_degrees,
                    pitch_degrees,
                );
                local_actor_presentation_for_visibility(
                    local_runtime_id,
                    visibility.runtime_id(),
                    None,
                    diagnostic,
                    yaw_degrees,
                )
            });
        (visibility.visible(), local)
    });
    // The visibility override rebuilds the local transform, so re-apply the death tip-over.
    let local_death = stream
        .and_then(|stream| stream.authority().actor(local_runtime_id))
        .and_then(|actor| actor.death_rotation_progress(partial_tick));
    let local = local.map(|mut local| {
        if let (Some(pose), Some(stream)) = (local_emote_pose, stream)
            && let (Some(rig), Some(actor)) = (
                stream.authority().actor_rig(local_runtime_id),
                stream.authority().actor(local_runtime_id),
            )
            && let Some(animated) = crate::presentation::actors::actor_rig_presentation(
                &pose.snapshot(rig),
                actor,
                stream.authority().actor_player_profile(local_runtime_id),
                partial_tick,
            )
        {
            emote_geometry::apply(
                &pose.snapshot(rig),
                skin_rigs,
                equipment.as_deref_mut(),
                new_geometries,
                &mut local.submission,
                animated.submission,
            );
        }

        let java_death_ticks = java::posed(java_posed, local_runtime_id).and_then(|_| {
            let actor = stream?.authority().actor(local_runtime_id)?;
            Some(f32::from(actor.status.death_time) + partial_tick)
        });
        local.submission.world_from_actor = java::local_death_tilt(
            local.submission.world_from_actor,
            local_death,
            java_death_ticks,
        );
        local
    });
    (local_visible, local)
}

/// Positions the local rig at the same render sample as its camera subject.
pub(super) fn place_local_actor_at_render_feet(
    presentation: &mut ActorRigPresentation,
    feet: Vec3,
) {
    for (row, coordinate) in presentation
        .submission
        .world_from_actor
        .iter_mut()
        .zip(feet.to_array())
    {
        row[3] = coordinate;
    }
}
