//! Retained block-use picks and placement observations.
use crate::{
    mining::verified_selection,
    movement::{MovementTicker, PhysicsCollisionRegistries},
};
use bevy::prelude::{Res, ResMut, Resource};
use client_presentation::local_player::{InteractionOriginSnapshot, LocalViewPose};
use client_ui::ui_runtime::UiRuntime;
use gameplay::{
    block_use::UseSurroundings, interaction_authority::FrozenBlockObservation,
    mining::FrozenMiningSelection,
};
use sim::PaletteWorld;

mod prediction_frames;

/// Bevy resource adapter for the gameplay block_use owner.
#[derive(Resource, Debug, Default)]
pub struct BlockUseRuntime {
    owner: gameplay::block_use::BlockUseRuntime,
    /// The latest published frame pick; build actions run before the next publication.
    previous_pick: Option<FramePick>,
    pub prediction_frames: prediction_frames::PredictionFrames,
}
impl std::ops::Deref for BlockUseRuntime {
    type Target = gameplay::block_use::BlockUseRuntime;
    /// Borrows the gameplay owner at the existing ordered system boundary.
    fn deref(&self) -> &Self::Target {
        &self.owner
    }
}
impl std::ops::DerefMut for BlockUseRuntime {
    /// Mutates the gameplay owner without duplicating its state.
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.owner
    }
}

/// One frame's eye ray; vanilla builds from the picks of the frames before each tick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FramePick {
    /// Movement authority when the pick was taken; a correction or reanchor retires it.
    authority: (u64, u64),
    pub session_generation: u64,
    pub actor_session_id: u64,
    pub origin: bevy::prelude::Vec3,
    pub direction: bevy::prelude::Vec3,
}

impl BlockUseRuntime {
    /// Keeps this frame's published pick for the build actions before the next tick.
    pub fn retain_pick(&mut self, origin: &InteractionOriginSnapshot, authority: (u64, u64)) {
        self.previous_pick = origin.outbound_ray().map(|ray| FramePick {
            authority,
            session_generation: ray.session_generation(),
            actor_session_id: ray.actor_session_id(),
            origin: ray.origin(),
            direction: ray.direction(),
        });
    }

    /// The retained pick, if it was taken under `authority`.
    pub fn pick(&self, authority: (u64, u64)) -> Option<FramePick> {
        self.previous_pick
            .filter(|previous| previous.authority == authority)
    }
}

/// Retains each frame's published (and assisted) pick for the next tick's build actions.
pub fn retain_block_use_pick(
    origin: Res<InteractionOriginSnapshot>,
    movement: Res<MovementTicker>,
    mut runtime: ResMut<BlockUseRuntime>,
) {
    runtime.retain_pick(&origin, movement.interaction_authority_identity());
}
/// The selected stack, unless its own slot awaits a gesture answer or the hotbar change is unsent.
/// Requests on other cells never hold a use back: vanilla acts on the predicted held item.
pub fn verified_use_selection(
    player_runtime: &client_state::PlayerRuntime,
    ui: &UiRuntime,
) -> Option<FrozenMiningSelection> {
    let ledger = ui.inventory_ledger(player_runtime);
    let selection = verified_selection(player_runtime)?;
    if ledger.slot_gesture_pending(selection.slot)
        || ledger.resync_required()
        || player_runtime
            .inventory
            .pending_hotbar_selection()
            .is_some()
    {
        return None;
    }
    Some(selection)
}

#[cfg(test)]
mod tests;

/// Adapts the published world to gameplay's switch prediction.
pub fn predicted_toggle(
    collisions: &PhysicsCollisionRegistries,
    stream: &chunk_pipeline::WorldStream,
    clicked: u32,
) -> Option<u32> {
    gameplay::block_use::predicted_toggle(
        collisions,
        &crate::movement::world_view::GameplayWorldView(stream),
        clicked,
    )
}
/// Adapts the published world to gameplay's placement prediction.
pub fn predicted_placement(
    collisions: &PhysicsCollisionRegistries,
    stream: &chunk_pipeline::WorldStream,
    item_block: i32,
    observed: &FrozenBlockObservation,
    surroundings: &UseSurroundings,
    view: LocalViewPose,
) -> Option<gameplay::block_use::PredictedPlacement> {
    let mode = stream.network_id_mode();
    let world = PaletteWorld::new(
        stream.collision_store(),
        collisions.registry(mode),
        stream.current_dimension(),
        Default::default(),
    );
    let range = stream
        .authority()
        .dimension_range(stream.current_dimension())?;
    let (yaw, pitch, _) = view.rotation().to_euler(bevy::math::EulerRot::YXZ);
    gameplay::block_use::predicted_placement(
        collisions,
        mode,
        held_block_store_id(stream, item_block)?,
        &world,
        &gameplay::block_use::PlacementContext {
            clicked: observed.target.position,
            input: gameplay::placement_state::PlacementInput {
                face: observed.target.face,
                click_position: observed.target.relative_hit,
                yaw: gameplay::movement::wire_yaw(180.0 - yaw.to_degrees()),
                pitch: -pitch.to_degrees(),
            },
            surroundings,
            build_height: range.base_sub_chunk_y * 16
                ..(range.base_sub_chunk_y + range.sub_chunk_count as i32) * 16,
        },
    )
}
/// Resolves an item block through the production gameplay boundary.
pub fn held_block_store_id(stream: &chunk_pipeline::WorldStream, item_block: i32) -> Option<u32> {
    gameplay::block_use::held_block_store_id(
        &crate::movement::world_view::GameplayWorldView(stream),
        item_block,
    )
}
