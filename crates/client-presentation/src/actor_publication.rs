#[cfg(test)]
mod armour_tests;
mod bodies;
#[cfg(test)]
mod cape_tests;
mod cloth_contacts;
mod commit;
mod companions;
mod flames;
mod geometry;
mod ground_effects;
mod hand_frame;
mod hats;
pub use ground_effects::{GroundEffectWearer, GroundEffectWearers};
pub use hats::{CosmeticEmitterKey, CosmeticEmitterPlacement, CosmeticEmitterPlacements};
mod layers;
mod occlusion;
mod paintings;
pub use flames::WorldActorFlames;
mod views;
use geometry::{SessionGeometryReady, apply_session_pack, register_geometries};
mod preparation;
pub use preparation::{ActorFrameState, advance_actor_frame, advance_actor_motion};
mod emote_geometry;
mod equipment_layers;
use equipment_layers::local_equipment;
mod hand;
mod java;
pub use commit::{PreparedActorPublication, publish_actor_render_frame};
use hand::{HandSource, publish_hand_rig};

use std::sync::Arc;

use bevy::{
    ecs::system::SystemParam,
    prelude::{Projection, Res, ResMut, Resource, Time},
    time::Real,
};
use chunk_pipeline::WorldStream;
use client_world::LocalPlayerFeed;
use render_model::{ActorCullView, ActorRigFrameBuilder};
use render_scene::{ActorRenderScene, HandRigScene, RuntimeStage, RuntimeStageProfiler};

use crate::camera::{CameraSettingsAuthority, FlyCamera};
use crate::local_player::{LocalAvatarPresentation, LocalAvatarVisibilityCarrier, LocalViewPose};
use crate::{
    actor_clock::authoritative_local_actor_eye,
    dropped_items::DroppedItemPublisher,
    presentation::{actors::select_actor_presentations, equipment::EquipmentRuntime},
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
    pub cosmetics: Vec<cosmetics::Item>,
    /// Authenticated nearby hats bound to current actor lifetimes.
    pub remote_hats: Vec<RemoteHat>,
    pub local_feed: Option<LocalPlayerFeed>,
    pub predicted_eye: Option<[f32; 3]>,
    pub predicted_feet: Option<[f32; 3]>,
    pub local_equipment: crate::presentation::equipment::ActorEquipmentInput,
    pub swing_progress: Option<actor_animation::LocalSwingProgress>,
    pub renders_game: bool,
    pub hide_hand: bool,
    pub custom_emote: Option<(cosmetics::emotes::Item, f64)>,
    /// API-authenticated playback already resolved to this session's actor lifetimes.
    pub remote_emotes: Vec<RemoteEmote>,
}

/// Render-only remote playback bound to one spawned actor, preventing runtime-ID reuse.
#[derive(Clone, Copy, Debug)]
pub struct RemoteEmote {
    pub runtime_id: u64,
    pub spawn_revision: u64,
    pub emote: cosmetics::emotes::Item,
    pub elapsed: f64,
}

