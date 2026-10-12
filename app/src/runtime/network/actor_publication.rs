//! Captures post-send observations and finalizes poses after local interaction admission.
#[cfg(test)]
use bevy::prelude::IntoScheduleConfigs;
#[cfg(test)]
use bevy::prelude::{App, Mut, Resource, Update};
use bevy::time::Real;
use bevy::{
    ecs::system::SystemParam,
    prelude::{Local, Projection, Query, Res, ResMut, Time, Transform, With},
};
use client_presentation::actor_publication::ActorFramePartialTick;
use client_presentation::actor_publication::{ActorFrameInput, ActorWorld};
use client_shell::ui_resources::{UiPresentationResource, UiRuntimeResource};
use {
    crate::runtime::world::ClientWorld,
    client_interaction::movement::{
        LocalPhysicsController, MovementTicker, PhysicsCollisionRegistries,
    },
    client_state::PlayerRuntime,
};

/// Projects local damage only after actor ticking, clearing it when the session leaves gameplay.
pub(crate) fn publish_local_actor_damage(
    world: Res<ClientWorld>,
    mut ui: Option<ResMut<UiRuntimeResource>>,
) {
    let Some(ui) = ui.as_deref_mut() else {
        return;
    };
    let actor = world
        .stream
        .as_ref()
        .filter(|_| world.fatal_error.is_none() && world.transfer_notice.is_none())
        .and_then(|stream| stream.authority().actor(stream.local_player_runtime_id()));
    ui.publish_local_actor_health(actor);
}

/// App-owned inputs borrowed only while this frame's presentation is prepared.
#[derive(SystemParam)]
pub(crate) struct ActorObservations<'w> {
    world: ResMut<'w, ClientWorld>,
    player: Res<'w, PlayerRuntime>,
    physics: Res<'w, LocalPhysicsController>,
    view: Res<'w, client_presentation::local_player::LocalViewPose>,
    skin: Res<'w, client_shell::player_skin::LocalPlayerSkin>,
    settings: Res<'w, client_presentation::camera::CameraSettingsAuthority>,
    effects: Option<Res<'w, client_interaction::movement::LocalMovementEffectTimeline>>,
    ui: Option<Res<'w, UiRuntimeResource>>,
    menu: Option<Res<'w, client_shell::menu::MenuRuntime>>,
    ui_presentation: Option<Res<'w, UiPresentationResource>>,
    collisions: Option<Res<'w, PhysicsCollisionRegistries>>,
    item_use: Option<Res<'w, client_interaction::item_use::ItemUseRuntime>>,
    input: Option<Res<'w, client_state::SemanticInputSnapshot>>,
    movement: Option<Res<'w, MovementTicker>>,
    time: Res<'w, Time<Real>>,
    profiler: Option<Res<'w, render_scene::RuntimeStageProfiler>>,
}

/// Predicts remote actors at this frame's tick positions before interaction picks them.
pub(crate) fn advance_actor_motion(
    mut world: ResMut<ClientWorld>,
    mut state: ResMut<client_presentation::actor_publication::ActorFrameState>,
    time: Res<Time<Real>>,
) {
    client_presentation::actor_publication::advance_actor_motion(
        &mut state,
        world.stream.as_mut(),
        time.delta(),
    );
}

