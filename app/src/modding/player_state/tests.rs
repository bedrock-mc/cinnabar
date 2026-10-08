use std::sync::Arc;

use inventory::inventory_ledger::{PLAYER_INVENTORY_SLOT_COUNT, StackResponseOverlay};
use protocol::{
    ActorEffectAction, ActorEffectEvent, ContainerIdentity, InventoryContentEvent, InventoryEvent,
    InventorySlotEvent, NetworkItemStack, SlotIdentity, WorldBootstrap,
};
use sha2::{Digest, Sha256};

use super::*;

const SESSION: u64 = 7;

fn authority() -> WorldAuthority {
    WorldAuthority::new(
        WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: 1,
            local_player_unique_id: 1,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: 0,
            block_network_ids_are_hashes: false,
        },
        Arc::new(assets::RuntimeAssets::diagnostic()),
        None,
        [0.0; 3],
        None,
    )
}

fn stack(identifier: &str, count: u16) -> NetworkItemStack {
    NetworkItemStack {
        network_id: protocol::vanilla_item_registry()
            .iter()
            .find(|entry| entry.identifier.as_ref() == identifier)
            .unwrap()
            .network_id,
        count,
        ..Default::default()
    }
}

fn content(window: i32, slots: Vec<NetworkItemStack>) -> InventoryEvent {
    InventoryEvent::Content(InventoryContentEvent {
        container: ContainerIdentity::window(window),
        slots: slots.into(),
        storage_item: NetworkItemStack::empty(),
    })
}

fn damage(stack: &mut NetworkItemStack, value: u32) {
    // Valid retained little-endian root NBT Damage tag in item user data.
    let mut extra = vec![0xff, 0xff, 1, 10, 0, 0, 3, 6, 0];
    extra.extend_from_slice(b"Damage");
    extra.extend_from_slice(&value.to_le_bytes());
    extra.push(0);
    stack.nbt_digest = Sha256::digest(&extra).into();
    stack.extra_data = extra.into();
}

#[test]
fn snapshots_preserve_unknown_empty_and_present_inventory_and_gear() {
    let authority = authority();
    let mut player = player_state::PlayerState::new(SESSION);
    let ui = UiRuntime::new(SESSION);
    let empty = snapshot(&authority, SESSION, &player, &ui, 0).unwrap();
    assert!(empty.inventory.iter().all(|slot| !slot.known));
    assert!(empty.armor.iter().all(|slot| !slot.known));
    assert!(!empty.offhand.known);
    let mut slots = vec![NetworkItemStack::empty(); PLAYER_INVENTORY_SLOT_COUNT];
    slots[0] = stack("minecraft:arrow", 16);
    slots[9] = stack("minecraft:arrow", 23);
    player.inventory.ledger_mut().apply(&content(0, slots));
    player.inventory.set_local_selected_slot(0);
    player.inventory.ledger_mut().apply(&content(
        protocol::ARMOR_WINDOW_ID,
        vec![
            stack("minecraft:diamond_helmet", 1),
            NetworkItemStack::empty(),
        ],
    ));
    player.inventory.ledger_mut().apply(&content(
        protocol::OFFHAND_WINDOW_ID,
        vec![stack("minecraft:arrow", 8)],
    ));
    let actual = snapshot(&authority, SESSION, &player, &ui, 0).unwrap();
    assert_eq!(actual.selected_slot, Some(0));
    assert!(actual.inventory.iter().all(|slot| slot.known));
    let arrows: u32 = actual
        .inventory
        .iter()
        .filter_map(|slot| slot.item.as_ref())
        .map(|item| u32::from(item.count))
        .sum();
    assert_eq!(arrows, 39);
    assert!(actual.armor[0].item.is_some());
    assert!(actual.armor[1].known && actual.armor[1].item.is_none());
    assert!(!actual.armor[2].known);
    assert_eq!(actual.offhand.item.unwrap().count, 8);
    player.begin_session(SESSION + 1);
    assert!(snapshot(&authority, SESSION, &player, &ui, 0).is_none());
}

#[test]
fn durability_corrections_win_and_custom_or_invalid_data_is_not_invented() {
    let authority = authority();
    let mut player = player_state::PlayerState::new(SESSION);
    let mut helmet = stack("minecraft:diamond_helmet", 1);
    damage(&mut helmet, 7);
    let item = project_item(&authority, player.inventory.ledger(), &helmet, None).unwrap();
    assert_eq!(item.damage, Some(7));
    assert_eq!(
        item.max_durability,
        client_world::vanilla_max_durability("minecraft:diamond_helmet")
    );
    let corrected = project_item(
        &authority,
        player.inventory.ledger(),
        &helmet,
        Some(&StackResponseOverlay {
            durability_correction: Some(11),
            ..Default::default()
        }),
    )
    .unwrap();
    assert_eq!(corrected.damage, Some(11));
    let mut custom = protocol::vanilla_item_registry()
        .iter()
        .find(|entry| entry.network_id == helmet.network_id)
        .unwrap()
        .clone();
    custom.component_based = true;
    player
        .inventory
        .ledger_mut()
        .apply_registry(&protocol::ItemRegistryEvent {
            entries: vec![custom].into(),
        });
    assert_eq!(
        project_item(&authority, player.inventory.ledger(), &helmet, None)
            .unwrap()
            .max_durability,
        None
    );
    helmet.nbt_digest = [0; 32];
    let invalid = project_slot(
        &authority,
        player.inventory.ledger(),
        PlayerInventorySlot::Present(&helmet),
        None,
    );
    assert!(!invalid.known);
    assert!(invalid.item.is_none());
}

