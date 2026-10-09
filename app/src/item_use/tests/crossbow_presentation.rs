use protocol::{
    ContainerIdentity, InventoryEvent, InventorySlotEvent, ItemRegistryEntry, ItemRegistryEvent,
    ItemRegistryVersion, SlotIdentity,
};

use super::*;
use crate::mining::FrozenMiningSelection;
use protocol::{NetworkItemStack, VerifiedNetworkItemStack};
use sha2::{Digest, Sha256};
use std::sync::Arc;

const BOW: i32 = 300;

fn selected_stack(slot: u8, network_id: i32, count: u16) -> FrozenMiningSelection {
    let extra_data: Arc<[u8]> = Arc::from([]);
    let stack = NetworkItemStack {
        network_id,
        metadata: 0,
        stack_network_id: 41,
        count,
        nbt_digest: Sha256::digest(&extra_data).into(),
        block_runtime_id: 0,
        extra_data,
    };
    FrozenMiningSelection {
        slot,
        item: VerifiedNetworkItemStack::try_new(stack.clone(), stack.nbt_digest).unwrap(),
    }
}

fn selection(slot: u8, network_id: i32) -> FrozenMiningSelection {
    selected_stack(slot, network_id, 1)
}

fn frame(tick: u64, held: bool) -> UseFrame {
    UseFrame {
        tick,
        now_millis: tick * 50,
        position: [0.5, 65.62, 0.5],
        held,
        selection: Some(selection(2, BOW)),
        air_use: classify("minecraft:bow", false, 0, None),
        ready: true,
        creative: false,
        inventory_revision: Some(1),
        charge_projectile: None,
        press_consumed: false,
    }
}

const ARROW: i32 = BOW + 1;
const FIREWORK: i32 = BOW + 2;

fn stack(network_id: i32) -> NetworkItemStack {
    NetworkItemStack {
        network_id,
        count: 1,
        stack_network_id: 41,
        ..NetworkItemStack::default()
    }
}

fn charged_stack() -> NetworkItemStack {
    let mut extra = vec![255, 255, 1, 10, 0, 0, 10, 11, 0];
    extra.extend_from_slice(b"chargedItem");
    extra.extend_from_slice(&[8, 4, 0]);
    extra.extend_from_slice(b"Name");
    let name = b"minecraft:arrow";
    extra.extend_from_slice(&(name.len() as u16).to_le_bytes());
    extra.extend_from_slice(name);
    extra.extend_from_slice(&[0, 0]);
    NetworkItemStack {
        nbt_digest: Sha256::digest(&extra).into(),
        extra_data: extra.into(),
        ..stack(BOW)
    }
}

fn fixture(
    player_runtime: &mut crate::player_runtime::PlayerRuntime,
    main_identifier: &str,
) -> (WorldStream, UiRuntime) {
    let mut stream = WorldStream::new(protocol::WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        local_player_unique_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: protocol::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    });
    assert!(
        stream.seed_item_registry(ItemRegistryEvent {
            entries: [
                (BOW, main_identifier),
                (ARROW, "minecraft:arrow"),
                (FIREWORK, "minecraft:firework_rocket"),
            ]
            .map(|(network_id, identifier)| ItemRegistryEntry {
                identifier: identifier.into(),
                network_id,
                component_based: false,
                version: ItemRegistryVersion::Legacy,
                component_digest: [0; 32],
                negotiated_max_stack_size: None,
                canonical_empty_component_data: true,
                item_tags: Arc::from([]),
            })
            .into(),
        })
    );
    let mut ui = UiRuntime::new(1);
    player_runtime.inventory.set_local_selected_slot(2);
    publish(
        player_runtime,
        &mut ui,
        1,
        ContainerIdentity::window(0),
        2,
        stack(BOW),
    );
    (stream, ui)
}

fn publish(
    player_runtime: &mut crate::player_runtime::PlayerRuntime,
    ui: &mut UiRuntime,
    sequence: u64,
    container: ContainerIdentity,
    slot: u16,
    stack: NetworkItemStack,
) {
    ui.enqueue_inventory_event(
        player_runtime,
        1,
        sequence,
        InventoryEvent::Slot(InventorySlotEvent {
            identity: SlotIdentity { container, slot },
            stack,
            storage_item: None,
        }),
    )
    .unwrap();
    ui.drain_pending_inventory(player_runtime);
}

fn use_frame(
    player_runtime: &crate::player_runtime::PlayerRuntime,
    stream: &WorldStream,
    ui: &UiRuntime,
    tick: u64,
) -> UseFrame {
    UseFrame {
        selection: verified_use_selection(player_runtime, ui),
        air_use: selected_air_use(player_runtime, stream),
        inventory_revision: ui
            .inventory_ledger(player_runtime)
            .authoritative_slot_revision(2),
        charge_projectile: loading_projectile(player_runtime, stream, ui, true),
        ..frame(tick, true)
    }
}

