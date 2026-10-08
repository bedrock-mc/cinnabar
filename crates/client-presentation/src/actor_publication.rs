mod commit;
mod preparation;
pub use preparation::{ActorFrameState, advance_actor_frame};
mod emote_geometry;
mod equipment_layers;
use equipment_layers::local_equipment;
mod hand;
mod java;
pub use commit::{PreparedActorPublication, publish_actor_render_frame};
use hand::{HandSource, hand_motion_matrix, publish_hand_rig};
#[cfg(test)]
use hand::{hand_camera_from_rig, hand_progress};

use std::sync::Arc;

use bevy::prelude::DetectChanges;
use bevy::{
    ecs::system::SystemParam,
    math::Mat4,
    prelude::{Projection, Res, ResMut, Resource, Time, Vec3},
    time::Real,
};
use chunk_pipeline::WorldStream;
use client_world::LocalPlayerFeed;
use render::{
    ActorCullView, ActorMainWitness, ActorRenderScene, ActorRigFrameBuilder, ActorRigSubmission,
    HandItemAtlas, HandRigLight, HandRigScene, MAX_ACTOR_RENDER_DISTANCE_BLOCKS, RuntimeStage,
    RuntimeStageProfiler,
};

use crate::camera::{CameraSettingsAuthority, FlyCamera};
use crate::local_player::{LocalAvatarPresentation, LocalAvatarVisibilityCarrier, LocalViewPose};
use crate::{
    actor_clock::{ActorFrameClock, authoritative_local_actor_eye, publish_local_actor_visibility},
    dropped_items::DroppedItemPublisher,
    presentation::actors::{
        ActorRigPresentation, local_actor_presentation_for_visibility,
        local_diagnostic_presentation, rig_world_from_actor, select_actor_presentations_for_view,
    },
    presentation::equipment::{
        EquipmentRuntime, FirstPersonArms, FirstPersonHand, FirstPersonItem, StagedSessionIcons,
        remote_input,
    },
};
use bevy::prelude::{Query, Transform, With};

/// The exact stream and immutable artwork snapshots borrowed for actor publication.
pub struct ActorWorld<'a> {
    pub stream: Option<&'a mut WorldStream>,
    pub collisions: Option<&'a dyn crate::observations::CollisionLookup>,
    pub entity_assets: Option<&'a assets::RuntimeEntityAssets>,
    pub pack_entities: Option<Arc<assets::SessionEntityPack>>,
    pub session_items: Option<Arc<crate::session_assets::SessionItems>>,
    pub prepared_actor_artwork:
        &'a mut Option<Arc<crate::prepared_actor_artwork::PreparedActorArtwork>>,
}

/// Gameplay and screen observations captured at the actor preparation boundary.
pub struct ActorFrameInput {
    pub local_feed: Option<LocalPlayerFeed>,
    pub predicted_eye: Option<[f32; 3]>,
    pub predicted_feet: Option<[f32; 3]>,
    pub local_equipment: crate::presentation::equipment::ActorEquipmentInput,
    pub swing_progress: Option<client_world::LocalSwingProgress>,
    pub renders_game: bool,
    pub hide_hand: bool,
    pub custom_emote: Option<(client_world::CustomEmote, f64)>,
}

/// Presentation-owned resources that determine the avatar's current view.
#[derive(SystemParam)]
pub struct ActorPresentationState<'w, 's> {
    avatar: Res<'w, LocalAvatarPresentation>,
    local_visibility: ResMut<'w, LocalAvatarVisibilityCarrier>,
    settings: Res<'w, CameraSettingsAuthority>,
    server_camera: Option<Res<'w, crate::camera::ServerCameraView>>,
    view: Res<'w, LocalViewPose>,
    camera: Query<'w, 's, (&'static Transform, &'static Projection), With<FlyCamera>>,
}

/// The frame fraction the actor rigs interpolate at, for overlays anchored to actors.
#[derive(Resource, Default)]
pub struct ActorFramePartialTick(pub f32);

