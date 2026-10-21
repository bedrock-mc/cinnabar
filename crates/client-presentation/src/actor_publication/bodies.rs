//! Samples canonical actor bodies, then places the local body at the visible camera sample.
use super::*;
use crate::local_player::FrozenLocalAvatarVisibility;
use crate::presentation::actors::{
    ActorRigPresentation, local_actor_presentation_for_visibility, local_diagnostic_presentation,
};
use crate::presentation::{actors::PoseConversions, skin_rig::SkinRigCache};
use bevy::math::Vec3;
use render_model::ActorCullView;

mod parallel;

/// Captured view facts shared by body and hand sampling.
#[derive(Clone, Copy)]
pub(super) struct PoseView {
    pub partial_tick: f32,
    pub first_person: bool,
    pub java_mode: bool,
    pub combat_visuals: render_api::CombatVisuals,
    pub sampling_camera: Option<([f32; 2], [f32; 3])>,
    /// Viewport width over height the first-person hand reads.
    pub aspect: f32,
}

/// Retained pose conversions and skin registration used while sampling bodies.
pub(super) struct BodyCaches<'a> {
    pub coverage: &'a [u64],
    pub poses: &'a mut PoseConversions,
    pub java_poses: &'a mut java::PoseScratch,
    pub native_pose: &'a mut hand::NativePoseCache,
    pub skin_rigs: &'a mut SkinRigCache,
    pub equipment: Option<&'a mut EquipmentRuntime>,
    pub scratch: &'a mut BodyScratch,
}

/// Remote persona layers sampled this frame, by runtime id.
pub(super) type RemoteSkinLayers =
    bevy::platform::collections::HashMap<u64, Arc<[actor_animation::SkinRenderLayer]>>;

/// Storage of the per-frame Java poses and remote persona layers, kept between frames so a
/// steady frame does not reallocate it.
#[derive(Default)]
pub(super) struct BodyScratch {
    java_posed: Vec<java::Posed>,
    remote_skin_layers: RemoteSkinLayers,
}

impl BodyScratch {
    /// Takes back a finished frame's storage, emptied so no pose outlives its frame.
    pub(super) fn recycle(
        &mut self,
        mut java_posed: Vec<java::Posed>,
        mut remote_skin_layers: RemoteSkinLayers,
    ) {
        java_posed.clear();
        remote_skin_layers.clear();
        self.java_posed = java_posed;
        self.remote_skin_layers = remote_skin_layers;
    }
}