#[test]
fn presentation_load_fire_and_authoritative_nbt_share_one_charge_state() {
    let mut player_runtime = crate::player_runtime::PlayerRuntime::new(1);

    let (stream, mut ui) = fixture(&mut player_runtime, "minecraft:crossbow");
    let mut runtime = ItemUseRuntime::default();
    runtime.observe_press(true);
    runtime.step(&use_frame(&player_runtime, &stream, &ui, 10));
    runtime.step(&use_frame(&player_runtime, &stream, &ui, 35));
    let input = runtime.render_input(&player_runtime, &stream, &ui, 36, 0.5);
    assert!(input.hand_charged);
    assert_eq!(input.animation_frame, 4);
    assert_eq!(input.use_elapsed_ticks, None);
    assert_eq!(input.max_use_ticks, crossbow_duration());
    assert_eq!(
        runtime.local_item_use(&player_runtime, &stream, &ui),
        LocalItemUse::Idle
    );

    // Even an identical authoritative restatement rejects the local loaded state.
    publish(
        &mut player_runtime,
        &mut ui,
        2,
        ContainerIdentity::window(0),
        2,
        stack(BOW),
    );
    let input = runtime.render_input(&player_runtime, &stream, &ui, 37, 0.0);
    assert!(!input.hand_charged);
    assert_eq!(input.animation_frame, 0);
    publish(
        &mut player_runtime,
        &mut ui,
        3,
        ContainerIdentity::window(0),
        2,
        charged_stack(),
    );
    let input = runtime.render_input(&player_runtime, &stream, &ui, 38, 0.0);
    assert!(input.hand_charged);
    assert_eq!(input.animation_frame, 4);
    assert_eq!(input.max_use_ticks, crossbow_duration());
    assert_eq!(
        runtime.local_item_use(&player_runtime, &stream, &ui),
        LocalItemUse::Idle
    );

    runtime.observe_press(true);
    let fired = runtime.step(&use_frame(&player_runtime, &stream, &ui, 39));
    assert_eq!(kinds(&fired), ["use"]);
    assert!(!fired.started);
    let input = runtime.render_input(&player_runtime, &stream, &ui, 39, 0.0);
    assert!(!input.hand_charged);
    assert_eq!(input.animation_frame, 0);
    publish(
        &mut player_runtime,
        &mut ui,
        4,
        ContainerIdentity::window(0),
        2,
        stack(BOW),
    );
    assert!(
        !runtime
            .render_input(&player_runtime, &stream, &ui, 40, 0.0)
            .hand_charged
    );
}

#[test]
fn offhand_projectile_precedes_inventory_and_only_creative_synthesizes_ammo() {
    let mut player_runtime = crate::player_runtime::PlayerRuntime::new(1);
    let (stream, mut ui) = fixture(&mut player_runtime, "minecraft:crossbow");
    assert_eq!(
        loading_projectile(&player_runtime, &stream, &ui, false),
        None
    );
    assert_eq!(
        loading_projectile(&player_runtime, &stream, &ui, true),
        Some("minecraft:arrow")
    );
    publish(
        &mut player_runtime,
        &mut ui,
        2,
        ContainerIdentity::window(0),
        5,
        stack(ARROW),
    );
    assert_eq!(
        loading_projectile(&player_runtime, &stream, &ui, false),
        Some("minecraft:arrow")
    );
    let offhand = ContainerIdentity {
        window_id: None,
        slot_type: Some(protocol::CONTAINER_NAME_OFFHAND),
        dynamic_id: None,
    };
    publish(&mut player_runtime, &mut ui, 3, offhand, 0, stack(FIREWORK));
    assert_eq!(
        loading_projectile(&player_runtime, &stream, &ui, false),
        Some("minecraft:firework_rocket")
    );
    assert_eq!(
        loading_projectile(&player_runtime, &stream, &ui, true),
        Some("minecraft:firework_rocket")
    );
    publish(&mut player_runtime, &mut ui, 4, offhand, 0, stack(ARROW));
    assert_eq!(
        loading_projectile(&player_runtime, &stream, &ui, false),
        Some("minecraft:arrow")
    );
}

