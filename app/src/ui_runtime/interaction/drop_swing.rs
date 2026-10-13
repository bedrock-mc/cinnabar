//! The arm swing that dropping the held item plays and sends.

use bevy::prelude::{Res, ResMut};

/// What dropping the held item needs to swing the arm, as vanilla does with a `dropitem` swing.
#[derive(bevy::ecs::system::SystemParam)]
pub(crate) struct DropSwing<'w> {
    swings: Option<ResMut<'w, client_interaction::melee::SwingTracker>>,
    movement: Option<Res<'w, client_interaction::movement::MovementTicker>>,
    effects: Option<Res<'w, client_interaction::movement::LocalMovementEffectTimeline>>,
    network: Option<
        Res<
            'w,
            client_runtime::network::NetworkHandle<
                crate::runtime::network::resource_packs::PackApplication,
            >,
        >,
    >,
    client_world: Option<Res<'w, crate::runtime::world::ClientWorld>>,
}

impl DropSwing<'_> {
    /// Swings the local arm for the tick about to simulate and tells the server, unless a swing
    /// is already running.
    pub(super) fn swing(mut self, player_runtime: &client_state::PlayerRuntime) {
        let (Some(swings), Some(movement), Some(effects), Some(network), Some(client_world)) = (
            self.swings.as_deref_mut(),
            self.movement.as_deref(),
            self.effects.as_deref(),
            self.network.as_deref(),
            self.client_world.as_deref(),
        ) else {
            return;
        };
        let (Some(state), Some(stream)) = (movement.build_action_state(), &client_world.stream)
        else {
            return;
        };
        let item_duration = crate::melee::selected_attack_timing(player_runtime, client_world)
            .and_then(|timing| timing.swing_duration_ticks);
        if super::inventory::swing_before_tick(
            swings,
            movement.interaction_authority_identity(),
            movement.completed_tick(),
            effects,
            item_duration,
            state.tick.saturating_add(1),
        ) {
            let _ = network.send_inventory_packet(protocol::swing_arm_packet(
                stream.local_player_runtime_id(),
                protocol::SwingSource::DropItem,
            ));
        }
    }
}
