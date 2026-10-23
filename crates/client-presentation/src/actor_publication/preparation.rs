//! Advances actors and captures hand readiness before UI and outbound interactions.
use super::hand::hand_motion_matrix;
use super::*;
use crate::{
    actor_clock::{ActorFrameClock, publish_local_actor_visibility},
    presentation::{actors::ActorRigPresentation, equipment::StagedSessionIcons},
};
use bevy::math::Mat4;
use bevy::prelude::DetectChanges;

/// Retained actor state shared by early simulation and final render preparation.
#[derive(Resource, Default)]
pub struct ActorFrameState {
    pub(super) paintings: super::paintings::PaintingRoutes,
    pub(super) published_session: Option<u64>,
    pub(super) published_pack: Option<Arc<assets::SessionEntityPack>>,
    pub(super) pack_geometry_ready: SessionGeometryReady,
    pub(super) published_items: Option<Arc<crate::session_assets::SessionItems>>,
    published_artwork: Option<Arc<crate::prepared_actor_artwork::PreparedActorArtwork>>,
    pub(super) actor_clock: ActorFrameClock,
    pub(super) session_artwork: Option<render_model::ActorArtworkPages>,
    pub(super) hat_state: super::hats::HatState,
    pub(super) cape_state: crate::presentation::cape::CapeState,
    pub(super) skin_rigs: crate::presentation::skin_rig::SkinRigCache,
    pub(super) skin_layers: crate::presentation::skin_layers::SkinLayerCache,
    pub(super) poses: crate::presentation::actors::PoseConversions,
    pub(super) coverage_poses: crate::presentation::actors::PoseConversions,
    pub(super) layer_poses: crate::presentation::entity_layers::LayerPoseCache,
    pub(super) hand_revision: u64,
    pub(super) java_hand: java::HandCache,
    pub(super) java_poses: java::PoseScratch,
    pub(super) body_scratch: super::bodies::BodyScratch,
    pub(super) input: Option<ActorFrameInput>,
    pub(super) step: Option<crate::actor_clock::ActorFrameStep>,
    motion_step: Option<crate::actor_clock::ActorFrameStep>, // Taken by early remote motion.
    pub(super) hand_source: Option<HandSource>,
    pub(super) hand_key: Option<hand::SourceKey>,
    hand_ready: bool,
    pub(super) captured_view: LocalViewPose,
    pub(super) captured_sampling_camera: Option<([f32; 2], [f32; 3])>,
    pub(super) captured_aspect: f32,
    pub(super) first_person: bool,
    pub(super) java_mode: bool,
    pub(super) combat_visuals: render_api::CombatVisuals,
    pub(super) hand_use: (
        Option<u32>,
        Option<actor_animation::AttachableAnimationInput<'static>>,
    ),
}

impl ActorFrameState {
    /// Cosmetic motion shared by the world attachment and the HUD paperdoll for this frame.
    pub fn cosmetic_motion(&self) -> Vec<(cosmetics::Item, render_model::hats::Motion)> {
        self.hat_state.local_motion()
    }

    /// Shares the exact accepted artwork snapshot with client-owned actor publishers.
    pub fn actor_artwork<'a>(
        &'a self,
        startup: &'a render_model::ActorArtworkPages,
    ) -> &'a render_model::ActorArtworkPages {
        self.session_artwork.as_ref().unwrap_or(startup)
    }

    /// Returns the clock step used to evaluate and publish every actor in this frame.
    pub fn frame_step(&self) -> Option<crate::actor_clock::ActorFrameStep> {
        self.step
    }

    /// Returns the resolved camera rotation and position captured for actor evaluation.
    pub fn sampling_camera(&self) -> Option<([f32; 2], [f32; 3])> {
        self.captured_sampling_camera
    }

    /// Whether the current captured first-person source will pass hand-frame validation.
    pub fn hand_is_active(&self) -> bool {
        self.hand_ready
    }
}

/// Predicts remote poses from the supplied interval, independently of local physics.
/// Live actor publication later consumes the same step.
pub fn advance_actor_motion(
    state: &mut ActorFrameState,
    stream: Option<&mut WorldStream>,
    delta: std::time::Duration,
) {
    let session_id = stream
        .as_ref()
        .map(|stream| stream.authority().actor_session_id());
    // The session's first step owns the clock reset; the later preparation reuses this step.
    if state.published_session != session_id {
        state.actor_clock.reset();
    }
    let step = state.actor_clock.advance(delta);
    if let Some(stream) = stream {
        stream.predict_remote_actor_motion(step.ticks);
    }
    state.motion_step = Some(step);
}