#[test]
fn normal_transaction_clears_loaded_prediction_and_retains_charged_nbt_and_offhand() {
    let mut player_runtime = crate::player_runtime::PlayerRuntime::new(1);

    let (stream, mut ui) = fixture(&mut player_runtime, "minecraft:crossbow");
    let mut runtime = ItemUseRuntime::default();
    runtime.observe_press(true);
    runtime.step(&use_frame(&player_runtime, &stream, &ui, 10));
    runtime.step(&use_frame(&player_runtime, &stream, &ui, 35));
    assert!(
        runtime
            .render_input(&player_runtime, &stream, &ui, 36, 0.0)
            .hand_charged
    );
    let batch = |main, offhand| {
        InventoryEvent::Transaction(protocol::InventoryTransactionEvent {
            slots: Arc::from([
                InventorySlotEvent {
                    identity: SlotIdentity {
                        container: ContainerIdentity::window(protocol::PLAYER_INVENTORY_WINDOW_ID),
                        slot: 2,
                    },
                    stack: main,
                    storage_item: None,
                },
                InventorySlotEvent {
                    identity: SlotIdentity {
                        container: ContainerIdentity::window(protocol::OFFHAND_WINDOW_ID),
                        slot: 0,
                    },
                    stack: offhand,
                    storage_item: None,
                },
            ]),
            skipped_actions: 0,
        })
    };
    ui.enqueue_inventory_event(
        &mut player_runtime,
        1,
        2,
        batch(stack(BOW), stack(FIREWORK)),
    )
    .unwrap();
    ui.drain_pending_inventory(&mut player_runtime);
    assert!(
        !runtime
            .render_input(&player_runtime, &stream, &ui, 37, 0.0)
            .hand_charged
    );
    assert_eq!(
        loading_projectile(&player_runtime, &stream, &ui, false),
        Some("minecraft:firework_rocket")
    );
    assert_eq!(ui.gameplay_hud().offhand_stack(), Some(&stack(FIREWORK)));
    ui.enqueue_inventory_event(
        &mut player_runtime,
        1,
        3,
        batch(charged_stack(), stack(ARROW)),
    )
    .unwrap();
    ui.drain_pending_inventory(&mut player_runtime);
    let rendered = runtime.render_input(&player_runtime, &stream, &ui, 38, 0.0);
    assert!(rendered.hand_charged);
    assert_eq!(rendered.animation_frame, 4);
    assert_eq!(
        player_runtime.presented_hotbar_stack(2),
        Some(&charged_stack())
    );
}

/// Reads the unloaded crossbow duration from the production item-use classifier.
fn crossbow_duration() -> u32 {
    match classify("minecraft:crossbow", false, 0, None).unwrap() {
        AirUse::Hold { max_ticks, .. } => max_ticks,
        _ => panic!("an unloaded crossbow charges"),
    }
}

/// Classifies the ordered use transactions produced by one accepted gameplay step.
fn kinds(outcome: &gameplay::item_use::UseOutcome) -> Vec<&'static str> {
    outcome
        .packets
        .iter()
        .map(|packet| {
            let debug = format!("{:?}", packet.data);
            if debug.contains("ItemUseInventoryTransaction(") {
                "use"
            } else if debug.contains("action_type: Release") {
                "release"
            } else {
                "other"
            }
        })
        .collect()
}

#[test]
fn owner_frame_is_admitted_only_for_the_selected_ranged_item() {
    for (identifier, expected) in [
        ("minecraft:apple", 0),
        ("minecraft:trident", 0),
        ("minecraft:bow", 1),
    ] {
        let mut player_runtime = crate::player_runtime::PlayerRuntime::new(1);
        let (stream, ui) = fixture(&mut player_runtime, identifier);
        let mut runtime = ItemUseRuntime::default();
        runtime.observe_press(true);
        let accepted = UseFrame {
            air_use: classify(identifier, false, 0, Some(32)),
            ..use_frame(&player_runtime, &stream, &ui, 10)
        };
        assert!(runtime.step(&accepted).started);
        let input = runtime.render_input(&player_runtime, &stream, &ui, 15, 0.5);
        assert_eq!(input.use_elapsed_ticks, Some(5));
        assert_eq!(
            input.animation_frame, expected,
            "using {identifier} must select its own frame rule"
        );
        let off = input.for_hand(true);
        assert_eq!(
            off.animation_frame, expected,
            "offhand models must see the selected {identifier} frame"
        );
        assert_eq!(off.use_elapsed_ticks, input.use_elapsed_ticks);
        assert_eq!(off.max_use_ticks, input.max_use_ticks);
        if identifier == "minecraft:bow" {
            let complete = runtime.render_input(
                &player_runtime,
                &stream,
                &ui,
                10 + u64::from(input.max_use_ticks),
                0.5,
            );
            assert_eq!(complete.use_elapsed_ticks, Some(input.max_use_ticks));
            assert_eq!(
                complete.animation_frame, 0,
                "a completed main-hand bow counter must select standby"
            );
        }
    }
}
