use super::*;
use gameplay::item_use::{UseFrame, classify};
use protocol::{
    ContainerIdentity, InventoryEvent, InventorySlotEvent, NetworkItemStack, SlotIdentity,
};
use std::sync::Arc;

/// Publishes a named vanilla stack into the player's authoritative inventory.
fn publish_item(player: &mut PlayerRuntime, ui: &mut UiRuntime, slot: u8, identifier: &str) {
    let network_id = protocol::vanilla_item_registry()
        .iter()
        .find(|item| item.identifier.as_ref() == identifier)
        .unwrap()
        .network_id;
    ui.enqueue_inventory_event(
        player,
        ui.session_id(),
        u64::from(slot) + 1,
        InventoryEvent::Slot(InventorySlotEvent {
            identity: SlotIdentity {
                container: ContainerIdentity::window(protocol::PLAYER_INVENTORY_WINDOW_ID),
                slot: u16::from(slot),
            },
            stack: NetworkItemStack {
                network_id,
                count: 16,
                stack_network_id: i32::from(slot) + 1,
                ..Default::default()
            },
            storage_item: None,
        }),
    )
    .unwrap();
    ui.drain_pending_inventory(player);
}

#[test]
fn hotbar_publishes_food_and_projectile_cooldowns_for_every_matching_slot() {
    for (identifier, unrelated) in [
        ("minecraft:chorus_fruit", "minecraft:ender_pearl"),
        ("minecraft:ender_pearl", "minecraft:wind_charge"),
        ("minecraft:wind_charge", "minecraft:chorus_fruit"),
    ] {
        let mut player = PlayerRuntime(player_state::PlayerState::new(1));
        let mut ui = UiRuntime::new(1);
        let mut stream = WorldStream::new(protocol::WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: 1,
            local_player_unique_id: 1,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
            block_network_ids_are_hashes: false,
        });
        assert!(stream.seed_item_registry(protocol::ItemRegistryEvent {
            entries: protocol::vanilla_item_registry(),
        }));
        for (slot, item) in [
            (0, identifier),
            (1, identifier),
            (2, unrelated),
            (3, "minecraft:snowball"),
        ] {
            publish_item(&mut player, &mut ui, slot, item);
        }
        player.inventory.set_local_selected_slot(0);
        stream.set_item_use_durations(Arc::new(std::collections::BTreeMap::from([(
            Box::from(identifier),
            32,
        )])));
        let network_id = player.selected_stack().unwrap().network_id;
        let air_use = classify(stream.authority().item_use_profile(network_id), false, 0).unwrap();
        let cooldown = air_use.cooldown().unwrap();
        let tick = 10;
        let mut item_use = ItemUseRuntime::default();
        item_use.observe_press(true);
        let outcome = item_use.step(&UseFrame {
            riptide_facing: Default::default(),
            riptide: None,
            tick,
            now_millis: tick * 50,
            position: [0.0; 3],
            held: true,
            selection: client_interaction::block_use::verified_use_selection(&player, &ui),
            air_use: Some(air_use),
            ready: true,
            creative: false,
            inventory_revision: ui.inventory_ledger(&player).authoritative_slot_revision(0),
            charge_projectile: None,
            press_consumed: false,
        });
        assert!(outcome.started || outcome.swung);
        for (elapsed, expected) in [
            (0, 1.0),
            (u64::from(cooldown.ticks) / 2, 0.5),
            (u64::from(cooldown.ticks), 0.0),
        ] {
            let values = hotbar_cooldowns(
                &player,
                &ui,
                Some(&stream),
                &item_use,
                &MeleeRuntime::default(),
                tick + elapsed,
            );
            assert_eq!(values[0], expected, "selected {identifier}");
            assert_eq!(values[1], expected, "another slot of {identifier}");
            assert!(values[2..].iter().all(|&value| value == 0.0));
        }
        assert!(
            hotbar_cooldowns(
                &player,
                &ui,
                None,
                &item_use,
                &MeleeRuntime::default(),
                tick
            )
            .iter()
            .all(|&value| value == 0.0)
        );
    }
}

#[test]
fn hotbar_publishes_shared_attack_cooldowns_and_expiry() {
    let mut player = PlayerRuntime(player_state::PlayerState::new(1));
    let mut ui = UiRuntime::new(1);
    let mut stream = WorldStream::new(protocol::WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        local_player_unique_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    });
    assert!(stream.seed_item_registry(protocol::ItemRegistryEvent {
        entries: protocol::vanilla_item_registry(),
    }));
    let items = [
        ("minecraft:iron_spear", "fixture:jab"),
        ("minecraft:netherite_spear", "fixture:jab"),
        ("minecraft:golden_spear", "fixture:other"),
    ];
    let cooldown = protocol::ItemAttackCooldown {
        category: "fixture:jab".into(),
        ticks: 20,
    };
    stream.set_item_attack_timings(Arc::new(
        items
            .into_iter()
            .map(|(item, category)| {
                (
                    item.into(),
                    protocol::ItemAttackTiming {
                        attack_cooldown: Some(protocol::ItemAttackCooldown {
                            category: category.into(),
                            ticks: cooldown.ticks,
                        }),
                        piercing_weapon: true,
                        ..Default::default()
                    },
                )
            })
            .collect(),
    ));
    for (slot, (identifier, _)) in items.into_iter().enumerate() {
        publish_item(&mut player, &mut ui, slot as u8, identifier);
    }
    player.inventory.set_local_selected_slot(0);
    let stack = player.selected_stack().unwrap().clone();
    let mut melee = gameplay::melee::MeleeRuntime::default();
    melee.observe_input(true, true);
    let outcome = melee.resolve(
        gameplay::melee::Crosshair::Miss,
        &gameplay::melee::PressContext {
            tick: 100,
            player_position: [0.0; 3],
            input_mode: protocol::PlayerInputMode::Mouse,
            local_runtime_id: 1,
            selection: Some(gameplay::mining::FrozenMiningSelection {
                slot: 0,
                item: protocol::VerifiedNetworkItemStack::try_new(stack.clone(), stack.nbt_digest)
                    .unwrap(),
            }),
            swing_duration: 20,
            now_millis: 0,
            item_attack: Some(gameplay::melee::ItemAttackPress {
                direction: [0.0, 0.0, 1.0],
                cooldown: Some(cooldown),
            }),
        },
        &mut gameplay::melee::SwingTracker::default(),
    );
    assert!(!outcome.packets.is_empty());
    let item_use = ItemUseRuntime::default();
    for (tick, expected) in [(100, 1.0), (110, 0.5), (120, 0.0), (121, 0.0)] {
        let actual = hotbar_cooldowns(&player, &ui, Some(&stream), &item_use, &melee, tick);
        assert_eq!(actual[0], expected, "selected spear at {tick}");
        assert_eq!(actual[1], expected, "shared category at {tick}");
        assert!(actual[2..].iter().all(|&value| value == 0.0));
    }
}
