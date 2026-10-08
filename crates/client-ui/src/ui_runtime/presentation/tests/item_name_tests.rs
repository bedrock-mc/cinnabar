//! Server-authored stack names through inventory authority and HUD capture.

use protocol::{
    ContainerIdentity, InventoryContentEvent, InventoryEvent, NetworkItemStack, PlayerGameMode,
    WorldBootstrap,
};

use super::*;
use crate::ui_runtime::presentation::{inventory_tooltip, publish::capture_hud_frame};

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
