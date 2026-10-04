use super::*;
use crate::item_use::{AirUse, ItemUseRuntime, UseFrame, classify};
use protocol::{
    ContainerIdentity, InventoryEvent, InventorySlotEvent, NetworkItemStack, SlotIdentity,
};
use sha2::{Digest, Sha256};

fn presentation() -> UiPresentationRuntime {
    let sprites = (0..6)
        .map(|index| assets::IconSprite {
            width: 1,
            height: 1,
            rgba8: Arc::from([index, 0, 0, 255]),
        })
        .collect::<Vec<_>>();
    let mut entries = vec![assets::IconEntry {
        identifier: "minecraft:crossbow".into(),
        metadata: 0,
        sprite: 0,
    }];
    entries.extend((0..5).map(|metadata| assets::IconEntry {
        identifier: "minecraft:crossbow_pulling".into(),
        metadata,
        sprite: metadata + 1,
    }));
    let bytes = assets::encode_icon_catalog([1; 32], &sprites, &entries).unwrap();
    UiPresentationRuntime::with_hud_and_icons(
        crate::ui_runtime::presentation::tests::fixture_font(),
        crate::ui_runtime::presentation::tests::fixture_hud(),
        Arc::new(assets::RuntimeIconCatalog::decode(&bytes).unwrap()),
    )
    .unwrap()
}

fn crossbow(projectile: Option<&str>) -> NetworkItemStack {
    let network_id = protocol::vanilla_item_registry()
        .iter()
        .find(|item| item.identifier.as_ref() == "minecraft:crossbow")
        .unwrap()
        .network_id;
    let mut extra = Vec::new();
    if let Some(projectile) = projectile {
        extra.extend_from_slice(&[255, 255, 1, 10, 0, 0, 10, 11, 0]);
        extra.extend_from_slice(b"chargedItem");
        extra.extend_from_slice(&[8, 4, 0]);
        extra.extend_from_slice(b"Name");
        extra.extend_from_slice(&(projectile.len() as u16).to_le_bytes());
        extra.extend_from_slice(projectile.as_bytes());
        extra.extend_from_slice(&[0, 0]);
    }
    NetworkItemStack {
        network_id,
        count: 1,
        stack_network_id: 41,
        metadata: 73, // Durability is not an animation frame.
        nbt_digest: Sha256::digest(&extra).into(),
        extra_data: extra.into(),
        ..NetworkItemStack::default()
    }
}

fn publish(
    player_runtime: &mut crate::player_runtime::PlayerRuntime,
    ui: &mut UiRuntime,
    sequence: u64,
    slot: u16,
    stack: NetworkItemStack,
) {
    ui.enqueue_inventory_event(
        player_runtime,
        ui.session_id(),
        sequence,
        InventoryEvent::Slot(InventorySlotEvent {
            identity: SlotIdentity {
                container: ContainerIdentity::window(0),
                slot,
            },
            stack,
            storage_item: None,
        }),
    )
    .unwrap();
    ui.drain_pending_inventory(player_runtime);
}

fn stream() -> chunk_pipeline::WorldStream {
    let mut stream = chunk_pipeline::WorldStream::new(protocol::WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        local_player_unique_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: protocol::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    });
    assert!(stream.seed_item_registry(protocol::ItemRegistryEvent {
        entries: protocol::vanilla_item_registry(),
    }));
    stream
}

fn charge_frame(
    player_runtime: &crate::player_runtime::PlayerRuntime,
    ui: &UiRuntime,
    tick: u64,
) -> UseFrame {
    UseFrame {
        tick,
        now_millis: tick * 50,
        position: [0.0; 3],
        held: true,
        selection: crate::block_use::verified_use_selection(player_runtime, ui),
        air_use: classify("minecraft:crossbow", false, 0, None),
        ready: true,
        creative: false,
        inventory_revision: ui
            .inventory_ledger(player_runtime)
            .authoritative_slot_revision(2),
        charge_projectile: Some("minecraft:arrow"),
        press_consumed: false,
    }
}