/// Captures live owners and evaluates actor visuals after this frame's sends.
pub(crate) fn advance_actor_frame(
    observations: ActorObservations,
    params: client_presentation::actor_publication::ActorFramePublication,
    mut java_blocking: Local<bool>,
) {
    let ActorObservations {
        mut world,
        player,
        physics,
        view,
        skin,
        settings,
        effects,
        ui,
        menu,
        ui_presentation,
        collisions,
        item_use,
        input,
        movement,
        time,
        profiler,
    } = observations;
    let _timer = profiler
        .as_deref()
        .map(|profiler| profiler.time(render_scene::RuntimeStage::ActorPublication));
    let stream = world.stream.as_ref();
    let local_equipment = stream.map_or_else(Default::default, |stream| {
        client_presentation::presentation::equipment::local_input(
            &player,
            stream,
            ui.as_deref().map(|ui| &ui.0),
            stream.local_player_runtime_id(),
        )
    });
    let mining_effects = effects
        .as_deref()
        .map_or_else(Default::default, |effects| effects.mining_effects());
    #[cfg(feature = "developer-control")]
    let prefer_client_skin = skin.recording_cape_enabled();
    #[cfg(not(feature = "developer-control"))]
    let prefer_client_skin = false;
    let local_feed = client_presentation::actor_feed::prepare_local_player_feed(
        &*physics,
        &view,
        &settings,
        &local_equipment,
        &player,
        input
            .as_deref()
            .is_some_and(|input| input.phase(semantic_input::Action::Use).held),
        || match stream.zip(ui.as_deref()).zip(item_use.as_deref()) {
            Some(((stream, ui), item_use)) => item_use.local_item_use(&player, stream, ui),
            None => client_world::LocalItemUse::Unpredicted,
        },
        &mut java_blocking,
        skin.local_uuid,
        || skin.player_skin(),
        prefer_client_skin,
        (
            gameplay::melee::swing_duration(mining_effects),
            gameplay::melee::java_swing_duration(mining_effects),
        ),
    );
    let input = ActorFrameInput {
        local_feed,
        predicted_eye: physics.render_eye_position(),
        predicted_feet: physics.render_feet_position(),
        local_equipment,
        swing_progress: None,
        // Resolving the screen policy is UI work, attributed as such inside actor publication.
        renders_game: {
            let _ui = profiler
                .as_deref()
                .map(|profiler| profiler.time(render_scene::RuntimeStage::UiPreparation));
            client_shell::screen_policy::renders_game(
                &player,
                ui.as_deref().map(|ui| &ui.0),
                menu.as_deref(),
                ui_presentation
                    .as_deref()
                    .map(|presentation| &presentation.0),
            )
        },
        custom_emote: ui
            .as_deref()
            .and_then(|ui| ui.emotes().playback())
            .map(|playback| {
                let now = u64::try_from(time.elapsed().as_millis()).unwrap_or(u64::MAX);
                (playback.emote, playback.elapsed(now))
            }),
        hide_hand: menu.as_ref().is_some_and(|menu| {
            let settings = menu.settings_snapshot().0;
            !client_presentation::presentation::visibility::GameplayOverlayVisibility::new(
                settings.value("hide_hud") != 0,
                settings.value("hide_hand") != 0,
                player.facts.player_game_mode() == Some(protocol::PlayerGameMode::Spectator),
            )
            .hand
        }),
    };
    if let Some(stream) = world.stream.as_mut() {
        stream.set_local_motion_authority(
            movement
                .as_deref()
                .filter(|movement| movement.physics_is_authorized())
                .map(|movement| movement.interaction_authority_identity()),
        );
    }
    let ClientWorld {
        stream,
        entity_assets,
        pack_entities,
        session_items,
        prepared_actor_artwork,
        ..
    } = &mut *world;
    client_presentation::actor_publication::advance_actor_frame(
        ActorWorld {
            stream: stream.as_mut(),
            collisions: collisions
                .as_deref()
                .map(|value| value as &dyn client_presentation::observations::CollisionLookup),
            entity_assets: entity_assets.as_deref(),
            pack_entities: pack_entities.clone(),
            session_items: session_items.clone(),
            prepared_actor_artwork,
        },
        input,
        |stream| {
            if let Some(collisions) = collisions.as_deref() {
                client_presentation::actor_sampling::sample_actor_world_state(stream, collisions);
            }
        },
        |stream, partial_tick| {
            let consume = ui.as_deref().and_then(|_| {
                client_interaction::item_use::selection::consume_ticks(&player, stream)
            });
            let animation = ui
                .as_deref()
                .zip(item_use.as_deref())
                .map(|(ui, use_runtime)| {
                    use_runtime.render_input(
                        &player,
                        stream,
                        ui,
                        movement
                            .as_deref()
                            .map_or(0, |movement| movement.completed_tick()),
                        partial_tick,
                    )
                });
            (consume, animation)
        },
        params,
    );
}

/// Gameplay clocks borrowed only after the interaction owners have admitted this frame's actions.
#[derive(SystemParam)]
pub(crate) struct ActorFinalObservations<'w> {
    world: ResMut<'w, ClientWorld>,
    player: Option<Res<'w, client_state::PlayerRuntime>>,
    physics: Res<'w, LocalPhysicsController>,
    effects: Option<Res<'w, client_interaction::movement::LocalMovementEffectTimeline>>,
    swings: Option<ResMut<'w, client_interaction::melee::SwingTracker>>,
    movement: Option<Res<'w, MovementTicker>>,
    collisions: Option<Res<'w, PhysicsCollisionRegistries>>,
    cave: Option<Res<'w, crate::runtime::visibility::CaveVisibilityCache>>,
    icons: Option<Res<'w, UiPresentationResource>>,
}

