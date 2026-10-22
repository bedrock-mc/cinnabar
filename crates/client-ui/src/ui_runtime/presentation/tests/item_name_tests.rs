//! Server-authored stack names through inventory authority and HUD capture.

use protocol::{
    ContainerIdentity, InventoryContentEvent, InventoryEvent, NetworkItemStack, PlayerGameMode,
    WorldBootstrap,
};

use super::*;
use crate::ui_runtime::presentation::{inventory_tooltip, publish::capture_hud_frame};

#[test]
fn enchantment_names_resolve_complete_keys_and_level_fallbacks() {
    let entries = [
        ("enchantment.damage.all", "Sharpness"),
        ("enchantment.fire", "Fire Aspect"),
        ("enchantment.level.2", "II"),
    ]
    .map(|(key, value)| assets::LangEntry {
        key: key.into(),
        value: value.into(),
    });
    let bytes = assets::encode_lang_catalog([9; 32], [10; 32], &entries).unwrap();
    let mut runtime = UiRuntime::new(1);
    runtime.set_lang_catalog(Arc::new(
        assets::RuntimeLangCatalog::decode(&bytes).unwrap(),
    ));
    assert_eq!(
        inventory_tooltip::enchantment_name(
            &runtime,
            i16::from(protocol_ids::Enchantment::Fireaspect.wire_value()),
            2,
        ),
        "Fire Aspect II",
    );
    assert_eq!(
        inventory_tooltip::enchantment_name(
            &runtime,
            i16::from(protocol_ids::Enchantment::Sharpness.wire_value()),
            3,
        ),
        "Sharpness 3",
    );
    assert_eq!(
        inventory_tooltip::enchantment_name(&runtime, -1, 2),
        "Enchantment -1 II",
    );
}

fn sword(name: Option<&str>) -> NetworkItemStack {
    let mut extra = (-1_i16).to_le_bytes().to_vec();
    extra.extend([1, 10, 0, 0]); // Version and unnamed fixed little-endian NBT root.
    if let Some(name) = name {
        extra.extend([10, 7, 0]);
        extra.extend(b"display");
        extra.extend([8, 4, 0]);
        extra.extend(b"Name");
        extra.extend(u16::try_from(name.len()).unwrap().to_le_bytes());
        extra.extend(name.as_bytes());
        extra.push(0);
    }
    extra.push(0);
    extra.extend([0; 8]); // Empty canPlaceOn and canDestroy lists.
    NetworkItemStack {
        network_id: protocol::vanilla_item_registry()
            .iter()
            .find(|item| item.identifier.as_ref() == "minecraft:diamond_sword")
            .unwrap()
            .network_id,
        count: 1,
        stack_network_id: 1,
        nbt_digest: Sha256::digest(&extra).into(),
        extra_data: extra.into(),
        ..NetworkItemStack::empty()
    }
}

fn stream() -> chunk_pipeline::WorldStream {
    chunk_pipeline::WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        local_player_runtime_id: 1,
        dimension: 0,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 0,
        block_network_ids_are_hashes: false,
    })
}

fn publish(
    runtime: &mut UiRuntime,
    player: &mut player_state::PlayerState,
    stacks: Vec<NetworkItemStack>,
) {
    runtime
        .enqueue_inventory_event(
            player,
            1,
            1,
            InventoryEvent::Content(InventoryContentEvent {
                container: ContainerIdentity::window(0),
                slots: stacks.into(),
                storage_item: NetworkItemStack::empty(),
            }),
        )
        .unwrap();
    runtime.drain_pending_inventory(player);
}

#[test]
fn selected_item_name_reads_nbt_and_restarts_for_same_kind_in_another_slot() {
    let mut player = player_state::PlayerState::new(1);
    let mut runtime = UiRuntime::new(1);
    player
        .facts
        .publish_player_game_mode(PlayerGameMode::Survival);
    let stacks = vec![
        sword(Some("§r§bFFA Selector")),
        sword(Some("§6Duel Selector")),
    ];
    publish(&mut runtime, &mut player, stacks.clone());
    let stream = stream();
    let mut presentation = UiPresentationRuntime::new(fixture_font()).unwrap();

    for (slot, now, expected) in [
        (0, 1_000, "§o§r§bFFA Selector§r"),
        (1, 10_000, "§o§6Duel Selector§r"),
        (0, 20_000, "§o§r§bFFA Selector§r"),
    ] {
        player.inventory.set_local_selected_slot(slot);
        assert!(player.selected_stack_custom_name().is_none());
        capture_hud_frame(
            &player,
            &mut runtime,
            &mut presentation,
            Some(&stream),
            semantic_input::PerspectiveMode::FirstPerson,
            now,
            Default::default(),
        );
        let name = presentation.hud_frame().selected_item_name.as_deref();
        assert_eq!(name, Some(expected));
        assert_eq!(runtime.selected_item_changed_millis(), Some(now));
        let tooltip = inventory_tooltip::tooltip_lines(
            &runtime,
            &stacks[usize::from(slot)],
            Some("minecraft:diamond_sword"),
            None,
        );
        assert_eq!(name, Some(tooltip[0].text.as_str()));
        capture_hud_frame(
            &player,
            &mut runtime,
            &mut presentation,
            Some(&stream),
            semantic_input::PerspectiveMode::FirstPerson,
            now + 100,
            Default::default(),
        );
        assert_eq!(runtime.selected_item_changed_millis(), Some(now));
    }
}

