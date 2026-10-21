use super::selection::selected_riptide_use;
use chunk_pipeline::WorldStream;
use client_state::PlayerRuntime;
use protocol::{
    ContainerIdentity, InventoryEvent, InventorySlotEvent, ItemRegistryEntry, ItemRegistryEvent,
    ItemRegistryVersion, ItemStackResponseEvent, NetworkItemStack, SlotIdentity, StackResponse,
    StackResponseContainer, StackResponseSlot, StackResponseStatus,
};
use sha2::{Digest, Sha256};

const SELECTED_SLOT: u8 = 2;
const TRIDENT: i32 = 42;

/// Reads the admitted maximum so the fixture follows the shared item facts.
fn maximum_damage() -> u32 {
    assets::item::durability::builtin_item_durability("minecraft:trident")
        .maximum()
        .unwrap()
}

/// Builds a selected trident whose canonical damage remains in its original NBT.
fn fixture(damage: u32) -> (PlayerRuntime, WorldStream) {
    let registry = ItemRegistryEvent {
        entries: [ItemRegistryEntry {
            identifier: "minecraft:trident".into(),
            network_id: TRIDENT,
            component_based: false,
            version: ItemRegistryVersion::Legacy,
            components: None,
            component_digest: [0; 32],
            negotiated_max_stack_size: None,
            canonical_empty_component_data: true,
            item_tags: [].into(),
        }]
        .into(),
    };
    let mut stream = WorldStream::new(protocol::WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        local_player_unique_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    });
    assert!(stream.seed_item_registry(registry.clone()));
    let mut player = PlayerRuntime(player_state::PlayerState::new(1));
    player.inventory.set_local_selected_slot(SELECTED_SLOT);
    let ledger = player.inventory.ledger_mut();
    ledger.begin_session(1);
    ledger.apply_registry(&registry);
    ledger.apply(&InventoryEvent::Authority(
        protocol::InventoryAuthority::Server,
    ));
    let mut extra = vec![255, 255, 1, 10, 0, 0, 3, 6, 0];
    extra.extend_from_slice(b"Damage");
    extra.extend_from_slice(&i32::try_from(damage).unwrap().to_le_bytes());
    extra.push(0);
    ledger.apply(&InventoryEvent::Slot(InventorySlotEvent {
        identity: SlotIdentity {
            container: ContainerIdentity::window(0),
            slot: u16::from(SELECTED_SLOT),
        },
        stack: NetworkItemStack {
            network_id: TRIDENT,
            count: 1,
            stack_network_id: 41,
            nbt_digest: Sha256::digest(&extra).into(),
            extra_data: extra.into(),
            ..NetworkItemStack::default()
        },
        storage_item: None,
    }));
    (player, stream)
}

/// Applies an accepted response through a real outstanding request.
fn correct_damage(player: &mut PlayerRuntime, damage: i32) {
    let ledger = player.inventory.ledger_mut();
    let request_id = ledger.begin_mining_request(SELECTED_SLOT, 1).unwrap();
    ledger.apply(&InventoryEvent::Response(ItemStackResponseEvent {
        responses: [StackResponse {
            status: StackResponseStatus::Accepted,
            request_id,
            containers: [StackResponseContainer {
                container: ContainerIdentity {
                    window_id: None,
                    slot_type: Some(protocol::CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY),
                    dynamic_id: None,
                },
                slots: [StackResponseSlot {
                    slot: SELECTED_SLOT,
                    hotbar_slot: SELECTED_SLOT,
                    count: 1,
                    item_stack_id: 77,
                    custom_name: "".into(),
                    filtered_custom_name: "".into(),
                    durability_correction: damage,
                }]
                .into(),
            }]
            .into(),
        }]
        .into(),
    }));
    assert_eq!(
        ledger
            .presented_slot_overlay(SELECTED_SLOT)
            .unwrap()
            .durability_correction,
        (damage >= 0).then_some(damage)
    );
}

#[test]
fn accepted_damage_materializes_and_stops_a_trident() {
    let (mut player, stream) = fixture(0);
    assert!(selected_riptide_use(&player, &stream, true).unwrap().usable);
    correct_damage(&mut player, i32::try_from(maximum_damage()).unwrap());
    assert_eq!(
        protocol::item_stack_damage(player.selected_stack().unwrap()),
        Some(maximum_damage())
    );
    assert!(!selected_riptide_use(&player, &stream, true).unwrap().usable);
}

#[test]
fn accepted_repair_materializes_and_restores_a_trident() {
    let damage = maximum_damage();
    let (mut player, stream) = fixture(damage);
    assert!(!selected_riptide_use(&player, &stream, true).unwrap().usable);
    correct_damage(&mut player, 0);
    assert_eq!(
        protocol::item_stack_damage(player.selected_stack().unwrap()),
        Some(0)
    );
    assert!(selected_riptide_use(&player, &stream, true).unwrap().usable);
}

#[test]
fn signed_damage_correction_materializes_in_the_selected_trident() {
    for damage in [0, maximum_damage()] {
        let (mut player, stream) = fixture(damage);
        correct_damage(&mut player, -1);
        assert_eq!(
            protocol::item_extra_damage_value(&player.selected_stack().unwrap().extra_data),
            Some(-1)
        );
        assert!(selected_riptide_use(&player, &stream, true).unwrap().usable);
    }
}
