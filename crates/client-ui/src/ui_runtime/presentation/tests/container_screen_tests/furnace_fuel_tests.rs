use super::*;
use serde_json::json;
use std::sync::Arc;

/// Creates an original slot grid over the production furnace and player collections.
fn furnace_presentation(runtime: &mut UiRuntime) -> UiPresentationRuntime {
    let collections = [
        ("furnace_ingredient_items", [1, 1], [40, 30]),
        ("furnace_fuel_items", [1, 1], [40, 70]),
        ("furnace_output_items", [1, 1], [120, 50]),
        ("inventory_items", [9, 3], [20, 120]),
        ("hotbar_items", [9, 1], [20, 200]),
    ];
    let mut definitions = serde_json::Map::new();
    definitions.insert("namespace".into(), json!("furnace"));
    let mut controls = Vec::new();
    for (collection, dimensions, offset) in collections {
        controls.push(
            json!({collection: {"type":"grid", "size":[dimensions[0]*24, dimensions[1]*24],
            "anchor_from":"top_left", "anchor_to":"top_left", "offset":offset,
            "grid_dimensions":dimensions, "collection_name":collection,
            "grid_item_template":format!("furnace.{collection}_cell")}}),
        );
        definitions.insert(
            format!("{collection}_cell"),
            json!({"type":"panel", "size":[24,24],
            "controls":[
                {"border":{"type":"image", "texture":"textures/ui/original_slot", "size":[22,22]}},
                {"item":{"type":"custom", "renderer":"inventory_item_renderer", "size":[16,16],
                    "bindings":[{"binding_type":"collection", "binding_collection_name":collection,
                    "binding_name":"#item_renderer_data"}]}}
            ]}),
        );
    }
    definitions.insert(
        "furnace_screen".into(),
        json!({"type":"screen", "controls":controls}),
    );
    let mut png = std::io::Cursor::new(Vec::new());
    image::RgbaImage::from_fn(22, 22, |x, y| {
        image::Rgba(if x == 0 || y == 0 || x == 21 || y == 21 {
            [160, 180, 200, 255]
        } else {
            [35, 45, 60, 255]
        })
    })
    .write_to(&mut png, image::ImageFormat::Png)
    .unwrap();
    let mut presentation = crate::test_support::mini_engine_presentation();
    presentation.set_server_ui_pack(&super::super::super::forms::ServerUiPack {
        ui_layers: vec![vec![
            (
                "ui/_ui_defs.json".into(),
                br#"{"ui_defs":["ui/furnace.json"]}"#.to_vec(),
            ),
            (
                "ui/furnace.json".into(),
                serde_json::Value::Object(definitions)
                    .to_string()
                    .into_bytes(),
            ),
        ]],
        textures: vec![("textures/ui/original_slot.png".into(), png.into_inner())],
        ..Default::default()
    });
    let icons = Arc::new(super::super::super::SessionIcons {
        icons: vec![super::super::super::SessionIcon {
            identifier: "custom:oak_planks".into(),
            metadata: 0,
            width: 16,
            height: 16,
            rgba8: [230, 160, 45, 255].repeat(16 * 16).into(),
        }],
        ..Default::default()
    });
    super::super::super::session_icons::observe(&mut presentation, Some(&icons));
    runtime.set_session_icons(Some(icons));
    presentation
}

#[test]
fn furnace_quick_move_does_not_treat_a_custom_suffix_as_fuel() {
    let mut player = player_state::PlayerState::new(1);
    let mut runtime = opened(&mut player, ::protocol::WINDOW_TYPE_FURNACE, 3);
    let registry = ::protocol::ItemRegistryEvent {
        entries: vec![::protocol::ItemRegistryEntry {
            components: None,
            identifier: "custom:oak_planks".into(),
            network_id: 1,
            component_based: false,
            version: ::protocol::ItemRegistryVersion::None,
            component_digest: [0; 32],
            negotiated_max_stack_size: Some(64),
            canonical_empty_component_data: true,
            item_tags: Arc::from([]),
        }]
        .into(),
    };
    runtime.publish_crafting_bootstrap(
        &mut player,
        Some(&registry),
        ::protocol::InventoryAuthority::Server,
    );
    let ledger = runtime.inventory_ledger_mut(&mut player);
    ledger.apply_registry(&registry);
    let mut slots =
        vec![NetworkItemStack::empty(); usize::from(::protocol::PLAYER_INVENTORY_SLOTS)];
    slots[0] = NetworkItemStack {
        network_id: 1,
        count: 1,
        stack_network_id: 10,
        ..NetworkItemStack::empty()
    };
    ledger.apply(&InventoryEvent::Content(InventoryContentEvent {
        container: ContainerIdentity::window(0),
        slots: slots.into(),
        storage_item: NetworkItemStack::empty(),
    }));
    ledger
        .begin_quick_move(inventory::inventory_ledger::InventoryTarget::Player(0))
        .unwrap();
    let mut presentation = furnace_presentation(&mut runtime);
    let icon = presentation.item_icon("custom:oak_planks", 0).unwrap();
    let ledger = runtime.inventory_ledger(&player);
    for slot in 0..::protocol::PLAYER_INVENTORY_SLOTS {
        presentation.hud_frame.inventory_icons.0[usize::from(slot)] =
            ledger.displayed_stack(slot).map(|_| icon);
    }
    for slot in 0..3 {
        presentation.hud_frame.storage_icons.0[slot] =
            ledger.storage_stack(slot as u8).map(|_| icon);
    }
    let input = presentation
        .build(
            &player,
            &runtime,
            0,
            [640, 480],
            DpiScale::new(1.0).unwrap(),
        )
        .unwrap();
    assert!(
        presentation.engine_container_frame().is_some(),
        "the original fixture uses the production JSON-UI container path"
    );
    let pixels = super::super::super::forms::snapshot::rasterize(&input);
    assert!(
        pixels.pixels().any(|pixel| pixel.0 == [230, 160, 45, 255]),
        "the occupied ledger cell draws its original item icon"
    );
    super::super::super::forms::snapshot::write(&input, "furnace-custom-fuel");
    assert!(
        ledger.storage_stack(1).is_none(),
        "unknown custom names do not establish fuel eligibility"
    );
    assert_eq!(
        ledger
            .displayed_stack(::protocol::HOTBAR_SLOT_COUNT)
            .unwrap()
            .network_id,
        1
    );
}