#[test]
fn native_crossbow_icon_uses_loaded_nbt_and_frame_minus_one_not_damage() {
    let presentation = presentation();
    let runtime = UiRuntime::new(1);
    for (projectile, variant) in [("minecraft:arrow", 3), ("minecraft:firework_rocket", 4)] {
        let stack = crossbow(Some(projectile));
        assert_eq!(
            stack_icon(&runtime, &presentation, &stack, "minecraft:crossbow", None),
            presentation.item_icon("minecraft:crossbow_pulling", variant),
        );
        // Local fire clears loaded presentation without rewriting the charged stack.
        assert_eq!(
            stack_icon(
                &runtime,
                &presentation,
                &stack,
                "minecraft:crossbow",
                Some(0)
            ),
            presentation.item_icon("minecraft:crossbow", 0),
        );
        assert_eq!(
            protocol::item_charged_projectile(&stack.extra_data).as_deref(),
            Some(projectile)
        );
    }
    for frame in 1..=5 {
        assert_eq!(
            stack_icon(
                &runtime,
                &presentation,
                &crossbow(None),
                "minecraft:crossbow",
                Some(frame)
            ),
            presentation.item_icon("minecraft:crossbow_pulling", frame - 1),
        );
    }
    let mut malformed = crossbow(None);
    malformed.extra_data = Arc::from([255, 255, 1, 10]);
    assert_eq!(
        stack_icon(
            &runtime,
            &presentation,
            &malformed,
            "minecraft:crossbow",
            None
        ),
        presentation.item_icon("minecraft:crossbow", 0),
    );
}

#[test]
fn hotbar_inventory_and_held_icons_share_charge_fire_and_authoritative_corrections() {
    let mut player_runtime = crate::player_runtime::PlayerRuntime::new(1);

    let mut presentation = presentation();
    let stream = stream();
    let mut ui = UiRuntime::new(1);
    player_runtime.inventory.set_local_selected_slot(2);
    publish(&mut player_runtime, &mut ui, 1, 2, crossbow(None));
    publish(
        &mut player_runtime,
        &mut ui,
        2,
        12,
        crossbow(Some("minecraft:firework_rocket")),
    );
    let mut item_use = ItemUseRuntime::default();
    item_use.observe_press(true);
    let started = item_use.step(&charge_frame(&player_runtime, &ui, 10));
    assert!(started.started);
    let duration = match classify("minecraft:crossbow", false, 0, None).unwrap() {
        AirUse::Hold { max_ticks, .. } => u64::from(max_ticks),
        _ => unreachable!(),
    };
    item_use.step(&charge_frame(&player_runtime, &ui, 10 + duration));
    let mut capture = |player_runtime: &crate::player_runtime::PlayerRuntime,
                       ui: &mut UiRuntime,
                       item_use: &ItemUseRuntime,
                       tick| {
        super::super::capture_hud_frame(
            player_runtime,
            ui,
            &mut presentation,
            Some(&stream),
            semantic_input::PerspectiveMode::FirstPerson,
            tick * 50,
            ItemIconFrames(std::array::from_fn(|slot| {
                item_use.inventory_animation_frame(player_runtime, &stream, ui, slot as u8, tick)
            })),
        );
        let frame = presentation.hud_frame();
        assert_eq!(frame.hotbar_icons[2], frame.inventory_icons.0[2]);
        (frame.hotbar_icons[2], frame.inventory_icons.0[12])
    };
    let loaded = capture(&player_runtime, &mut ui, &item_use, 36);
    player_runtime.inventory.set_local_selected_slot(0);
    assert_eq!(capture(&player_runtime, &mut ui, &item_use, 37), loaded);
    // A byte-identical authoritative rejection invalidates the local overlay.
    publish(&mut player_runtime, &mut ui, 3, 2, crossbow(None));
    let corrected = capture(&player_runtime, &mut ui, &item_use, 38);
    assert_ne!(corrected.0, loaded.0);
    assert_eq!(corrected.1, loaded.1);
    publish(
        &mut player_runtime,
        &mut ui,
        4,
        2,
        crossbow(Some("minecraft:arrow")),
    );
    assert_eq!(capture(&player_runtime, &mut ui, &item_use, 39), loaded);
    player_runtime.inventory.set_local_selected_slot(2);
    item_use.observe_press(true);
    let mut fire = charge_frame(&player_runtime, &ui, 40);
    fire.air_use = Some(AirUse::Instant);
    assert!(!item_use.step(&fire).started);
    assert_eq!(capture(&player_runtime, &mut ui, &item_use, 40), corrected);
    assert_eq!(
        protocol::item_charged_projectile(&player_runtime.selected_stack().unwrap().extra_data)
            .as_deref(),
        Some("minecraft:arrow")
    );
    publish(
        &mut player_runtime,
        &mut ui,
        5,
        2,
        crossbow(Some("minecraft:firework_rocket")),
    );
    let rocket = capture(&player_runtime, &mut ui, &item_use, 41);
    assert_eq!(rocket.0, rocket.1);
    assert_ne!(rocket.0, loaded.0);
}