/// The local player's own first-person rig, built as a single instance placed in camera space.
#[derive(Resource)]
pub struct HandRigBuilder(pub ActorRigFrameBuilder);

impl HandRigBuilder {
    pub fn from_runtime_assets(assets: &assets::RuntimeEntityAssets) -> anyhow::Result<Self> {
        ActorRigFrameBuilder::from_runtime_assets(assets)
            .map(Self)
            .map_err(|error| {
                anyhow::anyhow!("prepare validated first-person hand rig geometry: {error:?}")
            })
    }
}

/// Vertical FOV of the first-person pass; underwater and death-camera narrowing are not modelled.
pub const HAND_FOV_DEGREES: f32 = 70.0;

/// Rebuilds session artwork and item routes, or restores startup artwork after disconnect.
fn apply_session_pack(
    scene: &mut ActorRenderScene,
    mut pages: render::ActorArtworkPages,
    pack: Option<&crate::session_assets::SessionEntityPack>,
    session_icons: Option<StagedSessionIcons>,
    geometry_ready: &mut SessionGeometryReady,
    equipment: Option<&mut EquipmentRuntime>,
    profiler: Option<&RuntimeStageProfiler>,
) -> (
    Option<render::ActorArtworkPages>,
    Option<StagedSessionIcons>,
    Vec<Option<render::ActorArtworkLocation>>,
) {
    let equipment_timer = profiler.map(|profiler| profiler.time(RuntimeStage::ActorEquipmentSetup));
    let mut layer = None;
    let mut geometries = Vec::new();
    if let Some(pack) = pack
        && let Some(catalog) = &pack.equipment
    {
        let (extended, locations) =
            pages.with_equipment_rasters(&EquipmentRuntime::pack_rasters(catalog));
        // Startup pages carry the vanilla glint, so disconnect restores it.
        pages = match EquipmentRuntime::actor_glint(catalog) {
            Some(glint) => extended.with_actor_glint(glint),
            None => extended,
        };
        if !geometry_ready.equipment {
            geometries = EquipmentRuntime::pack_geometries(&pack.assets, catalog);
        }
        layer = Some((Arc::clone(&pack.assets), Arc::clone(catalog), locations));
    }
    if let Some(equipment) = equipment {
        equipment.set_pack_layer(layer);
    }
    let mut icon_locations = Vec::new();
    if let Some(icons) = &session_icons {
        let (extended, locations) = pages.with_equipment_rasters(icons.rasters());
        pages = extended;
        icon_locations = locations;
    }
    drop(equipment_timer);
    if !geometry_ready.entities || !geometry_ready.equipment {
        apply_session_geometry(scene, pack, geometries, geometry_ready, profiler);
    }
    scene.configure_artwork(pages.clone());
    let effective = (pack.is_some() || session_icons.is_some()).then_some(pages);
    (effective, session_icons, icon_locations)
}

/// Each accepted namespace can survive item/artwork refreshes independently.
#[derive(Default)]
struct SessionGeometryReady {
    entities: bool,
    equipment: bool,
}

/// Retries rejected namespaces while retaining geometry that already published successfully.
fn apply_session_geometry(
    scene: &mut ActorRenderScene,
    pack: Option<&crate::session_assets::SessionEntityPack>,
    geometries: Vec<render_model::ActorRigGeometry>,
    ready: &mut SessionGeometryReady,
    profiler: Option<&RuntimeStageProfiler>,
) {
    let geometry_timer = profiler.map(|profiler| profiler.time(RuntimeStage::ActorGeometrySetup));
    let assets = pack.map(|pack| &*pack.assets);
    let (entities, equipment) = match (ready.entities, ready.equipment) {
        (false, false) => scene.replace_session_pack_geometries(assets, geometries),
        (false, true) => (scene.replace_pack_entities(assets), Ok(())),
        (true, false) => (Ok(()), scene.replace_pack_equipment(geometries)),
        (true, true) => return,
    };
    ready.entities = entities.is_ok();
    ready.equipment = equipment.is_ok();
    if let Err(error) = entities {
        bevy::log::warn!(?error, "server pack entity geometry was not applied");
    }
    if let Err(error) = equipment {
        bevy::log::warn!(?error, "server pack equipment geometry was not applied");
    }
    drop(geometry_timer);
}