#[test]
fn selected_item_name_keeps_empty_custom_names_and_localizes_only_defaults() {
    let mut player = player_state::PlayerState::new(1);
    let mut runtime = UiRuntime::new(1);
    player
        .facts
        .publish_player_game_mode(PlayerGameMode::Survival);
    let input = b"item.diamond_sword.name=Localized Sword\n";
    runtime.set_server_lang(assets::ServerLangOverlay::read(input.len(), |target| {
        target.copy_from_slice(input);
        true
    }));
    let mut malformed = sword(None);
    malformed.extra_data = Arc::from([1, 2, 3]);
    malformed.nbt_digest = Sha256::digest(&malformed.extra_data).into();
    publish(
        &mut runtime,
        &mut player,
        vec![
            sword(Some("")),
            sword(None),
            malformed,
            sword(Some("雪の剣")),
        ],
    );
    let stream = stream();
    let mut presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
    for (slot, expected) in [
        (0, "§o§r"),
        (1, "Localized Sword§r"),
        (2, "Localized Sword§r"),
        (3, "§o雪の剣§r"),
    ] {
        player.inventory.set_local_selected_slot(slot);
        capture_hud_frame(
            &player,
            &mut runtime,
            &mut presentation,
            Some(&stream),
            semantic_input::PerspectiveMode::FirstPerson,
            1_000 + u64::from(slot),
            Default::default(),
        );
        assert_eq!(
            presentation.hud_frame().selected_item_name.as_deref(),
            Some(expected)
        );
    }
}

#[test]
fn selected_item_name_shared_resolution_preserves_corrections_and_component_color() {
    let mut runtime = UiRuntime::new(1);
    runtime.set_session_items(Some(Arc::new(
        crate::ui_runtime::item_facts::SessionItemComponents::from_iter([(
            Arc::from("minecraft:diamond_sword"),
            protocol::ItemComponents {
                hover_text_color: Some(Arc::from("aqua")),
                ..Default::default()
            },
        )]),
    )));
    let stack = sword(Some("Old Name"));
    let lines = inventory_tooltip::tooltip_lines(
        &runtime,
        &stack,
        Some("minecraft:diamond_sword"),
        Some("§r§cResponse Name"),
    );
    assert_eq!(lines[0].text, "§o§b§r§cResponse Name§r");
    let parsed = ui::parse_bedrock_text(&lines[0].text, lines[0].text.len()).unwrap();
    assert_eq!(parsed.plain_text(), "Response Name");
    assert_eq!(parsed[0].style.color, ui::BedrockColor::Red);
    assert!(!parsed[0].style.italic);
}

#[test]
fn component_tooltip_base_colours_agree_with_formatted_text() {
    let stack = sword(None);
    for (name, code) in [
        ("material_emerald", 'q'),
        ("material_lapis", 't'),
        ("material_resin", 'v'),
    ] {
        let mut runtime = UiRuntime::new(1);
        runtime.set_session_items(Some(Arc::new(
            crate::ui_runtime::item_facts::SessionItemComponents::from_iter([(
                Arc::from("minecraft:diamond_sword"),
                protocol::ItemComponents {
                    hover_text_color: Some(Arc::from(name)),
                    ..Default::default()
                },
            )]),
        )));
        let lines = inventory_tooltip::tooltip_lines(
            &runtime,
            &stack,
            Some("minecraft:diamond_sword"),
            None,
        );
        let formatted = format!("§{code}Name");
        let spans = ui::parse_bedrock_text(&formatted, formatted.len()).unwrap();
        let [r, g, b] = spans[0].style.color.rgb().unwrap();
        assert_eq!(lines[0].color, [r, g, b, 255], "{name}");
    }
}

fn potion(identifier: &str, metadata: u32) -> NetworkItemStack {
    NetworkItemStack {
        network_id: protocol::vanilla_item_registry()
            .iter()
            .find(|item| item.identifier.as_ref() == identifier)
            .unwrap()
            .network_id,
        metadata,
        ..sword(None)
    }
}

fn potion_lang_runtime() -> UiRuntime {
    let mut runtime = UiRuntime::new(1);
    let input = "item.splash_potion.name=Splash Potion\n\
        potion.fireResistance=Fire Resistance\n\
        potion.fireResistance.splash.name=Splash Potion of Fire Resistance\n\
        potion.poison=Poison\n\
        potion.poison.linger.name=Lingering Potion of Poison\n\
        potion.moveSlowdown=Slowness\n\
        potion.resistance=Resistance\n\
        potion.turtleMaster.name=Potion of the Turtle Master\n\
        potion.heal=Instant Health\n\
        potion.heal.splash.name=Splash Potion of Healing\n\
        potion.potency.1=II\n\
        potion.potency.3=IV\n\
        potion.potency.5=VI\n";
    runtime.set_server_lang(assets::ServerLangOverlay::read(input.len(), |target| {
        target.copy_from_slice(input.as_bytes());
        true
    }));
    runtime
}