/// Consumes admitted local ticks and builds their final poses with the pre-send capture.
pub(crate) fn prepare_actor_render_frame(
    observations: ActorFinalObservations,
    params: client_presentation::actor_publication::ActorFramePublication,
) {
    let ActorFinalObservations {
        mut world,
        player,
        physics,
        effects,
        mut swings,
        movement,
        collisions,
        cave,
        icons,
    } = observations;
    let item_swing_ticks = player
        .as_deref()
        .and_then(|player| crate::melee::selected_attack_timing(player, &world))
        .and_then(|timing| timing.swing_duration_ticks);
    let stream = world.stream.as_ref();
    let swing_progress = stream.and_then(|_| {
        let movement = movement.as_deref()?;
        let swings = swings.as_deref_mut()?;
        if let Some(effects) = effects.as_deref() {
            swings.sync_ticks_for_item(
                movement.interaction_authority_identity(),
                movement.completed_tick(),
                effects,
                item_swing_ticks,
            );
        }
        let mut progress = swings.published_progress(movement.completed_tick());
        progress.frame_alpha = Some(physics.tick_alpha());
        Some(progress)
    });
    if let (Some(stream), Some(movement), Some(swings)) = (
        world.stream.as_mut(),
        movement
            .as_deref()
            .filter(|movement| movement.physics_is_authorized()),
        swings.as_deref(),
    ) {
        let alpha = physics.tick_alpha();
        let samples = swings
            .committed_samples()
            .filter_map(|(tick, mut progress)| {
                let sample = physics.sample_at(tick)?;
                progress.frame_alpha = Some(alpha);
                Some(actor_animation::LocalSwingMotionSample {
                    tick,
                    delta: sample.movement,
                    yaw: sample.yaw,
                    progress,
                })
            });
        stream.sync_local_swing_motion(movement.interaction_authority_identity(), samples);
    }
    let ClientWorld {
        stream,
        entity_assets,
        pack_entities,
        session_items,
        prepared_actor_artwork,
        ..
    } = &mut *world;
    let occlusion_budget = std::cell::RefCell::new(meshing::occlusion::OcclusionBudget::default());
    let projection_safe = std::cell::Cell::new(None);
    client_presentation::actor_publication::prepare_actor_render_frame(
        ActorWorld {
            stream: stream.as_mut(),
            collisions: collisions
                .as_deref()
                .map(|value| value as &dyn client_presentation::observations::CollisionLookup),
            entity_assets: entity_assets.as_deref(),
            pack_entities: pack_entities.clone(),
            session_items: session_items.clone(),
            prepared_actor_artwork,
        },
        swing_progress,
        |stream, view, low, high| {
            let _zone = bevy::log::info_span!("actor.occlusion").entered();
            let camera = view.camera_position.to_array();
            cave.as_deref().is_some_and(|cave| {
                cave.hides_box(
                    crate::runtime::telemetry::camera_sub_chunk_key(
                        stream.current_dimension(),
                        view.camera_position,
                    ),
                    stream.connectivity_generation(),
                    |key| stream.has_sub_chunk_connectivity(key),
                    low,
                    high,
                ) || (projection_safe.get().unwrap_or_else(|| {
                    let safe = render_model::visibility_bounds::perspective_supports_occlusion(
                        view.clip_from_world,
                        camera,
                        f64::from(meshing::occlusion::OCCLUSION_EYE_GAP_BLOCKS),
                    );
                    projection_safe.set(Some(safe));
                    safe
                }) && stream.terrain_hides_box(
                    &mut occlusion_budget.borrow_mut(),
                    camera,
                    low,
                    high,
                    |_, key| cave.is_visible(key) && cave.rendered.contains_key(&key),
                ))
            })
        },
        icons.as_deref().map(|icons| &icons.0),
        params,
    );
    bevy::log::trace!(
        probes =
            meshing::occlusion::ENTITY_OCCLUSION_PROBES - occlusion_budget.borrow().remaining(),
        "actor occlusion work"
    );
}

/// Publishes entity-shadow casters for the bodies this frame drew.
#[allow(clippy::too_many_arguments)]
pub(crate) fn publish_entity_shadows(
    world: Res<ClientWorld>,
    player: Res<PlayerRuntime>,
    partial_tick: Res<ActorFramePartialTick>,
    local: Res<client_presentation::local_player::LocalAvatarVisibilityCarrier>,
    camera: Query<(&Transform, &Projection), With<client_presentation::camera::FlyCamera>>,
    frame: Res<render_scene::ActorRenderFrame>,
    drawn: Local<Vec<u64>>,
    staging: Local<Vec<render_model::EntityShadow>>,
    scene: Option<ResMut<render_scene::EntityShadowScene>>,
) {
    client_presentation::entity_shadows::drive_entity_shadows(
        world.stream.as_ref(),
        &player,
        partial_tick,
        local,
        camera,
        frame,
        drawn,
        staging,
        scene,
    );
}

#[cfg(test)]
#[path = "actor_publication/tests/custom_emotes.rs"]
mod custom_emotes;

#[cfg(test)]
#[path = "actor_publication/tests/item_swing.rs"]
mod item_swing;

#[cfg(test)]
#[path = "actor_publication/tests/damage.rs"]
mod damage;
