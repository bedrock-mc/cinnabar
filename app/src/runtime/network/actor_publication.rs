//! Captures post-send observations and finalizes poses after local interaction admission.
use crate::{
    movement::{LocalPhysicsController, MovementTicker, PhysicsCollisionRegistries},
    player_runtime::PlayerRuntime,
    runtime::world::ClientWorld,
};
use bevy::time::Real;
use bevy::{ecs::system::SystemParam, prelude::*};
use client_presentation::actor_publication::{ActorFrameInput, ActorWorld};
pub(crate) use client_presentation::actor_publication::{
    ActorFramePartialTick, HandRigBuilder, publish_actor_render_frame,
};
use client_ui::ui_runtime::{UiRuntime, presentation::UiPresentationRuntime};

/// Projects local damage only after actor ticking, clearing it when the session leaves gameplay.
pub(crate) fn publish_local_actor_damage(
    world: Res<ClientWorld>,
    mut ui: Option<ResMut<UiRuntime>>,
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
    view: Res<'w, crate::local_player::LocalViewPose>,
    skin: Res<'w, crate::player_skin::LocalPlayerSkin>,
    settings: Res<'w, crate::camera::CameraSettingsAuthority>,
    effects: Option<Res<'w, crate::movement::LocalMovementEffectTimeline>>,
    ui: Option<Res<'w, UiRuntime>>,
    menu: Option<Res<'w, crate::menu::MenuRuntime>>,
    ui_presentation: Option<Res<'w, UiPresentationRuntime>>,
    collisions: Option<Res<'w, PhysicsCollisionRegistries>>,
    item_use: Option<Res<'w, crate::item_use::ItemUseRuntime>>,
    input: Option<Res<'w, crate::semantic_controls::SemanticInputSnapshot>>,
    movement: Option<Res<'w, MovementTicker>>,
    time: Res<'w, Time<Real>>,
    profiler: Option<Res<'w, render::RuntimeStageProfiler>>,
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
        .map(|profiler| profiler.time(render::RuntimeStage::ActorPublication));
    let stream = world.stream.as_ref();
    let local_equipment = stream.map_or_else(Default::default, |stream| {
        client_presentation::presentation::equipment::local_input(
            &player,
            stream,
            ui.as_deref(),
            stream.local_player_runtime_id(),
        )
    });
    // Java blocks with a sword while use is held; Bedrock never flags that use.
    let java_sword = settings.feel().java_animations
        && local_equipment
            .main
            .as_ref()
            .is_some_and(|item| render_model::java_animation::is_java_sword(&item.identifier));
    let blocking = java_sword
        && input
            .as_deref()
            .is_some_and(|input| input.phase(semantic_input::Action::Use).held);
    let local_use = if blocking {
        client_world::LocalItemUse::Using
    } else {
        match stream.zip(ui.as_deref()).zip(item_use.as_deref()) {
            Some(((stream, ui), item_use)) => item_use.local_item_use(&player, stream, ui),
            None => client_world::LocalItemUse::Unpredicted,
        }
    };
    // Ending a block clears the use flag it raised, whatever the hand holds next.
    let local_use =
        if (java_sword || *java_blocking) && local_use == client_world::LocalItemUse::Unpredicted {
            client_world::LocalItemUse::Idle
        } else {
            local_use
        };
    *java_blocking = blocking;
    let mut local_feed = client_presentation::actor_feed::build_local_player_feed(
        &*physics,
        view.rotation(),
        false,
        settings.feel().view_bobbing,
        skin.local_uuid,
        || skin.player_skin(),
        local_use,
    );
    if let Some(feed) = &mut local_feed {
        #[cfg(feature = "developer-control")]
        {
            feed.prefer_client_skin = skin.recording_cape_enabled();
        }
        feed.main_hand_slot = player.selected_hotbar_slot().unwrap_or(0);
        feed.main_hand_stack_id = player
            .selected_stack()
            .map(|stack| stack.stack_network_id)
            .filter(|id| *id > 0);
        let mining_effects = effects
            .as_deref()
            .map_or_else(Default::default, |effects| effects.mining_effects());
        feed.bedrock_swing_ticks = gameplay::melee::swing_duration(mining_effects);
        feed.java_swing_ticks = gameplay::melee::java_swing_duration(mining_effects);
    }
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
                .map(|profiler| profiler.time(render::RuntimeStage::UiPreparation));
            crate::screen_policy::renders_game(
                &player,
                ui.as_deref(),
                menu.as_deref(),
                ui_presentation.as_deref(),
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
            let consume = ui
                .as_deref()
                .and_then(|_| crate::item_use::consume_ticks(&player, stream));
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
    player: Option<Res<'w, crate::player_runtime::PlayerRuntime>>,
    physics: Res<'w, LocalPhysicsController>,
    effects: Option<Res<'w, crate::movement::LocalMovementEffectTimeline>>,
    swings: Option<ResMut<'w, crate::melee::SwingTracker>>,
    movement: Option<Res<'w, MovementTicker>>,
    collisions: Option<Res<'w, PhysicsCollisionRegistries>>,
    cave: Option<Res<'w, crate::runtime::visibility::CaveVisibilityCache>>,
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
                Some(client_world::LocalSwingMotionSample {
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
        |stream, camera, low, high| {
            cave.as_deref().is_some_and(|cave| {
                cave.hides_box(
                    crate::runtime::telemetry::camera_sub_chunk_key(
                        stream.current_dimension(),
                        Vec3::from_array(camera),
                    ),
                    stream.connectivity_generation(),
                    |key| stream.has_sub_chunk_connectivity(key),
                    low,
                    high,
                )
            })
        },
        params,
    );
}

/// Publishes entity-shadow casters for the bodies this frame drew.
#[allow(clippy::too_many_arguments)]
pub(crate) fn publish_entity_shadows(
    world: Res<ClientWorld>,
    player: Res<PlayerRuntime>,
    partial_tick: Res<ActorFramePartialTick>,
    local: Res<crate::local_player::LocalAvatarVisibilityCarrier>,
    camera: Query<(&Transform, &Projection), With<crate::camera::FlyCamera>>,
    frame: Res<render::ActorRenderFrame>,
    mut drawn: Local<Vec<u64>>,
    mut staging: Local<Vec<render_model::EntityShadow>>,
    scene: Option<ResMut<render::EntityShadowScene>>,
) {
    let Some(mut scene) = scene else {
        return;
    };
    let stream = world.stream.as_ref();
    let local = stream.map(|stream| {
        let runtime_id = stream.local_player_runtime_id();
        client_presentation::entity_shadows::LocalShadowSource {
            runtime_id,
            visible: local.snapshot().is_some_and(|visibility| {
                visibility.runtime_id() == runtime_id && visibility.visible()
            }),
            feet: local
                .snapshot()
                .filter(|visibility| visibility.runtime_id() == runtime_id)
                .map(|visibility| visibility.feet().to_array()),
            spectator: player
                .facts
                .game_mode_capabilities()
                .is_some_and(|caps| !caps.visible),
        }
    });
    let view = camera
        .single()
        .ok()
        .map(|(transform, projection)| render::ActorCullView {
            clip_from_world: projection.get_clip_from_view() * transform.to_matrix().inverse(),
            camera_position: transform.translation,
            max_distance: render::MAX_ACTOR_RENDER_DISTANCE_BLOCKS,
        });
    client_presentation::entity_shadows::drawn_bodies(&frame, &mut drawn);
    client_presentation::entity_shadows::publish_entity_shadows(
        stream,
        partial_tick.0,
        local,
        view,
        &drawn,
        &mut staging,
        &mut scene,
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