fn effect(id: i32, duration: i32) -> ActorEffectEvent {
    ActorEffectEvent {
        dimension: 0,
        actor_runtime_id: 1,
        action: ActorEffectAction::Add,
        effect_id: id,
        amplifier: 1,
        particles: true,
        ambient: false,
        duration_ticks: duration,
        tick: 40,
    }
}

#[test]
fn effects_count_down_update_remove_and_keep_infinite_durations() {
    let authority = authority();
    let player = player_state::PlayerState::new(SESSION);
    let mut ui = UiRuntime::new(SESSION);
    ui.apply_local_effect(SESSION, 1, effect(19, -1), 1_000)
        .unwrap();
    ui.apply_local_effect(SESSION, 2, effect(1, 80), 1_000)
        .unwrap();
    let first = snapshot(&authority, SESSION, &player, &ui, 2_000).unwrap();
    assert_eq!(
        first
            .effects
            .iter()
            .map(|effect| effect.effect_id)
            .collect::<Vec<_>>(),
        [1, 19]
    );
    assert_eq!(first.effects[0].remaining_ticks, Some(60));
    assert_eq!(first.effects[0].amplifier, 1);
    assert_eq!(first.effects[1].remaining_ticks, None);
    assert_eq!(
        snapshot(&authority, SESSION, &player, &ui, 5_000)
            .unwrap()
            .effects
            .len(),
        1
    );
    let mut update = effect(1, 200);
    update.action = ActorEffectAction::Update;
    update.amplifier = 2;
    update.tick = 120;
    ui.apply_local_effect(SESSION, 3, update, 5_000).unwrap();
    assert_eq!(
        snapshot(&authority, SESSION, &player, &ui, 5_000)
            .unwrap()
            .effects[0]
            .remaining_ticks,
        Some(200)
    );
    let mut remove = effect(1, 0);
    remove.action = ActorEffectAction::Remove;
    remove.tick = 120;
    ui.apply_local_effect(SESSION, 4, remove, 5_000).unwrap();
    assert_eq!(
        snapshot(&authority, SESSION, &player, &ui, 5_000)
            .unwrap()
            .effects[0]
            .effect_id,
        19
    );
    ui.begin_session(SESSION + 1);
    assert!(snapshot(&authority, SESSION, &player, &ui, 5_000).is_none());
}

#[test]
fn block_classification_uses_retained_identity_and_unknown_names_preserve_counts() {
    let authority = authority();
    let player = player_state::PlayerState::new(SESSION);
    let mut block = stack("minecraft:stone", 64);
    block.block_runtime_id = 7;
    assert!(
        project_item(&authority, player.inventory.ledger(), &block, None)
            .unwrap()
            .block
    );
    block.network_id = i32::MAX;
    let unknown = project_item(&authority, player.inventory.ledger(), &block, None).unwrap();
    assert_eq!(unknown.identifier, None);
    assert_eq!(unknown.count, 64);
    assert_eq!(unknown.max_durability, None);
}

#[test]
fn current_authoritative_empty_replaces_old_gear_and_session_zero_is_absent() {
    let authority = authority();
    let mut player = player_state::PlayerState::new(SESSION);
    let ui = UiRuntime::new(SESSION);
    player.inventory.ledger_mut().apply(&content(
        protocol::ARMOR_WINDOW_ID,
        vec![stack("minecraft:diamond_helmet", 1)],
    ));
    assert!(
        snapshot(&authority, SESSION, &player, &ui, 0)
            .unwrap()
            .armor[0]
            .item
            .is_some()
    );
    player
        .inventory
        .ledger_mut()
        .apply(&InventoryEvent::Slot(InventorySlotEvent {
            identity: SlotIdentity {
                container: ContainerIdentity::window(protocol::ARMOR_WINDOW_ID),
                slot: 0,
            },
            stack: NetworkItemStack::empty(),
            storage_item: None,
        }));
    let actual = snapshot(&authority, SESSION, &player, &ui, 0).unwrap();
    assert!(actual.armor[0].known && actual.armor[0].item.is_none());
    assert!(snapshot(&authority, 0, &player, &ui, 0).is_none());
}
