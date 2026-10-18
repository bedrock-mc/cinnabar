//! World decisions and held-item presentation use their own inventory view consistently.

use super::*;

const CONSUMPTION_TICKS: u32 = 32;

/// Installs a selected component food with an explicitly declared use duration.
fn selected_food() -> (client_state::PlayerRuntime, WorldStream, UiRuntime) {
    let mut player = client_state::PlayerRuntime(player_state::PlayerState::new(1));
    player
        .facts
        .publish_player_game_mode(protocol::PlayerGameMode::Survival);
    let (mut stream, mut ui) = fixture(&mut player, "fixture:food");
    let mut entry = protocol::vanilla_item_registry()
        .iter()
        .find(|entry| entry.identifier.as_ref() == "minecraft:apple")
        .unwrap()
        .clone();
    entry.network_id = BOW;
    entry.identifier = "fixture:food".into();
    entry.component_based = true;
    entry.version = ItemRegistryVersion::DataDriven;
    entry.canonical_empty_component_data = false;
    entry.components = Some(Arc::new(protocol::ItemComponents {
        food: true,
        food_can_always_eat: Some(false),
        use_animation: Some("eat".into()),
        use_duration_ticks: Some(CONSUMPTION_TICKS),
        item_properties_declared: true,
        max_stack_size: Some(64),
        ..Default::default()
    }));
    assert!(stream.seed_item_registry(ItemRegistryEvent {
        entries: vec![entry.clone()].into()
    }));
    let ledger = ui.inventory_ledger_mut(&mut player);
    ledger.apply_registry(&ItemRegistryEvent {
        entries: vec![entry].into(),
    });
    ledger.apply(&InventoryEvent::Authority(
        protocol::InventoryAuthority::Server,
    ));
    let mut food = stack(BOW);
    food.count = 2;
    food.nbt_digest = Sha256::digest([]).into();
    write_cell(ledger, protocol::PLAYER_INVENTORY_WINDOW_ID, 2, food);
    (player, stream, ui)
}

#[test]
fn consumption_animation_survives_pending_backing_transport() {
    let (mut player, stream, _) = selected_food();
    let duration = super::super::selection::consume_ticks(&player, &stream);
    assert_eq!(duration, Some(CONSUMPTION_TICKS));
    player
        .inventory
        .ledger_mut()
        .begin_world_drop(2, Some(1))
        .unwrap();
    assert_eq!(player.selected_stack().unwrap().count, 1);
    assert!(selected_air_use(&player, &stream).is_none());
    assert_eq!(
        super::super::selection::consume_ticks(&player, &stream),
        duration
    );
}

#[test]
fn consumption_animation_classifies_the_presented_stack() {
    let (mut player, stream, mut ui) = selected_food();
    open_screen(&mut player, &mut ui);
    ui.inventory_ledger_mut(&mut player).begin_click(2).unwrap();
    assert!(player.selected_stack().is_none());
    assert!(selected_air_use(&player, &stream).is_some());
    assert_eq!(
        super::super::selection::consume_ticks(&player, &stream),
        None
    );
}

/// Enables screen prediction without changing the world fixture's selected item.
fn open_screen(player: &mut client_state::PlayerRuntime, ui: &mut UiRuntime) {
    player
        .facts
        .publish_player_game_mode(protocol::PlayerGameMode::Survival);
    let ledger = ui.inventory_ledger_mut(player);
    ledger.apply(&InventoryEvent::Authority(
        protocol::InventoryAuthority::Server,
    ));
    assert!(ledger.request_personal_open(42));
    assert!(ledger.mark_transport_enqueued(0));
}

/// Writes one authoritative cell without constructing a screen prediction.
fn write_cell(
    ledger: &mut inventory::PlayerInventoryLedger,
    window: i32,
    slot: u16,
    stack: NetworkItemStack,
) {
    ledger.apply(&InventoryEvent::Slot(InventorySlotEvent {
        identity: SlotIdentity {
            container: ContainerIdentity::window(window),
            slot,
        },
        stack,
        storage_item: None,
    }));
}

