//! Shared inputs for startup item captures, without checked-in pack assets.

use client_ui::ui_runtime::UiRuntime;
use protocol::{
    ContainerIdentity, InventoryContentEvent, InventoryEvent, NetworkItemStack, PlayerGameMode,
    WorldBootstrap,
};
use resource_pack::LayeredPackView;

pub(super) const IDENTIFIER: &str = "fixture:custom_sword";
pub(super) const ICON: &str = "plutonium_sword";
pub(super) const NETWORK_ID: i16 = 5;
pub(super) const PLAYER_ID: u64 = 42;

/// Opens the explicitly supplied resource archive, reporting a named missing-fixture skip.
pub(super) fn local_view(test: &str) -> Option<LayeredPackView> {
    let Some(pack) = std::env::var_os("CINNABAR_RENDER_PACK") else {
        eprintln!("skipping {test}: missing CINNABAR_RENDER_PACK fixture");
        return None;
    };
    let path = std::path::Path::new(&pack);
    if !path.is_file() {
        eprintln!("skipping {test}: missing CINNABAR_RENDER_PACK archive");
        return None;
    }
    Some(super::super::local_pack::local_pack_view_at(path).expect("local fixture pack"))
}

/// Registry-backed inventory state shared by the HUD and local equipment readers.
pub(super) struct SelectedInventory {
    pub stream: chunk_pipeline::WorldStream,
    pub player: player_state::PlayerState,
    pub runtime: UiRuntime,
}

/// Admits the startup registry and a server-authoritative sword in selected hotbar slot zero.
pub(super) fn selected_inventory(registry: protocol::ItemRegistryEvent) -> SelectedInventory {
    let mut stream = chunk_pipeline::WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: PLAYER_ID,
        player_position: [0., 64., 0.],
        world_spawn_position: [0, 64, 0],
        air_network_id: 0,
        block_network_ids_are_hashes: false,
    });
    assert!(stream.seed_item_registry(registry.clone()));
    let mut player = player_state::PlayerState::new(1);
    let mut runtime = UiRuntime::new(1);
    runtime.publish_inventory_authority(&mut player, protocol::InventoryAuthority::Server);
    runtime
        .publish_local_runtime_id(&mut player, 1, PLAYER_ID)
        .unwrap();
    runtime
        .enqueue_item_registry_event(&mut player, 1, 1, registry)
        .unwrap();
    let mut slots =
        vec![NetworkItemStack::empty(); inventory::inventory_ledger::PLAYER_INVENTORY_SLOT_COUNT];
    slots[0] = NetworkItemStack {
        network_id: i32::from(NETWORK_ID),
        count: 1,
        stack_network_id: 1,
        ..NetworkItemStack::empty()
    };
    runtime
        .enqueue_inventory_event(
            &mut player,
            1,
            2,
            InventoryEvent::Content(InventoryContentEvent {
                container: ContainerIdentity::window(0),
                slots: slots.into(),
                storage_item: NetworkItemStack::empty(),
            }),
        )
        .unwrap();
    runtime.drain_pending_inventory(&mut player);
    player.inventory.set_local_selected_slot(0);
    player
        .facts
        .publish_player_game_mode(PlayerGameMode::Survival);
    SelectedInventory {
        stream,
        player,
        runtime,
    }
}
