//! Orders tick-bound interaction payloads and later frame-time presses in the session FIFO.
use super::*;

/// Drains completed movement before admitting frame-time attacks and item uses.
pub(super) fn configure(app: &mut App) {
    app.add_systems(
        Update,
        (
            flush_inventory_network,
            #[cfg(feature = "acceptance")]
            emit_movement_evidence,
            #[cfg(not(feature = "acceptance"))]
            crate::runtime::telemetry::discard_completed_movement_evidence,
            crate::melee::prepare_melee,
            produce_survival_mining,
            crate::item_use::prepare_item_release,
            #[cfg(feature = "tracy")]
            crate::tracy::plot_physics_to_send,
            send_player_auth_inputs,
            produce_melee,
            crate::item_use::produce_item_use,
            crate::pick_block::produce_pick_block,
        )
            .chain()
            .in_set(ClientFrameSet::NetworkSend),
    );
}
