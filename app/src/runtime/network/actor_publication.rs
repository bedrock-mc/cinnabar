//! Captures gameplay observations at the existing pre-send presentation boundary.
use crate::{
    movement::{LocalPhysicsController, MovementTicker, PhysicsCollisionRegistries},
    player_runtime::PlayerRuntime,
    runtime::world::ClientWorld,
    ui_runtime::{UiRuntime, presentation::UiPresentationRuntime},
};
use bevy::{ecs::system::SystemParam, prelude::*};
use client_presentation::actor_publication::{ActorFrameInput, ActorWorld};
pub(crate) use client_presentation::actor_publication::{
    ActorFramePartialTick, HandRigBuilder, publish_actor_render_frame,
};

/// App-owned inputs borrowed only while this frame's presentation is prepared.
#[derive(SystemParam)]
pub(crate) struct ActorObservations<'w> {
    world: ResMut<'w, ClientWorld>,
    player: Res<'w, PlayerRuntime>,
    physics: Res<'w, LocalPhysicsController>,
    view: Res<'w, crate::local_player::LocalViewPose>,
    skin: Res<'w, crate::player_skin::LocalPlayerSkin>,
    settings: Res<'w, crate::camera::CameraSettingsAuthority>,
    swings: Option<ResMut<'w, crate::melee::SwingTracker>>,
    ui: Option<Res<'w, UiRuntime>>,
    menu: Option<Res<'w, crate::menu::MenuRuntime>>,
    ui_presentation: Option<Res<'w, UiPresentationRuntime>>,
    collisions: Option<Res<'w, PhysicsCollisionRegistries>>,
    item_use: Option<Res<'w, crate::item_use::ItemUseRuntime>>,
    movement: Option<Res<'w, MovementTicker>>,
    cave: Option<Res<'w, crate::runtime::visibility::CaveVisibilityCache>>,
    profiler: Option<Res<'w, render::RuntimeStageProfiler>>,
}

/// Samples live owners without changing the established prepare/send/publish order.
pub(crate) fn prepare_actor_render_frame(
    observations: ActorObservations,
    params: client_presentation::actor_publication::ActorFramePublication,
) {
    let ActorObservations {
        mut world,
        player,
        physics,
        view,
        skin,
        settings,
        mut swings,
        ui,
        menu,
        ui_presentation,
        collisions,
        item_use,
        movement,
        cave,
        profiler,
    } = observations;
    let _timer = profiler
        .as_deref()
        .map(|profiler| profiler.time(render::RuntimeStage::ActorPublication));
    let stream = world.stream.as_ref();
    let local_use = stream.zip(ui.as_deref()).zip(item_use.as_deref()).map_or(
        client_world::LocalItemUse::Unpredicted,
        |((stream, ui), item_use)| item_use.local_item_use(&player, stream, ui),
    );
    let input = ActorFrameInput {
        local_feed: client_presentation::actor_feed::build_local_player_feed(
            &*physics,
            view.rotation(),
            false,
            settings.feel().view_bobbing,
            skin.local_uuid,
            || skin.player_skin(),
            local_use,
        ),
        predicted_eye: physics.render_eye_position(),
        predicted_feet: physics.render_feet_position(),
        local_equipment: stream.map_or_else(Default::default, |stream| {
            client_presentation::presentation::equipment::local_input(
                &player,
                stream,
                ui.as_deref(),
                stream.local_player_runtime_id(),
            )
        }),
        // Consume only while a stream exists, as the prior publisher did.
        swing_started: stream.and_then(|_| {
            swings
                .as_deref_mut()
                .and_then(crate::melee::SwingTracker::take_started)
        }),
        renders_game: crate::screen_policy::renders_game(
            &player,
            ui.as_deref(),
            menu.as_deref(),
            ui_presentation.as_deref(),
        ),
        hide_hand: menu
            .as_ref()
            .is_some_and(|menu| menu.settings_snapshot().0.value("hide_hand") != 0),
    };
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
                .and_then(|ui| crate::item_use::consume_ticks(&player, stream, ui));
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
        |stream, low, high| {
            cave.as_deref().is_some_and(|cave| {
                cave.hides_box(
                    stream.current_dimension(),
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
