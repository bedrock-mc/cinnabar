//! Publishes the current gameplay pick to the native outline/highlight overlay passes.
use bevy::{ecs::system::SystemParam, prelude::*};
use gameplay::melee::{Crosshair, classify_block_hit, pick_actor};
use render_model::{BlockSelectionTarget, CrackShape, crack_shape_for_block};
use sim::PaletteWorld;
use simulation_clock::identity::SessionGeneration;

use client_shell::ui_resources::UiRuntimeResource;
use {
    crate::{
        interaction_authority::ray_is_current,
        runtime::{shutdown::FatalError, world::ClientWorld},
    },
    client_interaction::movement::PhysicsCollisionRegistries,
    client_presentation::local_player::InteractionOriginSnapshot,
    client_state::{ClientFrameSet, RuntimeSettings, SemanticInputSnapshot},
    gameplay::mining::{creative_reach, protocol_input_mode, survival_reach},
};

/// Actor selected by this frame's eye-origin gameplay ray after block occlusion.
#[derive(Resource, Default)]
pub(crate) struct CrosshairActor(pub(crate) Option<u64>);

#[derive(SystemParam)]
pub(crate) struct SelectionContext<'w> {
    player: Res<'w, client_state::PlayerRuntime>,
    world: Res<'w, ClientWorld>,
    fatal_error: Res<'w, FatalError>,
    collisions: Res<'w, PhysicsCollisionRegistries>,
    ui: Res<'w, UiRuntimeResource>,
    focus: Res<'w, client_state::ScreenFocus>,
    origin: Res<'w, InteractionOriginSnapshot>,
    input: Res<'w, SemanticInputSnapshot>,
    settings: Res<'w, RuntimeSettings>,
}

/// Updates after the completed pose and interaction authorities have been published.
pub(crate) fn configure(app: &mut App) {
    app.init_resource::<render_scene::BlockSelectionFrameResource>()
        .init_resource::<CrosshairActor>()
        .add_systems(Update, publish.in_set(ClientFrameSet::WorldPublication));
}

/// A missing, stale or menu-owned ray clears last frame's target immediately.
pub(crate) fn publish(
    context: SelectionContext,
    mut frame: ResMut<render_scene::BlockSelectionFrameResource>,
    mut picked: ResMut<CrosshairActor>,
) {
    let (target, actor) = target(&context.player, &context).unwrap_or_default();
    picked.0 = actor;
    frame.update(
        target.as_ref(),
        context
            .settings
            .user_settings_update()
            .1
            .video
            .outline_selection,
    );
}

/// Resolves the block outline or actor selected by the shared gameplay pick.
/// Model highlighting uses its separate surface geometry.
fn target(
    player_runtime: &client_state::PlayerRuntime,
    context: &SelectionContext,
) -> Option<(Option<BlockSelectionTarget>, Option<u64>)> {
    if context.focus.menu_visible || context.focus.ui_focused || context.fatal_error.0.is_some() {
        return None;
    }
    let stream = context.world.stream.as_ref()?;
    let ray = context.origin.outbound_ray()?;
    if !ray_is_current(ray, SessionGeneration::new(context.ui.session_id()), stream)
        || player_runtime.facts.player_game_mode() == Some(protocol::PlayerGameMode::Spectator)
    {
        return None;
    }
    let mode = protocol_input_mode(context.input.snapshot()?.input_mode);
    let reach = if player_runtime
        .facts
        .game_mode_capabilities()?
        .creative_reach
    {
        creative_reach(mode)
    } else {
        survival_reach(mode)
    };
    let registry = context.collisions.registry(stream.network_id_mode());
    let world = PaletteWorld::new(
        stream.collision_store(),
        registry,
        stream.current_dimension(),
        crate::interaction_authority::pick_context(
            stream,
            &context.collisions,
            player_runtime.facts.player_game_mode(),
            player_runtime
                .selected_stack()
                .map(|item| item.block_runtime_id),
        ),
    );
    let vector = |value: Vec3| sim::Vec3::new(value.x as f64, value.y as f64, value.z as f64);
    let direct = world
        .block_interaction_ray_current(vector(ray.origin()), vector(ray.direction()), reach)
        .ok()?;
    let actor = pick_actor(
        stream.authority().remote_actors(),
        context.ui.gameplay_hud().mount_unique_id(),
        ray.origin().to_array(),
        ray.direction().to_array(),
        reach,
    );
    if let Crosshair::Actor(hit) =
        classify_block_hit(actor, ray.origin().to_array(), direct.as_ref(), reach)
    {
        return Some((None, Some(hit.runtime_id)));
    }
    let hit = match direct {
        Some(hit) => hit,
        None => world
            .block_use_miss_support_current(vector(ray.origin()), vector(ray.direction()))
            .ok()??,
    };
    if !context.collisions.selection_overlay_visible(
        stream.network_id_mode(),
        hit.runtime_id,
        player_runtime.facts.player_game_mode(),
    ) {
        return None;
    }
    let outline = world.block_outline(hit.block_pos, hit.runtime_id).ok()??;
    let point = |point: sim::Vec3| [point.x as f32, point.y as f32, point.z as f32];
    let assets = stream.runtime_assets();
    let shape = crack_shape_for_block(
        assets,
        stream.network_id_mode(),
        hit.runtime_id,
        hit.block_pos,
        |position| world.primary_runtime_id(position).ok(),
    )
    .unwrap_or(CrackShape::Cube);
    Some((
        Some(BlockSelectionTarget {
            block: hit.block_pos,
            bounds: [point(outline.min), point(outline.max)],
            shape,
        }),
        None,
    ))
}