/// Canonical bodies and their sampled animation data, before local visibility placement.
pub(super) struct SampledBodies<'a> {
    pub local_runtime_id: u64,
    pub actor_session_id: u64,
    pub dimension: i32,
    pub remotes: Vec<ActorRigPresentation>,
    pub mask_only_runtime_ids: Vec<u64>,
    pub canonical_local: Option<ActorRigPresentation>,
    pub unrigged_actors: usize,
    pub local_emote_pose: Option<actor_animation::CustomEmotePose>,
    pub remote_emote_poses:
        bevy::platform::collections::HashMap<u64, actor_animation::CustomEmotePose>,
    pub java_posed: Vec<java::Posed>,
    pub native_body_sampled: bool,
    pub remote_skin_layers: RemoteSkinLayers,
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
            combat_visuals,
            sampling_camera,
            aspect: _,
        },
        cull_view,
    ) = view;
    let BodyCaches {
        coverage,
        poses,
        java_poses,
        native_pose,
        skin_rigs,
        mut equipment,
        scratch,
    } = caches;
    let mut java_posed = std::mem::take(&mut scratch.java_posed);
    java_posed.clear();
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
    let mut mask_only_runtime_ids = Vec::new();
    let mut remote_emote_poses = bevy::platform::collections::HashMap::new();
    let mut remote_skin_layers = std::mem::take(&mut scratch.remote_skin_layers);
    remote_skin_layers.clear();
    let mut occlusion_work = occlusion::PreparationWork::default();
    let mut render_frame = stream.map(|stream| stream.authority().actor_render_frame(partial_tick));
    let (local_runtime_id, actor_session_id, dimension, remotes, canonical_local, unrigged_actors) =
        stream
            .map(|stream| {
                let local_runtime_id = stream.local_player_runtime_id();
                let mut remotes = Vec::with_capacity(stream.authority().actor_count());
                let mut canonical_local = None;
                let mut rigged = 0;
                let mut pending = Vec::new();
                for rig in stream.authority().actor_rigs() {
                    rigged += 1;
                    let Some(actor) = stream.authority().actor(rig.actor.runtime_id) else {
                        continue;
                    };
                    if actor.painting.is_some() {
                        continue;
                    }
                    let coverage = coverage.contains(&rig.actor.runtime_id);
                    let batch_body = !coverage
                        && rig.actor.runtime_id != local_runtime_id
                        && !java_mode
                        && matches!(actor.kind, protocol::ActorKind::Player { .. })
                        && !input.remote_emotes.iter().any(|emote| {
                            emote.runtime_id == rig.actor.runtime_id
                                && emote.spawn_revision == actor.spawn_revision
                        })
                        && render_frame.as_ref().is_some_and(|frame| {
                            !frame.samples_during_admission(rig.actor.runtime_id)
                        });
                    if !batch_body {
                        parallel::publish(
                            &mut pending,
                            render_frame.as_mut(),
                            cull_view,
                            combat_visuals,
                            skin_rigs,
                            equipment.as_deref_mut(),
                            new_geometries,
                            &mut remote_skin_layers,
                            &mut remotes,
                        );
                    }
                    let mut sample_scale = |rig| {
                        render_frame
                            .as_mut()
                            .map_or(rig, |frame| frame.sample_rig_scale(rig))
                    };
                    let rig = if rig.actor.runtime_id == local_runtime_id || coverage {
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
                    let visible = rig.actor.runtime_id == local_runtime_id
                        || crate::presentation::actors::rig_may_be_visible(
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
                        );
                    if !visible && !coverage {
                        occlusion_work.hidden_by_occlusion += u32::from(occluded.get());
                        continue;
                    }
                    let remote_emote_pose = (rig.actor.runtime_id != local_runtime_id)
                        .then(|| {
                            let emote = input.remote_emotes.iter().find(|emote| {
                                emote.runtime_id == rig.actor.runtime_id
                                    && emote.spawn_revision == actor.spawn_revision
                            })?;
                            actor_animation::sample_custom_emote(
                                &rig,
                                emote.emote,
                                emote.elapsed,
                                emote.elapsed,
                            )
                        })
                        .flatten();
                    let sampled_rig = remote_emote_pose
                        .as_ref()
                        .map_or(rig, |pose| pose.snapshot(rig));
                    let profile = stream
                        .authority()
                        .actor_player_profile(rig.actor.runtime_id);
                    let presentation = if matches!(actor.kind, protocol::ActorKind::Player { .. }) {
                        let local = rig.actor.runtime_id == local_runtime_id;
                        let java_pose = (java_mode
                            && remote_emote_pose.is_none()
                            && !(local && (first_person || input.custom_emote.is_some())))
                        .then(|| {
                            let local = local.then_some(&input.local_equipment);
                            java::third_person(stream, &rig, actor, local, partial_tick, java_poses)
                        })
                        .flatten();
                        let presented = if remote_emote_pose.is_some() {
                            crate::presentation::actors::actor_rig_presentation(
                                &sampled_rig,
                                actor,
                                profile,
                                partial_tick,
                            )
                        } else {
                            crate::presentation::actors::actor_rig_presentation_cached(
                                java_pose.as_ref().map_or(&rig, |java_pose| &java_pose.rig),
                                actor,
                                profile,
                                partial_tick,
                                poses,
                            )
                        };
                        presented.map(|mut presentation| {
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
                            if !batch_body
                                && !local
                                && remote_emote_pose.is_none()
                                && !java_applied
                                && let Some(frame) = render_frame.as_mut()
                                && let Some(skin) = hand::NativePoseCache::apply_frame_body(
                                    frame,
                                    &mut presentation,
                                )
                            {
                                remote_skin_layers.insert(rig.actor.runtime_id, skin);
                            }
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
                    let Some(mut presentation) = presentation else {
                        continue;
                    };
                    if batch_body {
                        pending.push(parallel::RemoteBody {
                            rig,
                            actor,
                            presentation,
                        });
                        continue;
                    }
                    if rig.actor.runtime_id != local_runtime_id {
                        let admission_rig = emote_geometry::skin_layer_snapshot(
                            rig,
                            remote_emote_pose.as_ref(),
                            java::posed(&java_posed, rig.actor.runtime_id)
                                .map(|posed| posed.skin_layers.as_slice()),
                            remote_skin_layers
                                .get(&rig.actor.runtime_id)
                                .map(AsRef::as_ref),
                        );
                        let visible_bounds = crate::presentation::actors::admit_frame_bounds(
                            &mut presentation,
                            &admission_rig,
                            cull_view,
                            render_frame.as_mut(),
                        );
                        if !visible_bounds && !coverage {
                            continue;
                        }
                        if !visible || !visible_bounds {
                            mask_only_runtime_ids.push(rig.actor.runtime_id);
                        }
                    }
                    preparation::register_player_skin(
                        &mut presentation,
                        &sampled_rig,
                        skin_rigs,
                        equipment.as_deref_mut(),
                        new_geometries,
                    );
                    presentation.submission.overlay_rgba8 =
                        crate::presentation::combat::overlay(actor, combat_visuals);
                    if rig.actor.runtime_id == local_runtime_id {
                        canonical_local = Some(presentation);
                    } else {
                        if let Some(pose) = remote_emote_pose {
                            remote_emote_poses.insert(rig.actor.runtime_id, pose);
                        }
                        remotes.push(presentation);
                    }
                }
                parallel::publish(
                    &mut pending,
                    render_frame.as_mut(),
                    cull_view,
                    combat_visuals,
                    skin_rigs,
                    equipment,
                    new_geometries,
                    &mut remote_skin_layers,
                    &mut remotes,
                );
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
        mask_only_runtime_ids,
        canonical_local,
        unrigged_actors,
        local_emote_pose,
        remote_emote_poses,
        java_posed,
        native_body_sampled,
        remote_skin_layers,
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
    equipment: Option<&mut EquipmentRuntime>,
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
                equipment,
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