#[test]
fn world_backing_ammunition_ignores_screen_only_arrow_changes() {
    for backing_arrow in [false, true] {
        let mut player = client_state::PlayerRuntime(player_state::PlayerState::new(1));
        let (stream, mut ui) = fixture(&mut player, "minecraft:crossbow");
        open_screen(&mut player, &mut ui);
        let ledger = ui.inventory_ledger_mut(&mut player);
        write_cell(
            ledger,
            protocol::PLAYER_INVENTORY_WINDOW_ID,
            5,
            if backing_arrow {
                stack(ARROW)
            } else {
                NetworkItemStack::empty()
            },
        );
        if !backing_arrow {
            write_cell(ledger, protocol::UI_INVENTORY_WINDOW_ID, 0, stack(ARROW));
        }
        ledger.begin_click(5).unwrap();
        assert_eq!(ledger.displayed_stack(5).is_some(), !backing_arrow);
        assert_eq!(
            super::super::selection::needs_met(
                &player,
                &stream,
                &ui,
                assets::item_use::ItemUseRequirement::Arrow
            ),
            backing_arrow
        );
        assert_eq!(
            loading_projectile(&player, &stream, &ui, false),
            backing_arrow.then_some(assets::item_use::ItemProjectileRole::Arrow)
        );
    }
}

/// Binds the fixture's numeric item before gestures compare its item data.
fn bind_item(ledger: &mut inventory::PlayerInventoryLedger, identifier: &str) {
    ledger.apply_registry(&ItemRegistryEvent {
        entries: Arc::from([ItemRegistryEntry {
            network_id: BOW,
            identifier: Arc::from(identifier),
            component_based: false,
            version: ItemRegistryVersion::Legacy,
            components: None,
            component_digest: [0; 32],
            negotiated_max_stack_size: Some(1),
            canonical_empty_component_data: true,
            item_tags: Arc::from([]),
        }]),
    });
}

/// Encodes an original trident with root integer damage and one Riptide enchantment.
fn trident(id: i32, damage: i32) -> NetworkItemStack {
    let mut extra = vec![255, 255, 1, 10, 0, 0, 3, 6, 0];
    extra.extend_from_slice(b"Damage");
    extra.extend_from_slice(&damage.to_le_bytes());
    extra.extend_from_slice(&[9, 4, 0]);
    extra.extend_from_slice(b"ench");
    extra.extend_from_slice(&[10, 1, 0, 0, 0, 2, 2, 0]);
    extra.extend_from_slice(b"id");
    extra.extend_from_slice(
        &i16::from(protocol_ids::Enchantment::Riptide.wire_value()).to_le_bytes(),
    );
    extra.extend_from_slice(&[2, 3, 0]);
    extra.extend_from_slice(b"lvl");
    extra.extend_from_slice(&1i16.to_le_bytes());
    extra.extend_from_slice(&[0; 10]);
    NetworkItemStack {
        stack_network_id: id,
        nbt_digest: Sha256::digest(&extra).into(),
        extra_data: extra.into(),
        ..stack(BOW)
    }
}

#[test]
fn world_backing_riptide_damage_does_not_follow_a_replacement_overlay() {
    for backing_broken in [false, true] {
        let mut player = client_state::PlayerRuntime(player_state::PlayerState::new(1));
        let (stream, mut ui) = fixture(&mut player, "minecraft:trident");
        let maximum = i32::try_from(
            stream
                .authority()
                .canonical_item_stack(&trident(201, 0))
                .unwrap()
                .durability
                .maximum()
                .unwrap(),
        )
        .unwrap();
        open_screen(&mut player, &mut ui);
        let ledger = ui.inventory_ledger_mut(&mut player);
        bind_item(ledger, "minecraft:trident");
        let backing_damage = if backing_broken { maximum } else { 0 };
        let cursor_damage = if backing_broken { 0 } else { maximum };
        write_cell(
            ledger,
            protocol::PLAYER_INVENTORY_WINDOW_ID,
            2,
            trident(201, backing_damage),
        );
        write_cell(
            ledger,
            protocol::PLAYER_INVENTORY_WINDOW_ID,
            3,
            trident(202, cursor_damage),
        );
        let request_id = ledger.begin_click(3).unwrap();
        assert!(ledger.mark_transport_enqueued(1));
        ledger.apply(&InventoryEvent::Response(
            protocol::ItemStackResponseEvent {
                responses: Arc::from([protocol::StackResponse {
                    request_id,
                    status: protocol::StackResponseStatus::Accepted,
                    containers: Arc::from([protocol::StackResponseContainer {
                        container: ContainerIdentity {
                            window_id: None,
                            slot_type: Some(protocol::CONTAINER_NAME_CURSOR),
                            dynamic_id: None,
                        },
                        slots: Arc::from([protocol::StackResponseSlot {
                            slot: 0,
                            hotbar_slot: 0,
                            count: 1,
                            item_stack_id: 202,
                            custom_name: Arc::from(""),
                            filtered_custom_name: Arc::from(""),
                            durability_correction: cursor_damage,
                        }]),
                    }]),
                }]),
            },
        ));
        ledger.begin_click(2).unwrap();
        assert_eq!(
            ledger
                .presented_slot_overlay(2)
                .and_then(|overlay| overlay.durability_correction),
            Some(cursor_damage)
        );
        let selected =
            super::super::selection::selected_riptide_use(&player, &stream, true).unwrap();
        assert_eq!(selected.level, 1);
        assert_eq!(selected.usable, !backing_broken);
    }
}

