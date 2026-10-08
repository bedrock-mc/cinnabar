//! Publishes the current gameplay pick to the native outline/highlight overlay passes.
use bevy::{ecs::system::SystemParam, prelude::*};
use render::{BlockSelectionFrame, BlockSelectionTarget, CrackShape, crack_shape_from_template};
use sim::PaletteWorld;

use crate::{
    app::ClientFrameSet,
    interaction_authority::ray_is_current,
    local_player::InteractionOriginSnapshot,
    menu::MenuRuntime,
    mining::{creative_reach, protocol_input_mode, survival_reach},
    movement::PhysicsCollisionRegistries,
    runtime::world::ClientWorld,
    semantic_controls::SemanticInputSnapshot,
    settings_runtime::RuntimeSettings,
};
use client_ui::ui_runtime::UiRuntime;

#[derive(SystemParam)]
struct SelectionContext<'w> {
    player: Res<'w, crate::player_runtime::PlayerRuntime>,
    world: Res<'w, ClientWorld>,
    collisions: Res<'w, PhysicsCollisionRegistries>,
    ui: Res<'w, UiRuntime>,
    menu: Res<'w, MenuRuntime>,
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
fn publish(context: SelectionContext, mut frame: ResMut<BlockSelectionFrame>) {
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

/// Resolves reviewed visual bounds from the same shapes that admitted the pick.
/// Vanilla stairs deliberately outline a full
/// unit box: unioning its slab/step/inner collision pieces preserves that native
/// wire outline. Model highlighting below uses the separate actual surface.
fn target(
    player_runtime: &crate::player_runtime::PlayerRuntime,
    context: &SelectionContext,
) -> Option<BlockSelectionTarget> {
    if context.menu.is_visible()
        || context.ui.ui_focused(player_runtime)
        || context.world.fatal_error.is_some()
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
    );
    let vector = |value: Vec3| sim::Vec3::new(value.x as f64, value.y as f64, value.z as f64);
    let hit = world
        .block_interaction_ray_current(vector(ray.origin()), vector(ray.direction()), reach)
        .ok()??;
    if !context.collisions.selection_overlay_visible(
        stream.network_id_mode(),
        hit.runtime_id,
        player_runtime.facts.player_game_mode(),
    ) {
        return None;
    }
    let shapes = registry.selection_shapes(hit.runtime_id)?;
    let first = shapes.first()?;
    let mut min = first.min;
    let mut max = first.max;
    for shape in &shapes[1..] {
        min = sim::Vec3::new(
            min.x.min(shape.min.x),
            min.y.min(shape.min.y),
            min.z.min(shape.min.z),
        );
        max = sim::Vec3::new(
            max.x.max(shape.max.x),
            max.y.max(shape.max.y),
            max.z.max(shape.max.z),
        );
    }
    let offset = sim::Vec3::new(
        hit.block_pos[0] as f64,
        hit.block_pos[1] as f64,
        hit.block_pos[2] as f64,
    );
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
        bounds: [point(min + offset), point(max + offset)],
        shape,
    })
}