fn tooltip(runtime: &UiRuntime, identifier: &str, metadata: u32) -> Vec<(String, [u8; 4])> {
    inventory_tooltip::tooltip_lines(
        runtime,
        &potion(identifier, metadata),
        Some(identifier),
        None,
    )
    .into_iter()
    .map(|line| (line.text, line.color))
    .collect()
}

#[test]
fn potion_tooltips_name_the_potion_and_list_its_effects() {
    let runtime = potion_lang_runtime();
    let blue = [85, 85, 255, 255];
    let red = [255, 85, 85, 255];
    let white = [255, 255, 255, 255];
    assert_eq!(
        tooltip(&runtime, "minecraft:splash_potion", 13),
        [
            ("Splash Potion of Fire Resistance§r".to_owned(), white),
            ("Fire Resistance (8:00)".to_owned(), blue),
        ]
    );
    assert_eq!(
        tooltip(&runtime, "minecraft:splash_potion", 22),
        [
            ("Splash Potion of Healing§r".to_owned(), white),
            ("Instant Health II".to_owned(), blue),
        ]
    );
    assert_eq!(
        tooltip(&runtime, "minecraft:potion", 39),
        [
            ("Potion of the Turtle Master§r".to_owned(), white),
            ("Slowness VI (0:20)".to_owned(), red),
            ("Resistance IV (0:20)".to_owned(), blue),
        ]
    );
    assert_eq!(
        tooltip(&runtime, "minecraft:lingering_potion", 26),
        [
            ("Lingering Potion of Poison§r".to_owned(), white),
            ("Poison (0:30)".to_owned(), red),
        ]
    );
    // An auxiliary value outside the vanilla potion table keeps the item name.
    assert_eq!(
        tooltip(&runtime, "minecraft:splash_potion", 200),
        [("Splash Potion§r".to_owned(), white)]
    );
}

#[test]
fn selected_item_name_names_the_held_potion() {
    let mut player = player_state::PlayerState::new(1);
    let mut runtime = potion_lang_runtime();
    player
        .facts
        .publish_player_game_mode(PlayerGameMode::Survival);
    publish(
        &mut runtime,
        &mut player,
        vec![potion("minecraft:splash_potion", 12)],
    );
    let stream = stream();
    let mut presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
    player.inventory.set_local_selected_slot(0);
    capture_hud_frame(
        &player,
        &mut runtime,
        &mut presentation,
        Some(&stream),
        semantic_input::PerspectiveMode::FirstPerson,
        1_000,
        Default::default(),
    );
    assert_eq!(
        presentation.hud_frame().selected_item_name.as_deref(),
        Some("Splash Potion of Fire Resistance§r")
    );
}

#[test]
fn potion_component_names_take_precedence_in_tooltip_and_hud() {
    let identifier = "minecraft:splash_potion";
    for (component_name, expected) in [
        (None, "Splash Potion of Fire Resistance§r"),
        (Some("Fortified Brew"), "Fortified Brew§r"),
        (Some("potion.fireResistance"), "Fire Resistance§r"),
        (Some(""), "§r"),
    ] {
        let mut player = player_state::PlayerState::new(1);
        let mut runtime = potion_lang_runtime();
        runtime.set_session_items(Some(Arc::new(
            crate::ui_runtime::item_facts::SessionItemComponents::from_iter([(
                Arc::from(identifier),
                protocol::ItemComponents {
                    display_name: component_name.map(Arc::from),
                    ..Default::default()
                },
            )]),
        )));
        player
            .facts
            .publish_player_game_mode(PlayerGameMode::Survival);
        let stack = potion(identifier, 12);
        publish(&mut runtime, &mut player, vec![stack.clone()]);
        let stream = stream();
        let mut presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
        player.inventory.set_local_selected_slot(0);
        capture_hud_frame(
            &player,
            &mut runtime,
            &mut presentation,
            Some(&stream),
            semantic_input::PerspectiveMode::FirstPerson,
            1_000,
            Default::default(),
        );
        let lines = inventory_tooltip::tooltip_lines(&runtime, &stack, Some(identifier), None);
        assert_eq!(lines[0].text, expected, "component: {component_name:?}");
        assert_eq!(
            presentation.hud_frame().selected_item_name.as_deref(),
            Some(expected),
            "component: {component_name:?}"
        );
    }
}

#[test]
fn creative_group_tooltips_are_not_italic() {
    let runtime = UiRuntime::new(1);
    let lines = inventory_tooltip::tooltip_lines_plain_name(
        &runtime,
        &NetworkItemStack::empty(),
        None,
        Some("Stairs"),
    );
    assert_eq!(lines[0].text, "Stairs§r");
}