/// Captures inventory and evaluates actor visuals once per frame.
pub fn advance_actor_frame(
    mut client_world: ActorWorld<'_>,
    mut input: ActorFrameInput,
    sample_world: impl FnOnce(&mut WorldStream),
    hand_use: impl Fn(
        &WorldStream,
        f32,
    ) -> (
        Option<u32>,
        Option<actor_animation::AttachableAnimationInput<'static>>,
    ),
    params: ActorFramePublication,
) {
    let ActorFramePublication {
        time,
        mut scene,
        mut state,
        presentation,
        artwork,
        mut hand_builder,
        hand_motion,
        mut equipment,
        profiler,
        mut partial_tick,
        ..
    } = params;
    let ActorFrameState {
        published_session,
        published_pack,
        pack_geometry_ready,
        published_items,
        published_artwork,
        actor_clock,
        motion_step,
        session_artwork,
        skin_rigs,
        skin_layers,
        cape_state,
        hat_state,
        poses,
        coverage_poses,
        layer_poses,
        java_hand,
        java_poses,
        hand_source,
        hand_key,
        hand_ready,
        input: captured_input,
        step: captured_step,
        captured_view,
        captured_sampling_camera,
        captured_aspect,
        first_person: captured_first_person,
        java_mode: captured_java_mode,
        combat_visuals: captured_combat_visuals,
        hand_use: captured_hand_use,
        ..
    } = &mut *state;
    let ActorPresentationState {
        avatar,
        mut local_visibility,
        settings,
        server_camera,
        view,
        camera,
    } = presentation;
    let session_id = client_world
        .stream
        .as_ref()
        .map(|stream| stream.authority().actor_session_id());
    // A step taken earlier this frame already reset and advanced the clock.
    let early = motion_step.take();
    let new_session = *published_session != session_id;
    if new_session {
        if session_id.is_none() {
            *client_world.prepared_actor_artwork = None;
        }
        scene.reset();
        if early.is_none() {
            actor_clock.reset();
        }
        *skin_layers = Default::default();
        *cape_state = Default::default();
        *hat_state = Default::default();
        *published_session = session_id;
    }
    let speed = input.local_feed.as_ref().map_or(0.0, |feed| {
        render_model::hats::forward_speed(feed.velocity, feed.yaw)
    });
    hat_state.advance(&input.cosmetics, speed, time.delta_secs());
    let pack = session_id.and_then(|_| client_world.pack_entities.clone());
    let items = session_id.and_then(|_| client_world.session_items.clone());
    let same = |left: Option<*const ()>, right: Option<*const ()>| left == right;
    let pack_changed = !same(
        published_pack.as_ref().map(|pack| Arc::as_ptr(pack).cast()),
        pack.as_ref().map(|pack| Arc::as_ptr(pack).cast()),
    );
    let items_changed = !same(
        published_items
            .as_ref()
            .map(|items| Arc::as_ptr(items).cast()),
        items.as_ref().map(|items| Arc::as_ptr(items).cast()),
    );
    let prepared = client_world.prepared_actor_artwork.clone();
    let prepared_changed = !same(
        published_artwork
            .as_ref()
            .map(|art| Arc::as_ptr(art).cast()),
        prepared.as_ref().map(|art| Arc::as_ptr(art).cast()),
    );
    if new_session || pack_changed || items_changed || prepared_changed || artwork.is_changed() {
        let _setup = profiler
            .as_deref()
            .map(|profiler| profiler.time(render_scene::RuntimeStage::ActorSessionSetup));
        *published_pack = pack.clone();
        *published_items = items.clone();
        *published_artwork = prepared.clone();
        let placed = crate::prepared_actor_artwork::session_icons(
            &artwork,
            pack.as_ref(),
            items.as_ref().and_then(|items| items.icons.as_ref()),
            prepared.as_deref(),
        );
        let icons = match placed {
            Some(placed) => SessionIconInput::Placed(placed),
            None => SessionIconInput::Staged(StagedSessionIcons::stage(items.as_deref())),
        };
        if new_session || pack_changed {
            *pack_geometry_ready = SessionGeometryReady::default();
        }
        let artwork_timer = profiler
            .as_deref()
            .map(|profiler| profiler.time(RuntimeStage::ActorArtworkSetup));
        let resources = crate::prepared_actor_artwork::session_resources(
            &artwork,
            pack.as_ref(),
            prepared.as_deref(),
        );
        drop(artwork_timer);
        // Always republished: presentation selects from these pages, the scene validates them.
        let (effective, staged, locations) = apply_session_pack(
            &mut scene,
            &resources,
            pack.as_deref(),
            icons,
            pack_geometry_ready,
            equipment.as_deref_mut(),
            profiler.as_deref(),
        );
        *session_artwork = effective;
        if let Some(equipment) = equipment.as_deref_mut() {
            equipment.set_session_items(items.as_deref(), staged, locations);
        }
    }
    let artwork = session_artwork.as_ref().unwrap_or(&artwork);
    let step = early.unwrap_or_else(|| actor_clock.advance(time.delta()));
    partial_tick.0 = step.partial_tick;
    skin_rigs.begin_frame();
    poses.begin_frame();
    coverage_poses.begin_frame();
    java_poses.begin_frame();
    if let Some(equipment) = equipment.as_deref_mut() {
        equipment.begin_frame();
    }
    layer_poses.begin_frame();
    let first_person = settings.perspective() == semantic_input::PerspectiveMode::FirstPerson;
    let first_person = server_camera.as_deref().map_or(first_person, |camera| {
        camera.renders_first_person(first_person)
    });
    let java_mode = settings.feel().java_animations;
    // Java selects poses and items; the shared combat profile always owns bob, hurt and glint.
    let combat_visuals = render_api::DEFAULT_COMBAT_VISUALS;
    let mut local_feed = input.local_feed.take();
    if let Some(feed) = local_feed.as_mut() {
        feed.first_person = first_person;
        feed.main_hand = input
            .local_equipment
            .main
            .as_ref()
            .map(|item| item.identifier.clone());
        feed.off_hand = input
            .local_equipment
            .off
            .as_ref()
            .map(|item| item.identifier.clone());
        feed.main_hand_metadata = input
            .local_equipment
            .main
            .as_ref()
            .map_or(0, |item| item.damage.unwrap_or(item.metadata));
        feed.held_variants = [&input.local_equipment.main, &input.local_equipment.off]
            .map(|item| item.as_ref().map(|item| item.variant).unwrap_or_default());
    }
    if let Some(stream) = client_world.stream.as_mut() {
        if let Some(equipment) = equipment.as_deref() {
            stream.set_item_use_overlays(equipment.item_use_overlays());
            stream.set_item_use_durations(equipment.item_use_durations());
            stream.set_item_attack_timings(equipment.item_attack_timings());
        }
        // Feed the client-authored local pose before the tick advance and rig read so the
        // local body/hand are driven by the shared rig, not the static fallback.
        if let Some(feed) = &mut local_feed
            && let Some(timing) = hand_use(stream, step.partial_tick).1
            && timing.max_use_ticks > 0
        {
            feed.item_use = client_world::LocalItemUse::Predicted {
                elapsed_ticks: timing.use_elapsed_ticks,
                max_ticks: timing.max_use_ticks,
                charged: timing.hand_charged,
            };
        }
        // A seat that locks rotation faces the rider's body along the vehicle.
        if let Some(feed) = &mut local_feed
            && let Some(lock) = stream.authority().local_seat_view_lock(step.partial_tick)
        {
            feed.yaw = lock.facing();
        }
        if let Some(feed) = &local_feed {
            stream.sync_local_player_pose(feed);
        }
        stream.sync_local_vehicle_pose(input.local_vehicle);
        // Attacks, mining and use swing the local arm at once; the server echoes no swing.
        if let Some(progress) = input.swing_progress {
            stream.sync_local_swing(progress);
        }
        stream.set_actor_camera_rotation(views::actor_camera_rotation(view.camera_rotation()));
        if let Ok((transform, _)) = camera.single() {
            stream.set_actor_camera_position(transform.translation.to_array());
        }
        stream.set_actor_animation_view(
            camera
                .single()
                .ok()
                .and_then(|(transform, projection)| views::animation_view(transform, projection)),
        );
        let _animation = profiler
            .as_deref()
            .map(|profiler| profiler.time(render_scene::RuntimeStage::ActorAnimation));
        // Liquid contact is tick state; bed headings are resolved before camera publication.
        if step.ticks > 0 {
            sample_world(stream);
        }
        stream.advance_actor_interpolation_frame(step.ticks);
    }
    let authoritative_subject_eye = authoritative_local_actor_eye(
        input.predicted_eye,
        client_world
            .stream
            .as_ref()
            .map(|stream| stream.resolved_server_position().position),
    );
    let authoritative_subject_feet = input
        .predicted_feet
        .map(bevy::prelude::Vec3::from_array)
        .or_else(|| {
            client_world.stream.as_ref().map(|stream| {
                let mut feet =
                    bevy::prelude::Vec3::from_array(stream.resolved_server_position().position);
                feet.y -= protocol::PLAYER_NETWORK_OFFSET;
                feet
            })
        });
    publish_local_actor_visibility(
        &avatar,
        settings.perspective(),
        server_camera.as_deref(),
        authoritative_subject_eye,
        authoritative_subject_feet,
        view.rotation(),
        &mut local_visibility,
    );

    let aspect = match camera.single() {
        Ok((_, Projection::Perspective(perspective))) => perspective.aspect_ratio,
        _ => 1.0,
    };
    let sampling_camera = camera.single().ok().map(|(transform, _)| {
        (
            views::actor_camera_rotation(view.camera_rotation()),
            transform.translation.to_array(),
        )
    });
    input.local_feed = local_feed;
    let mut geometries = Vec::new();
    let hand_visible = first_person && input.renders_game && !input.hide_hand;
    let canonical_local = hand_visible
        .then(|| {
            client_world.stream.as_ref().and_then(|stream| {
                let runtime_id = stream.local_player_runtime_id();
                let rig = stream.authority().actor_rig(runtime_id)?;
                let actor = stream.authority().actor(runtime_id)?;
                let profile = stream.authority().actor_player_profile(runtime_id);
                let mut presentation = crate::presentation::actors::actor_rig_presentation_cached(
                    &rig,
                    actor,
                    profile,
                    step.partial_tick,
                    poses,
                )?;
                presentation.submission.overlay_rgba8 =
                    crate::presentation::combat::overlay(actor, combat_visuals);
                register_player_skin(
                    &mut presentation,
                    &rig,
                    skin_rigs,
                    equipment.as_deref_mut(),
                    &mut geometries,
                );
                java_hand.remember(&rig, input.local_equipment.main.as_ref());
                Some(presentation)
            })
        })
        .flatten();
    *hand_source = if hand_visible {
        canonical_local.and_then(|presentation| {
            let stream = client_world.stream.as_ref()?;
            let equipment = equipment.as_deref_mut()?;
            let equipment_input = local_equipment(
                stream,
                stream.local_player_runtime_id(),
                &input.local_equipment,
            );
            let (consume_ticks, item_animation) = hand_use(stream, step.partial_tick);
            let item_animation =
                item_animation.map(|input| actor_animation::AttachableAnimationInput {
                    delta_seconds: Some(time.delta_secs()),
                    ..input
                });
            *captured_hand_use = (consume_ticks, item_animation);
            *hand_key = hand::source_key(stream, consume_ticks, item_animation, step.partial_tick);
            equipment.begin_hand_readiness();
            let source = hand::source(
                hand::HandInputs {
                    stream,
                    presentation,
                    equipment_input: &equipment_input,
                    owner_equipment: &equipment_input,
                    consume_ticks,
                    item_animation,
                    alpha: step.partial_tick,
                    artwork,
                    sampling_camera,
                    aspect,
                    motion: hand_motion
                        .as_deref()
                        .map_or(Mat4::IDENTITY, hand_motion_matrix),
                },
                java_mode,
                equipment,
                java_hand,
            );
            equipment.end_hand_readiness();
            source
        })
    } else {
        *hand_key = None;
        None
    };
    if let Some(equipment) = equipment.as_deref_mut() {
        geometries.extend(equipment.take_pending_geometries());
    }
    register_geometries(&mut hand_builder.0, &mut scene, geometries, None);
    let fov = camera
        .single()
        .ok()
        .and_then(|(_, projection)| crate::camera::first_person_hand_fov(projection));
    *hand_ready = hand::is_ready(&hand_builder.0, hand_source.as_ref(), fov);
    *captured_view = *view;
    *captured_sampling_camera = sampling_camera;
    *captured_aspect = aspect;
    *captured_first_person = first_person;
    *captured_java_mode = java_mode;
    *captured_combat_visuals = combat_visuals;
    *captured_step = Some(step);
    *captured_input = Some(input);
}

/// Registers the player's own model so hand readiness and final body use the same geometry.
pub(super) fn register_player_skin(
    presentation: &mut ActorRigPresentation,
    rig: &actor_animation::ActorRigSnapshot<'_>,
    cache: &mut crate::presentation::skin_rig::SkinRigCache,
    mut equipment: Option<&mut EquipmentRuntime>,
    geometries: &mut Vec<render_model::ActorRigGeometry>,
) {
    let Some(geometry) = rig.skin_geometry else {
        return;
    };
    let id = cache.rig(geometry, rig.skin_mesh, |built| {
        if let Some(equipment) = equipment.as_deref_mut() {
            equipment.register_skin_rig(
                built.id,
                geometry
                    .bones
                    .iter()
                    .map(|bone| bone.name.clone())
                    .collect(),
            );
        }
        geometries.push(built);
    });
    match id {
        Some(id) => presentation.submission.input.rig = id,
        None => presentation.submission.route = render_model::ActorRigRoute::NoDraw,
    }
}
