use super::*;
use crate::ui_runtime::presentation::publish::capture_hud_frame;

/// Builds a small original-art HUD so the production armour renderer needs no local packs.
fn armour_presentation() -> UiPresentationRuntime {
    use assets::{HudTexture, HudTextureRole, UiAtlasPage, UiFile};
    let textures = HudTextureRole::ALL.map(|role| {
        let [width, height] = role.expected_size();
        let colour = match role {
            HudTextureRole::ArmorFull => [235, 240, 250, 255],
            HudTextureRole::ArmorEmpty => [55, 65, 85, 255],
            HudTextureRole::ArmorHalf => [145, 155, 175, 255],
            _ => [0, 0, 0, 0],
        };
        let rgba8 = colour.repeat((width * height) as usize).into_boxed_slice();
        HudTexture {
            role,
            width,
            height,
            source_bytes: rgba8.len() as u32,
            source_sha256: Sha256::digest(&rgba8).into(),
            pixels_sha256: Sha256::digest(&rgba8).into(),
            rgba8,
        }
    });
    let hud = assets::encode_hud_catalog(assets::HUD_SOURCE_MANIFEST_SHA256, &textures).unwrap();
    let mut files = [
        ("ui/_ui_defs.json", r#"{"ui_defs":["ui/hud_screen.json"]}"#),
        ("ui/_global_variables.json", "{}"),
        ("ui/hud_screen.json", r#"{"namespace":"hud","hud_screen":{"type":"screen","controls":[{"protection":{"type":"custom","renderer":"armor_renderer","size":[90,9],"offset":[20,20],"anchor_from":"top_left","anchor_to":"top_left"}}]}}"#),
    ].map(|(path, text)| UiFile { path: path.into(), bytes: text.as_bytes().into() });
    files.sort_by(|a, b| a.path.cmp(&b.path));
    let pages = [UiAtlasPage {
        width: 1,
        height: 1,
        rgba8: vec![255; 4].into(),
    }];
    let carrier = assets::encode_ui_catalog([1; 32], &pages, &[], &[], &files).unwrap();
    let mut presentation = UiPresentationRuntime::with_hud(
        fixture_font(),
        Arc::new(RuntimeHudCatalog::decode(&hud).unwrap()),
    )
    .unwrap();
    presentation
        .enable_json_ui(Arc::new(assets::RuntimeUiAssets::decode(&carrier).unwrap()))
        .unwrap();
    presentation
}

#[test]
fn custom_wearable_protection_reaches_the_rendered_hud() {
    let mut player = player_state::PlayerState::new(1);
    player
        .facts
        .publish_player_game_mode(protocol::PlayerGameMode::Survival);
    let mut runtime = UiRuntime::new(1);
    runtime.hud.set_stats(
        BoundedStat::new(20, 20),
        BoundedStat::new(20, 20),
        BoundedStat::new(20, 20),
        None,
    );
    let mut presentation = armour_presentation();
    let mut entry = protocol::vanilla_item_registry()[0].clone();
    entry.identifier = Arc::from("fixture:coat");
    entry.network_id = 10_001;
    entry.component_based = true;
    entry.components = Some(std::sync::Arc::new(protocol::ItemComponents {
        wearable_declared: true,
        wearable_slot: Some(Arc::from("slot.armor.chest")),
        wearable_protection: Some(6),
        ..Default::default()
    }));
    let registry = protocol::ItemRegistryEvent {
        entries: vec![entry].into(),
    };
    let mut stream = chunk_pipeline::WorldStream::new(protocol::WorldBootstrap {
        local_player_unique_id: 1,
        local_player_runtime_id: 1,
        dimension: 0,
        player_position: [0.; 3],
        world_spawn_position: [0; 3],
        air_network_id: 0,
        block_network_ids_are_hashes: false,
    });
    assert!(stream.seed_item_registry(registry.clone()));
    runtime
        .enqueue_item_registry_event(&mut player, 1, 1, registry)
        .unwrap();
    runtime
        .enqueue_inventory_event(
            &mut player,
            1,
            2,
            protocol::InventoryEvent::Content(protocol::InventoryContentEvent {
                container: protocol::ContainerIdentity::window(protocol::ARMOR_WINDOW_ID),
                slots: vec![
                    protocol::NetworkItemStack::empty(),
                    protocol::NetworkItemStack {
                        network_id: 10_001,
                        count: 1,
                        ..protocol::NetworkItemStack::empty()
                    },
                    protocol::NetworkItemStack::empty(),
                    protocol::NetworkItemStack::empty(),
                ]
                .into(),
                storage_item: protocol::NetworkItemStack::empty(),
            }),
        )
        .unwrap();
    runtime.drain_pending_inventory(&mut player);
    capture_hud_frame(
        &player,
        &mut runtime,
        &mut presentation,
        Some(&stream),
        semantic_input::PerspectiveMode::FirstPerson,
        0,
        Default::default(),
    );
    let frame = presentation
        .build(&player, &runtime, 0, [480, 120], DpiScale::new(1.).unwrap())
        .unwrap();
    super::super::forms::snapshot::write(&frame, "custom-wearable-protection");
    assert_eq!(runtime.hud().armor().map(|value| value.current()), Some(6));
    let image = super::super::forms::snapshot::rasterize(&frame);
    assert!(image.pixels().any(|pixel| pixel.0 == [255, 255, 255, 255]));
}

#[test]
fn unknown_worn_item_keeps_protection_unresolved_in_the_hud() {
    let mut player = player_state::PlayerState::new(1);
    let mut runtime = UiRuntime::new(1);
    let mut presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
    let unknown = protocol::NetworkItemStack {
        network_id: 10_001,
        count: 1,
        ..protocol::NetworkItemStack::empty()
    };
    runtime
        .enqueue_inventory_event(
            &mut player,
            1,
            1,
            protocol::InventoryEvent::Content(protocol::InventoryContentEvent {
                container: protocol::ContainerIdentity::window(protocol::ARMOR_WINDOW_ID),
                slots: vec![
                    protocol::NetworkItemStack::empty(),
                    unknown,
                    protocol::NetworkItemStack::empty(),
                    protocol::NetworkItemStack::empty(),
                ]
                .into(),
                storage_item: protocol::NetworkItemStack::empty(),
            }),
        )
        .unwrap();
    runtime.drain_pending_inventory(&mut player);
    assert!(!runtime.local_armor(&player).chestplate.is_empty());
    capture_hud_frame(
        &player,
        &mut runtime,
        &mut presentation,
        None,
        semantic_input::PerspectiveMode::FirstPerson,
        0,
        Default::default(),
    );
    assert_eq!(runtime.hud().armor(), None);
}
