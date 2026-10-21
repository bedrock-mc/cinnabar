//! Captures post-send observations and finalizes poses after local interaction admission.
#[cfg(not(target_arch = "wasm32"))]
mod shared_players;
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
    crate::runtime::{shutdown::FatalError, world::ClientWorld},
    client_interaction::movement::{
        LocalPhysicsController, MovementTicker, PhysicsCollisionRegistries,
    },
    client_state::PlayerRuntime,
};

use launcher::menu::settings_options::SettingId;
/// Projects local damage only after actor ticking, clearing it when the session leaves gameplay.
pub(crate) fn publish_local_actor_damage(
    world: Res<ClientWorld>,
    fatal_error: Res<FatalError>,
    mut ui: Option<ResMut<UiRuntimeResource>>,
) {
    let Some(ui) = ui.as_deref_mut() else {
        return;
    };
    let actor = world
        .stream
        .as_ref()
        .filter(|_| fatal_error.0.is_none() && world.transfer_notice.is_none())
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
    #[cfg(not(target_arch = "wasm32"))]
    menu: Option<Res<'w, client_shell::menu::MenuRuntime>>,
    #[cfg(target_arch = "wasm32")]
    options: Res<'w, crate::browser_input::BrowserOptions>,
    ui_presentation: Option<Res<'w, UiPresentationResource>>,
    collisions: Option<Res<'w, PhysicsCollisionRegistries>>,
    item_use: Option<Res<'w, client_interaction::item_use::ItemUseRuntime>>,
    input: Option<Res<'w, client_state::SemanticInputSnapshot>>,
    movement: Option<Res<'w, MovementTicker>>,
    time: Res<'w, Time<Real>>,
    profiler: Option<Res<'w, render_scene::RuntimeStageProfiler>>,
    #[cfg(not(target_arch = "wasm32"))]
    account: Option<Res<'w, client_shell::menu::launcher_account::LauncherAccount>>,
}

/// Predicts remote actors from elapsed frame time, independently of local transport pauses.
pub(crate) fn advance_actor_motion(
    mut world: ResMut<ClientWorld>,
    mut state: ResMut<client_presentation::actor_publication::ActorFrameState>,
    time: Res<Time<Real>>,
) {
    client_presentation::actor_publication::advance_actor_motion(
        &mut state,
        world.stream.as_mut(),
        time.delta().min(simulation_clock::MAX_FRAME_ELAPSED),
    );
}

/// Captures live owners and evaluates actor visuals after this frame's sends.
pub(crate) fn advance_actor_frame(
    observations: ActorObservations,
    params: client_presentation::actor_publication::ActorFramePublication,
    mut java_blocking: Local<bool>,
    #[cfg(not(target_arch = "wasm32"))] mut shared_state: Local<shared_players::State>,
    mut fluids: Local<client_presentation::actor_sampling::ActorFluidSampler>,
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
        #[cfg(not(target_arch = "wasm32"))]
        menu,
        #[cfg(target_arch = "wasm32")]
        options,
        ui_presentation,
        collisions,
        item_use,
        input,
        movement,
        time,
        profiler,
        #[cfg(not(target_arch = "wasm32"))]
        account,
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
    let (predicted_eye, predicted_feet) = view.actor_positions(
        physics.render_eye_position(),
        physics.render_feet_position(),
    );
    #[cfg(not(target_arch = "wasm32"))]
    let shared = shared_players::exchange(
        stream,
        ui.as_deref().map(|ui| &ui.0),
        &skin.cosmetics,
        account.as_deref().map(|account| &account.0),
        &time,
        &mut shared_state,
    );
    let input =
        ActorFrameInput {
            cosmetics: skin.cosmetics.clone(),
            local_feed,
            predicted_eye,
            predicted_feet,
            local_equipment,
            swing_progress: None,
            // Resolving the screen policy is UI work, attributed as such inside actor publication.
            renders_game: {
                let _ui = profiler
                    .as_deref()
                    .map(|profiler| profiler.time(render_scene::RuntimeStage::UiPreparation));
                #[cfg(not(target_arch = "wasm32"))]
                let renders_game = client_shell::screen_policy::renders_game(
                    &player,
                    ui.as_deref().map(|ui| &ui.0),
                    menu.as_deref(),
                    ui_presentation
                        .as_deref()
                        .map(|presentation| &presentation.0),
                );
                #[cfg(target_arch = "wasm32")]
                let renders_game = ui.as_deref().zip(ui_presentation.as_deref()).is_none_or(
                    |(ui, presentation)| presentation.renders_game_behind(&player, &ui.0, None),
                );
                renders_game
            },
            #[cfg(not(target_arch = "wasm32"))]
            remote_emotes: shared.emotes,
            #[cfg(not(target_arch = "wasm32"))]
            remote_hats: shared.hats,
            #[cfg(target_arch = "wasm32")]
            remote_emotes: Vec::new(),
            #[cfg(target_arch = "wasm32")]
            remote_hats: Vec::new(),
            custom_emote: ui
                .as_deref()
                .and_then(|ui| ui.emotes().playback())
                .map(|playback| {
                    let now = u64::try_from(time.elapsed().as_millis()).unwrap_or(u64::MAX);
                    (playback.emote, playback.elapsed(now))
                }),
            hide_hand: {
                #[cfg(not(target_arch = "wasm32"))]
                let options = menu.as_ref().map(|menu| menu.settings_snapshot().0);
                #[cfg(target_arch = "wasm32")]
                let options = Some(std::sync::Arc::clone(&options.0));
                options.is_some_and(|settings| {
                    !client_presentation::presentation::visibility::GameplayOverlayVisibility::new(
                        settings.value(SettingId::HideHud) != 0,
                        settings.value(SettingId::HideHand) != 0,
                        player.facts.player_game_mode()
                            == Some(protocol::PlayerGameMode::Spectator),
                    )
                    .hand
                })
            },
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
                fluids.sample(stream, collisions);
            }
        },
        |stream, _partial_tick| {
            let consume = ui.as_deref().and_then(|_| {
                client_interaction::item_use::selection::consume_ticks(&player, stream)
            });
            // The use is timed in local physics ticks, so its frame fraction is that clock's too.
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
                        physics.tick_alpha(),
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
/// Lets the actor geometry catalog grow to this device's memory-tier budget, within what one
/// storage binding of its GPU holds.
pub(crate) fn initialize_actor_geometry_budget(
    device: Option<Res<bevy::render::renderer::RenderDevice>>,
    memory: Option<bevy::prelude::Res<crate::host_memory::HostMemory>>,
    scene: Option<ResMut<render_scene::ActorRenderScene>>,
) {
    let (Some(device), Some(mut scene)) = (device, scene) else {
        return;
    };
    scene.set_geometry_budget(
        resource_pack::subpacks::device_memory_tier(memory.map_or_else(
            || crate::host_memory::HostMemory::default().0,
            |memory| memory.0,
        )),
        render::actor_storage_binding_bytes(&device),
    );
}

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
    let server_swings = world
        .stream
        .as_mut()
        .map_or(0, |stream| stream.take_local_server_swings());
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
        // Admission for the next tick uses each animation's own effect-aware restart guard.
        if server_swings > 0 {
            swings.try_swing_before_tick_for_item(
                movement.completed_tick().saturating_add(1),
                effects
                    .as_deref()
                    .map_or_default(|effects| effects.mining_effects()),
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
pub(super) mod custom_emotes;

#[cfg(test)]
#[path = "actor_publication/tests/item_swing.rs"]
mod item_swing;

#[cfg(test)]
#[path = "actor_publication/tests/damage.rs"]
mod damage;