#[test]
fn world_backing_riptide_uses_signed_damage_values_from_integer_tags() {
    let mut player = client_state::PlayerRuntime(player_state::PlayerState::new(1));
    let (stream, mut ui) = fixture(&mut player, "minecraft:trident");
    player
        .facts
        .publish_player_game_mode(protocol::PlayerGameMode::Survival);
    let maximum = i32::try_from(
        stream
            .authority()
            .canonical_item_stack(&trident(201, 0))
            .unwrap()
            .durability
            .maximum()
            .unwrap(),
    )
    .unwrap();
    let ledger = ui.inventory_ledger_mut(&mut player);
    bind_item(ledger, "minecraft:trident");
    for (damage, usable) in [
        (maximum - 1, true),
        (maximum, false),
        (-1, true),
        (1 << 15, true),
        (1 << 16, true),
        ((1 << 16) + maximum, false),
        (-(1 << 15) - 1, false),
    ] {
        write_cell(
            ui.inventory_ledger_mut(&mut player),
            protocol::PLAYER_INVENTORY_WINDOW_ID,
            2,
            trident(201, damage),
        );
        assert_eq!(
            super::super::selection::selected_riptide_use(&player, &stream, true)
                .unwrap()
                .usable,
            usable,
            "root integer damage {damage}"
        );
    }
}

#[test]
fn world_backing_charge_does_not_replace_presented_crossbow_identity() {
    let mut player = client_state::PlayerRuntime(player_state::PlayerState::new(1));
    let (stream, mut ui) = fixture(&mut player, "minecraft:crossbow");
    bind_item(ui.inventory_ledger_mut(&mut player), "minecraft:crossbow");
    let mut runtime = ItemUseRuntime::default();
    runtime.observe_press(true);
    runtime.step(&use_frame(&player, &stream, &ui, 10));
    runtime.step(&use_frame(&player, &stream, &ui, 35));
    assert!(
        runtime
            .render_input(&player, &stream, &ui, 36, 0.0)
            .hand_charged
    );
    open_screen(&mut player, &mut ui);
    let ledger = ui.inventory_ledger_mut(&mut player);
    let mut shown = stack(BOW);
    shown.stack_network_id = 102;
    shown.extra_data = protocol::edit_item_extra(
        &[0; 10],
        &protocol::ItemNbtEdit {
            enchantments: Some(vec![(
                i16::from(protocol_ids::Enchantment::Quickcharge.wire_value()),
                3,
            )]),
            ..Default::default()
        },
    )
    .unwrap();
    shown.nbt_digest = Sha256::digest(&shown.extra_data).into();
    write_cell(ledger, protocol::UI_INVENTORY_WINDOW_ID, 0, shown);
    ledger.begin_click(2).unwrap();
    assert!(
        runtime
            .selected_projectile(&player, &ui)
            .flatten()
            .is_some(),
        "backing charge remains owned"
    );
    let input = runtime.render_input(&player, &stream, &ui, 37, 0.0);
    assert!(
        !input.hand_charged,
        "a different presented stack has no charge prediction"
    );
    let Some(AirUse::Hold { max_ticks, .. }) = classify("minecraft:crossbow", false, 3, None)
    else {
        panic!("crossbow duration fixture")
    };
    assert_eq!(input.max_use_ticks, max_ticks);
}
