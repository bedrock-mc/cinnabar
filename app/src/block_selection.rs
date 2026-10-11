//! Publishes the current gameplay pick to the native outline/highlight overlay passes.
use bevy::{ecs::system::SystemParam, prelude::*};
use gameplay::melee::{Crosshair, classify, pick_actor};
use render::{BlockSelectionFrame, BlockSelectionTarget, CrackShape, crack_shape_from_template};
use sim::PaletteWorld;

use client_ui::ui_runtime::UiRuntime;
use {
    crate::{interaction_authority::ray_is_current, runtime::world::ClientWorld},
    client_interaction::movement::PhysicsCollisionRegistries,
    client_presentation::local_player::InteractionOriginSnapshot,
    client_state::{ClientFrameSet, RuntimeSettings, SemanticInputSnapshot},
    gameplay::mining::{creative_reach, protocol_input_mode, survival_reach},
};

#[derive(SystemParam)]
pub(crate) struct SelectionContext<'w> {
    player: Res<'w, client_state::PlayerRuntime>,
    world: Res<'w, ClientWorld>,
    collisions: Res<'w, PhysicsCollisionRegistries>,
    ui: Res<'w, UiRuntime>,
    focus: Res<'w, client_state::ScreenFocus>,
    origin: Res<'w, InteractionOriginSnapshot>,
    input: Res<'w, SemanticInputSnapshot>,
    settings: Res<'w, RuntimeSettings>,
}

/// Updates after the completed pose and interaction authorities have been published.
pub(crate) fn configure(app: &mut App) {
    app.init_resource::<BlockSelectionFrame>()
        .add_systems(Update, publish.in_set(ClientFrameSet::WorldPublication));
}

/// A missing, stale or menu-owned ray clears last frame's target immediately.
pub(crate) fn publish(context: SelectionContext, mut frame: ResMut<BlockSelectionFrame>) {
    let target = target(&context.player, &context);
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

/// Resolves the compiled outline and its runtime context for the admitted pick.
/// Model highlighting uses its separate surface geometry.
fn target(
    player_runtime: &client_state::PlayerRuntime,
    context: &SelectionContext,
) -> Option<BlockSelectionTarget> {
    if context.focus.menu_visible || context.focus.ui_focused || context.world.fatal_error.is_some()
    {
        return None;
    }
    let stream = context.world.stream.as_ref()?;
    let ray = context.origin.outbound_ray()?;
    if !ray_is_current(ray, context.ui.session_id(), stream)
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
    let hit = match direct {
        Some(hit) => hit,
        None => {
            let actor = pick_actor(
                stream.authority().remote_actors(),
                context.ui.gameplay_hud().mount_unique_id(),
                ray.origin().to_array(),
                ray.direction().to_array(),
                reach,
            );
            if matches!(classify(actor, None, reach), Crosshair::Actor(_)) {
                return None;
            }
            world
                .block_use_miss_support_current(vector(ray.origin()), vector(ray.direction()))
                .ok()??
        }
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
    let visual = assets.resolve(stream.network_id_mode(), hit.runtime_id);
    let shape = visual
        .model_template()
        .and_then(|template| {
            crack_shape_from_template(assets, template, visual.variant(), hit.block_pos)
        })
        .unwrap_or(CrackShape::Cube);
    Some(BlockSelectionTarget {
        block: hit.block_pos,
        bounds: [point(outline.min), point(outline.max)],
        shape,
    })
}