#[derive(SystemParam)]
pub struct ActorFramePublication<'w, 's> {
    time: Res<'w, Time<Real>>,
    state: ResMut<'w, ActorFrameState>,
    scene: ResMut<'w, ActorRenderScene>,
    prepared: ResMut<'w, PreparedActorPublication>,
    presentation: ActorPresentationState<'w, 's>,
    artwork: Res<'w, render::ActorArtworkPages>,
    hand_builder: ResMut<'w, HandRigBuilder>,
    hand_scene: ResMut<'w, HandRigScene>,
    hand_motion: Option<Res<'w, crate::camera::FirstPersonHandMotion>>,
    equipment: Option<ResMut<'w, EquipmentRuntime>>,
    glint_settings: Option<Res<'w, render::UiGlintSettings>>,
    dropped_items: DroppedItemPublisher<'w, 's>,
    profiler: Option<Res<'w, render::RuntimeStageProfiler>>,
    partial_tick: ResMut<'w, ActorFramePartialTick>,
}

/// Builds final actor and hand poses after admission, without advancing remote actor ticks.
pub fn prepare_actor_render_frame(
    mut client_world: ActorWorld<'_>,
    swing_progress: Option<client_world::LocalSwingProgress>,
    hides_box: impl Fn(&WorldStream, [f32; 3], [f32; 3], [f32; 3]) -> bool,
    params: ActorFramePublication,
) {
    let ActorFramePublication {
        time: _,
        mut state,
        mut scene,
        mut prepared,
        presentation,
        artwork,
        mut hand_builder,
        mut hand_scene,
        hand_motion,
        mut equipment,
        glint_settings,
        mut dropped_items,
        profiler,
        partial_tick: _,
    } = params;
    let preparation::ActorFrameState {
        session_artwork,
        cape_state,
        skin_rigs,
        skin_layers,
        poses,
        layer_poses,
        hand_revision,
        java_hand,
        input,
        step,
        hand_source: ready_hand_source,
        hand_key: ready_hand_key,
        captured_view,
        captured_sampling_camera,
        first_person,
        java_mode,
        hand_use,
        ..
    } = &mut *state;
    let Some(input) = input.take() else {
        return;
    };
    let step = step.expect("actor preparation precedes final rendering");
    let artwork = session_artwork.as_ref().unwrap_or(&artwork);
    let ActorPresentationState {
        local_visibility,
        camera,
        ..
    } = presentation;
    let first_person = *first_person;
    let java_mode = *java_mode;
    let view = *captured_view;
    let mut java_posed = Vec::new();
    if let Some(stream) = client_world.stream.as_mut() {
        if let Some(progress) = swing_progress {
            stream.sync_local_swing(progress);
        }
        stream.set_actor_camera_rotation(actor_camera_rotation(view.camera_rotation()));
        stream.advance_actor_interpolation_frame(0);
    }
    let authoritative_subject_eye = authoritative_local_actor_eye(
        input.predicted_eye,
        client_world
            .stream
            .as_ref()
            .map(|stream| stream.resolved_server_position().position),
    );
    // The hand's independent perspective survives the world's portal projection.
    let hand_camera_fov = camera
        .single()
        .ok()
        .and_then(|(_, projection)| crate::camera::first_person_hand_fov(projection));
    let preparation = profiler
        .as_deref()
        .map(|profiler| profiler.time(render::RuntimeStage::ActorPreparation));
    let cull_view = camera
        .single()
        .ok()
        .map(|(transform, projection)| ActorCullView {
            clip_from_world: projection.get_clip_from_view() * transform.to_matrix().inverse(),
            camera_position: transform.translation,
            max_distance: MAX_ACTOR_RENDER_DISTANCE_BLOCKS,
        });
    // Registered together below: each registration rebuilds and re-uploads the whole catalog.
    let mut new_geometries = Vec::new();
    let local_emote_pose = (!first_person)
        .then(|| {
            let stream = client_world.stream.as_ref()?;
            let rig = stream
                .authority()
                .actor_rig(stream.local_player_runtime_id())?;
            let (emote, elapsed) = input.custom_emote?;
            client_world::sample_custom_emote(&rig, emote, elapsed, elapsed)
        })
        .flatten();
    let mut native_body_sampled = false;
    let (local_runtime_id, actor_session_id, dimension, remotes, canonical_local, unrigged_actors) =
        client_world
            .stream
            .as_ref()
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
                    // Culled before any per-actor work; the local rig also drives the hand.
                    if rig.actor.runtime_id != local_runtime_id
                        && !crate::presentation::actors::rig_may_be_visible(
                            &rig,
                            actor,
                            step.partial_tick,
                            cull_view,
                            |low, high| {
                                cull_view.is_some_and(|view| {
                                    hides_box(stream, view.camera_position.to_array(), low, high)
                                })
                            },
                        )
                    {
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
                            java::third_person(stream, &rig, actor, local, step.partial_tick)
                        })
                        .flatten();
                        crate::presentation::actors::actor_rig_presentation_cached(
                            java_pose.as_ref().map_or(&rig, |java_pose| &java_pose.rig),
                            actor,
                            profile,
                            step.partial_tick,
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
                                    step.partial_tick,
                                );
                                java_posed.push(java_pose.posed);
                            }
                            if local && !first_person && !java_applied && local_emote_pose.is_none()
                            {
                                java_hand.native_pose.apply_body(
                                    stream,
                                    &mut presentation,
                                    step.partial_tick,
                                    *captured_sampling_camera,
                                );
                                native_body_sampled = true;
                            }
                            preparation::register_player_skin(
                                &mut presentation,
                                &rig,
                                skin_rigs,
                                equipment.as_deref_mut(),
                                &mut new_geometries,
                            );
                            presentation
                        })
                    } else {
                        crate::presentation::actors::entity_rig_presentation_cached(
                            &rig,
                            actor,
                            artwork,
                            step.partial_tick,
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
    // First person draws the player's own rig near the camera: the visible arms with every other
    // bone hidden, and a drawable held item in its own first-person frame. Anything not covered
    // (an undrawable item) leaves the CPU viewmodel in charge.
    if java_mode
        && let Some(rig) = client_world
            .stream
            .as_ref()
            .and_then(|stream| stream.authority().actor_rig(local_runtime_id))
    {
        java_hand.remember(&rig, input.local_equipment.main.as_ref());
    }
    let mut reused_hand = false;
    let hand_source: Option<HandSource> = if first_person && input.renders_game {
        canonical_local.clone().and_then(|presentation| {
            let stream = client_world.stream.as_ref()?;
            let equipment = equipment.as_deref_mut()?;
            let equipment_input = local_equipment(stream, local_runtime_id, &input.local_equipment);
            let (consume_ticks, item_animation) = *hand_use;
            let motion = hand_motion
                .as_deref()
                .map_or(Mat4::IDENTITY, hand_motion_matrix);
            let key = hand::source_key(stream, consume_ticks, item_animation, step.partial_tick);
            if key == *ready_hand_key {
                reused_hand = true;
                return ready_hand_source.take();
            }
            hand::source(
                hand::HandInputs {
                    stream,
                    presentation,
                    equipment_input: &equipment_input,
                    owner_equipment: &equipment_input,
                    consume_ticks,
                    item_animation,
                    alpha: step.partial_tick,
                    artwork,
                    motion,
                    sampling_camera: *captured_sampling_camera,
                },
                java_mode,
                equipment,
                java_hand,
            )
        })
    } else {
        None
    };
    if let Some(equipment) = equipment.as_deref_mut() {
        equipment.finish_hand_readiness(reused_hand);
    }
    let visibility_snapshot = local_visibility.snapshot().copied();
    let (local_visible, local) = visibility_snapshot.map_or((false, None), |visibility| {
        if visibility.runtime_id() != local_runtime_id {
            return (false, None);
        }
        // Camera and body share the physics render sample while the rig retains its animation.
        let local = canonical_local
            .map(|mut local| {
                place_local_actor_at_render_feet(&mut local, visibility.feet());
                if java::posed(&java_posed, local_runtime_id).is_some() {
                    let sneaking = client_world
                        .stream
                        .as_ref()
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
    let local_death = client_world
        .stream
        .as_ref()
        .and_then(|stream| stream.authority().actor(local_runtime_id))
        .and_then(|actor| actor.death_rotation_progress(step.partial_tick));
    let local = local.map(|mut local| {
        if let (Some(pose), Some(stream)) = (&local_emote_pose, &client_world.stream)
            && let (Some(rig), Some(actor)) = (
                stream.authority().actor_rig(local_runtime_id),
                stream.authority().actor(local_runtime_id),
            )
            && let Some(animated) = crate::presentation::actors::actor_rig_presentation(
                &pose.snapshot(rig),
                actor,
                stream.authority().actor_player_profile(local_runtime_id),
                step.partial_tick,
            )
        {
            emote_geometry::apply(
                &pose.snapshot(rig),
                skin_rigs,
                equipment.as_deref_mut(),
                &mut new_geometries,
                &mut local.submission,
                animated.submission,
            );
        }

        let java_death_ticks = java::posed(&java_posed, local_runtime_id).and_then(|_| {
            let actor = client_world
                .stream
                .as_ref()?
                .authority()
                .actor(local_runtime_id)?;
            Some(f32::from(actor.status.death_time) + step.partial_tick)
        });
        local.submission.world_from_actor = java::local_death_tilt(
            local.submission.world_from_actor,
            local_death,
            java_death_ticks,
        );
        local
    });
    let camera_position = cull_view
        .as_ref()
        .map(|view| view.camera_position.to_array());
    let mut batch = select_actor_presentations_for_view(
        local_runtime_id,
        local_visible,
        local,
        remotes,
        cull_view,
    );
    if let Some(stream) = client_world.stream.as_ref() {
        crate::presentation::actors::light_bodies(&mut batch, stream);
    }
    let selected_count = batch.submissions.len();
    if let (Some(equipment), Some(stream)) =
        (equipment.as_deref_mut(), client_world.stream.as_ref())
    {
        equipment_layers::attach(
            &mut batch,
            equipment,
            stream,
            local_runtime_id,
            &input.local_equipment,
            &java_posed,
            (step.partial_tick, glint_settings.as_deref()),
        );
    }
    if let (Some(stream), Some(cape)) = (
        client_world.stream.as_ref(),
        cape_state.rig(client_world.entity_assets),
    ) {
        if !scene.contains_geometry(cape.id) {
            let _ = scene.insert_geometry(cape.geometry.clone());
        }
        crate::presentation::cape::apply_capes(
            &mut batch,
            cape,
            |runtime_id| {
                stream.authority().actor_rig(runtime_id).map(|rig| {
                    if runtime_id == local_runtime_id
                        && let Some(pose) = &local_emote_pose
                    {
                        pose.snapshot(rig)
                    } else {
                        rig
                    }
                })
            },
            |runtime_id| stream.authority().actor_player_profile(runtime_id),
            |runtime_id| java::posed(&java_posed, runtime_id).map(|posed| posed.cape),
            |runtime_id| {
                let input = if runtime_id == local_runtime_id {
                    local_equipment(stream, runtime_id, &input.local_equipment)
                } else {
                    remote_input(stream, runtime_id)
                };
                input.armor[1].as_ref().is_some_and(|item| {
                    equipment
                        .as_deref()
                        .is_some_and(|equipment| equipment.is_elytra(&item.identifier))
                })
            },
        );
    }

    // After equipment, which rides the rig's own model even when a controller draws another.
    if let Some(stream) = client_world.stream.as_ref() {
        let mut render_frame = stream.authority().actor_render_frame(step.partial_tick);
        crate::presentation::entity_layers::apply_render_layers_cached(
            &mut batch,
            |runtime_id| {
                if runtime_id == local_runtime_id
                    && let Some(pose) = &local_emote_pose
                {
                    Some(std::borrow::Cow::Borrowed(pose.render.as_slice()))
                } else {
                    render_frame.layers(runtime_id)
                }
            },
            artwork,
            layer_poses,
        );
    }
    if let Some(stream) = client_world.stream.as_ref()
        && let Some(pages) = skin_layers.apply(
            &mut batch,
            artwork,
            |runtime_id| {
                let rig = stream.authority().actor_rig(runtime_id)?;
                let emote = (runtime_id == local_runtime_id)
                    .then_some(local_emote_pose.as_ref())
                    .flatten();
                let java_layers =
                    java::posed(&java_posed, runtime_id).map(|posed| posed.skin_layers.as_slice());
                let native_layers = (runtime_id == local_runtime_id && native_body_sampled)
                    .then(|| java_hand.native_pose.skin_layers())
                    .flatten();
                Some(emote_geometry::skin_layer_snapshot(
                    rig,
                    emote,
                    java_layers,
                    native_layers,
                ))
            },
            skin_rigs,
            |geometry| new_geometries.push(geometry),
        )
    {
        scene.configure_artwork(pages);
    }
    // Hiding skin layers keeps armor and held items visible.
    if let Some(stream) = client_world.stream.as_ref() {
        for submission in &mut batch.submissions {
            let identity = submission.input.identity;
            if (identity.layer == render::ACTOR_LAYER_BODY
                || identity.layer == crate::presentation::cape::ACTOR_LAYER_CAPE
                || crate::presentation::skin_layers::is_skin_layer(identity.layer)
                || identity.layer >= crate::presentation::entity_layers::ACTOR_LAYER_TEXTURE_BASE)
                && stream
                    .authority()
                    .actor(identity.runtime_id)
                    .is_some_and(|actor| actor.is_invisible())
            {
                submission.route = render::ActorRigRoute::NoDraw;
            }
        }
    }
    if let Some(equipment) = equipment.as_deref_mut() {
        new_geometries.extend(equipment.take_pending_geometries());
    }
    drop(preparation);
    {
        let _rig_build = profiler
            .as_deref()
            .map(|profiler| profiler.time(render::RuntimeStage::ActorRigBuild));
        register_geometries(&mut hand_builder.0, &mut scene, new_geometries);
    }
    prepared.0 = Some(commit::PendingActorPublication {
        batch,
        partial_tick: step.partial_tick,
        witness: ActorMainWitness {
            local_snapshot: visibility_snapshot.is_some(),
            local_visible,
            expected_runtime_id: local_runtime_id,
            visibility_runtime_id: visibility_snapshot.map_or(0, |snapshot| snapshot.runtime_id()),
            selected_count,
            local_route: None,
            frame_instances: 0,
            frame_manifest: 0,
            skin_bytes: 0,
            rejects: Default::default(),
            unrigged_actors,
        },
    });
    let hand_light = client_world.stream.as_ref().map_or(
        HandRigLight {
            block_level: 0,
            sky_level: 0,
            daylight: 1.0,
            pad: 0,
            ..Default::default()
        },
        |stream| {
            let (block, sky) = authoritative_subject_eye
                .map_or((0, 0), |eye| stream.light_level_at(eye.to_array()));
            HandRigLight {
                block_level: u32::from(block),
                sky_level: u32::from(sky),
                // Reserved legacy field; the hand samples the shared world lightmap.
                daylight: 1.0,
                pad: 0,
                ..Default::default()
            }
        },
    );
    let hand_light = if java_mode {
        hand_light.with_java_lighting(hand::java_light_matrix(
            hand_motion.as_deref(),
            view.rotation(),
        ))
    } else {
        hand_light
    };
    dropped_items.publish(
        client_world.stream.as_deref(),
        client_world.collisions,
        camera_position.map(|position| {
            let (yaw, _, _) = view.rotation().to_euler(bevy::math::EulerRot::YXZ);
            (position, (180.0 - yaw.to_degrees()).rem_euclid(360.0))
        }),
        step.partial_tick,
    );
    publish_hand_rig(
        &mut hand_builder.0,
        &mut hand_scene,
        hand_revision,
        hand_source.filter(|_| !input.hide_hand),
        hand_camera_fov,
        hand_light,
        step.partial_tick,
    );
}

/// Rigs this far outside the view on every side still animate, so only a turn faster than this
/// in one tick shows a rig its held pose for that tick.
const ANIMATION_GUARD_DEGREES: f32 = 30.0;

fn actor_camera_rotation(rotation: bevy::math::Quat) -> [f32; 2] {
    let (yaw, pitch, _) = rotation.to_euler(bevy::math::EulerRot::YXZ);
    [
        -pitch.to_degrees(),
        (180.0 - yaw.to_degrees()).rem_euclid(360.0),
    ]
}

/// The camera's frustum widened by the guard band, with the render distances.
fn animation_view(
    transform: &bevy::prelude::Transform,
    projection: &Projection,
) -> Option<client_world::ActorAnimationView> {
    let Projection::Perspective(perspective) = projection else {
        return None;
    };
    let (guard, limit) = (ANIMATION_GUARD_DEGREES.to_radians(), 85f32.to_radians());
    let half_vertical = perspective.fov * 0.5;
    let half_horizontal = (half_vertical.tan() * perspective.aspect_ratio).atan();
    let (half_vertical, half_horizontal) = (
        (half_vertical + guard).min(limit),
        (half_horizontal + guard).min(limit),
    );
    let clip = Mat4::perspective_infinite_reverse_rh(
        half_vertical * 2.0,
        half_horizontal.tan() / half_vertical.tan(),
        perspective.near,
    ) * transform.to_matrix().inverse();
    let [x, y, z, w] = [0, 1, 2, 3].map(|row| clip.row(row));
    let planes = [w + x, w - x, w + y, w - y, z, w - z].map(|plane| plane.to_array());
    planes
        .iter()
        .flatten()
        .all(|value| value.is_finite())
        .then(|| client_world::ActorAnimationView {
            planes,
            camera: transform.translation.to_array(),
            player_distance: MAX_ACTOR_RENDER_DISTANCE_BLOCKS,
            entity_radius: render::ACTOR_CANDIDATE_RADIUS_BLOCKS,
        })
}

/// Registers new skin models and item meshes in both rig catalogs with one rebuild each; if a
/// batch is refused, each is tried alone so a rejected mesh only leaves that model undrawn.
fn register_geometries(
    hand: &mut ActorRigFrameBuilder,
    scene: &mut ActorRenderScene,
    geometries: Vec<render_model::ActorRigGeometry>,
) {
    if geometries.is_empty() {
        return;
    }
    if hand.insert_geometries(geometries.clone()).is_err() {
        for geometry in geometries.iter().cloned() {
            let _ = hand.insert_geometry(geometry);
        }
    }
    if scene.insert_geometries(geometries.clone()).is_err() {
        for geometry in geometries {
            let _ = scene.insert_geometry(geometry);
        }
    }
}

/// Positions the local rig at the same render sample as its camera subject.
fn place_local_actor_at_render_feet(presentation: &mut ActorRigPresentation, feet: Vec3) {
    for (row, coordinate) in presentation
        .submission
        .world_from_actor
        .iter_mut()
        .zip(feet.to_array())
    {
        row[3] = coordinate;
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod xp_orb_tests;

#[cfg(test)]
mod billboard_frame_tests;