/// A built-in hat bound to one remote spawn, independent of server skin handling.
#[derive(Clone, Copy, Debug)]
pub struct RemoteHat {
    pub runtime_id: u64,
    pub spawn_revision: u64,
    pub hat: cosmetics::Item,
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

/// Optional validated cosmetic tint of already active native actor damage overlays.
#[derive(Resource, Default)]
pub struct ActorPresentationStyle {
    pub hurt_color: Option<[f32; 4]>,
    pub hurt_armor_color: Option<[f32; 4]>,
    /// Native admitted ray target; never a guest-provided actor identity.
    pub attackable_actor: Option<u64>,
    pub attackable_color: Option<[f32; 4]>,
}

/// The frame fraction the actor rigs interpolate at, for overlays anchored to actors.
#[derive(Resource, Default)]
pub struct ActorFramePartialTick(pub f32);

/// The local player's own first-person rig, built as a single instance placed in camera space.
#[derive(Resource)]
pub struct HandRigBuilder(pub ActorRigFrameBuilder);

/// Vertical FOV of the first-person pass; underwater and death-camera narrowing are not modelled.
pub const HAND_FOV_DEGREES: f32 = 70.0;

#[derive(SystemParam)]
pub struct ActorFramePublication<'w, 's> {
    time: Res<'w, Time<Real>>,
    actor_style: Option<Res<'w, ActorPresentationStyle>>,
    state: ResMut<'w, ActorFrameState>,
    coverage: Option<Res<'w, render_scene::mod_scene::ActorCoverageRequests>>,
    scene: ResMut<'w, ActorRenderScene>,
    prepared: ResMut<'w, PreparedActorPublication>,
    presentation: ActorPresentationState<'w, 's>,
    artwork: Res<'w, render_scene::ActorArtworkResource>,
    hand_builder: ResMut<'w, HandRigBuilder>,
    hand_scene: ResMut<'w, HandRigScene>,
    hand_motion: Option<Res<'w, crate::camera::FirstPersonHandMotion>>,
    equipment: Option<ResMut<'w, EquipmentRuntime>>,
    glint_settings: Option<Res<'w, render_scene::UiGlintSettings>>,
    dropped_items: DroppedItemPublisher<'w, 's>,
    profiler: Option<Res<'w, render_scene::RuntimeStageProfiler>>,
    partial_tick: ResMut<'w, ActorFramePartialTick>,
    flames: Option<ResMut<'w, WorldActorFlames>>,
    cosmetic_emitters: Option<ResMut<'w, CosmeticEmitterPlacements>>,
    ground_effects: Option<ResMut<'w, GroundEffectWearers>>,
}

/// Builds final actor and hand poses after admission, without advancing remote actor ticks.
pub fn prepare_actor_render_frame(
    mut client_world: ActorWorld<'_>,
    swing_progress: Option<actor_animation::LocalSwingProgress>,
    hides_box: impl Fn(&WorldStream, ActorCullView, [f32; 3], [f32; 3]) -> bool,
    icons: Option<&client_ui::ui_runtime::presentation::UiPresentationRuntime>,
    mut params: ActorFramePublication,
) {
    let state = &mut *params.state;
    let Some(input) = state.input.take() else {
        if let Some(placements) = params.cosmetic_emitters.as_deref_mut() {
            placements.0.clear();
        }
        if let Some(wearers) = params.ground_effects.as_deref_mut() {
            wearers.0.clear();
        }
        return;
    };
    let step = state
        .step
        .expect("actor preparation precedes final rendering");
    let artwork = state.session_artwork.as_ref().unwrap_or(&params.artwork);
    if let Some(equipment) = params.equipment.as_deref_mut() {
        equipment.set_show_custom_item_models(
            params.presentation.settings.feel().show_custom_item_models,
        );
    }
    let view = state.captured_view;
    let pose_view = bodies::PoseView {
        partial_tick: step.partial_tick,
        first_person: state.first_person,
        java_mode: state.java_mode,
        combat_visuals: state.combat_visuals,
        sampling_camera: state.captured_sampling_camera,
        aspect: state.captured_aspect,
    };
    views::refresh_interpolation(
        client_world.stream.as_deref_mut(),
        swing_progress,
        view.camera_rotation(),
    );
    let stream = client_world.stream.as_deref();
    if let Some(wearers) = params.ground_effects.as_deref_mut() {
        wearers.capture(&input, stream);
    }
    let authoritative_subject_eye = authoritative_local_actor_eye(
        input.predicted_eye,
        stream.map(|stream| stream.resolved_server_position().position),
    );
    let hand_camera_fov = params.presentation.hand_camera_fov();
    let preparation = params
        .profiler
        .as_deref()
        .map(|profiler| profiler.time(RuntimeStage::ActorPreparation));
    let cull_view = params.presentation.cull_view();
    let mut new_geometries = Vec::new();
    // The hand and body share controller sampling before local placement.
    let coverage = params
        .coverage
        .as_deref()
        .filter(|requests| {
            stream.is_some_and(|stream| {
                requests.connection == stream.authority().actor_session_id()
                    && requests.dimension == stream.current_dimension()
            })
        })
        .map_or(&[][..], |requests| requests.runtime_ids.as_slice());
    let mut bodies = bodies::sample(
        stream,
        &input,
        (pose_view, cull_view),
        artwork,
        bodies::BodyCaches {
            coverage,
            poses: &mut state.poses,
            java_poses: &mut state.java_poses,
            native_pose: &mut state.java_hand.native_pose,
            skin_rigs: &mut state.skin_rigs,
            equipment: params.equipment.as_deref_mut(),
            scratch: &mut state.body_scratch,
        },
        hides_box,
        &mut new_geometries,
    );
    state.paintings.refresh(
        client_world.entity_assets,
        client_world.pack_entities.as_deref(),
        artwork,
    );
    let hand_source = hand_frame::prepare(
        stream,
        bodies.local_runtime_id,
        &bodies.canonical_local,
        &input,
        pose_view,
        state.hand_use,
        artwork,
        params.hand_motion.as_deref(),
        hand_frame::HandCaches {
            source: &mut state.hand_source,
            key: &state.hand_key,
            java_hand: &mut state.java_hand,
            equipment: params.equipment.as_deref_mut(),
        },
    );
    let coverage_local = coverage.contains(&bodies.local_runtime_id);
    if state.first_person
        && coverage_local
        && let Some(stream) = stream
    {
        bodies::apply_local_coverage_pose(&mut bodies, stream, &mut state.coverage_poses);
    }
    let visibility = params.presentation.local_visibility.snapshot().copied();
    let (local_visible, local) = bodies::place_local(
        &mut bodies,
        stream,
        visibility,
        step.partial_tick,
        &mut state.skin_rigs,
        params.equipment.as_deref_mut(),
        &mut new_geometries,
    );
    let local_flame_feet = flames::LocalFlameFeet::capture(local.as_ref(), visibility);
    if !local_visible
        && state.hat_state.has_local_cloth()
        && let (Some(body), Some(stream)) = (local.as_ref(), stream)
        && let Some(rig) = stream.authority().actor_rig(bodies.local_runtime_id)
    {
        let rig = emote_geometry::skin_layer_snapshot(
            rig,
            bodies.local_emote_pose.as_ref(),
            java::posed(&bodies.java_posed, bodies.local_runtime_id)
                .map(|posed| posed.skin_layers.as_slice()),
            bodies
                .native_body_sampled
                .then(|| state.java_hand.native_pose.skin_layers())
                .flatten(),
        );
        let contacts = cloth_contacts::hidden(
            &body.submission,
            &rig,
            (stream, &input.local_equipment, state.combat_visuals),
            (
                &params.scene,
                &mut new_geometries,
                params.equipment.as_deref_mut(),
            ),
            (step.partial_tick, params.time.delta_secs()),
        );
        state.hat_state.advance_hidden_local(
            &body.submission,
            rig.bone_names,
            rig.attachments,
            step.partial_tick,
            Some(&contacts),
        );
    }
    let camera_position = cull_view.map(|view| view.camera_position.to_array());
    // Remote bodies passed frame-model admission before bounded skin registration.
    let hidden_companion_body = (!local_visible && !coverage_local)
        .then(|| local.as_ref().map(|local| local.submission.clone()))
        .flatten();
    let mut batch = select_actor_presentations(
        bodies.local_runtime_id,
        local_visible || coverage_local,
        local,
        bodies.remotes,
    );
    if let Some(stream) = stream {
        state
            .paintings
            .append(stream.authority(), step.partial_tick, cull_view, &mut batch);
    }
    if let Some(stream) = stream {
        crate::presentation::actors::light_bodies(&mut batch, stream);
    }
    let selected_count = bodies
        .occlusion
        .record_selection(&batch, bodies.local_runtime_id);
    layers::attach(
        &mut batch,
        stream,
        bodies.render_frame.as_mut(),
        layers::LayerPoses {
            local_runtime_id: bodies.local_runtime_id,
            local_equipment: &input.local_equipment,
            java_posed: &bodies.java_posed,
            combat_visuals: state.combat_visuals,
            local_emote_pose: bodies.local_emote_pose.as_ref(),
            remote_emote_poses: &bodies.remote_emote_poses,
            native_body_sampled: bodies.native_body_sampled,
            local_coverage: state.first_person && coverage_local,
            remote_skin_layers: &bodies.remote_skin_layers,
            native_pose: &state.java_hand.native_pose,
        },
        layers::LayerCaches {
            layer_poses: &mut state.layer_poses,
            equipment: params.equipment.as_deref_mut(),
            cape_state: &mut state.cape_state,
            skin_layers: &mut state.skin_layers,
            skin_rigs: &mut state.skin_rigs,
        },
        artwork,
        client_world.entity_assets,
        &mut params.scene,
        (
            step.partial_tick,
            params.time.delta_secs(),
            params.glint_settings.as_deref(),
        ),
        &mut new_geometries,
    );
    if let Some(equipment) = params.equipment.as_deref_mut() {
        new_geometries.extend(equipment.take_pending_geometries());
    }
    state.hat_state.attach(
        &mut batch,
        stream,
        &input.remote_hats,
        &mut params.scene,
        step.partial_tick,
        hats::AttachmentFrame {
            collisions: client_world.collisions,
            hidden_local: hidden_companion_body.as_ref(),
            geometries: &new_geometries,
            equipment: params.equipment.as_deref(),
            local_emote: bodies.local_emote_pose.as_ref(),
            remote_emotes: Some(&bodies.remote_emote_poses),
        },
    );
    layers::apply_style_and_visibility(&mut batch, stream, params.actor_style.as_deref());
    if let Some(placements) = params.cosmetic_emitters.as_deref_mut() {
        state
            .hat_state
            .place_emitters(&batch, step.partial_tick, &mut placements.0);
    }
    state
        .body_scratch
        .recycle(bodies.java_posed, bodies.remote_skin_layers);
    if let Some(flames) = params.flames.as_deref_mut()
        && let Ok((camera, _)) = params.presentation.camera.single()
    {
        let (yaw, _, _) = camera.rotation.to_euler(bevy::math::EulerRot::YXZ);
        flames.append(
            stream.map(WorldStream::authority),
            yaw,
            step.partial_tick,
            local_flame_feet,
            &mut batch,
        );
    }
    drop(preparation);
    register_geometries(
        &mut params.hand_builder.0,
        &mut params.scene,
        new_geometries,
        params.profiler.as_deref(),
    );
    let independent_layers = state.hat_state.independent_layers(&batch);
    params.prepared.0 = Some(commit::PendingActorPublication::new(
        batch,
        step.partial_tick,
        visibility,
        local_visible,
        (
            stream.map(|stream| stream.authority().actor_session_id()),
            bodies.local_runtime_id,
        ),
        selected_count,
        bodies.unrigged_actors,
    ));
    if let Some(pending) = params.prepared.0.as_mut() {
        pending.independent_layers = independent_layers;
        pending.mask_only_runtime_ids = bodies.mask_only_runtime_ids;
        if !local_visible && coverage_local {
            pending.mask_only_runtime_ids.push(bodies.local_runtime_id);
        }
    }
    let hand_light = hand_frame::lighting(
        stream,
        bodies.local_runtime_id,
        visibility,
        input.predicted_feet,
        authoritative_subject_eye,
        state.java_mode,
        params.hand_motion.as_deref(),
        view.rotation(),
    );
    params.dropped_items.publish(
        icons,
        stream,
        client_world.collisions,
        (
            camera_position.map(|position| {
                let (yaw, _, _) = view.rotation().to_euler(bevy::math::EulerRot::YXZ);
                (position, (180.0 - yaw.to_degrees()).rem_euclid(360.0))
            }),
            cull_view,
        ),
        step.partial_tick,
        (
            artwork.item_glint.clone(),
            render_model::ActorGlint {
                profile: render_api::CombatVisuals::Modern,
                time_seconds: params.time.elapsed_secs(),
                strength: params
                    .glint_settings
                    .as_deref()
                    .map_or(1.0, |settings| settings.strength),
                speed: params
                    .glint_settings
                    .as_deref()
                    .map_or(1.0, |settings| settings.speed),
                ..Default::default()
            },
        ),
    );
    publish_hand_rig(
        &mut params.hand_builder.0,
        &mut params.hand_scene,
        &mut state.hand_revision,
        hand_source.filter(|_| !input.hide_hand),
        hand_camera_fov,
        hand_light,
        (step.partial_tick, params.glint_settings.as_deref()),
    );
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod culling_capacity_tests;

#[cfg(test)]
mod xp_orb_tests;

#[cfg(test)]
mod billboard_frame_tests;
