use super::*;
use crate::ui_runtime::{
    inventory_actions::recipe_book_entries, inventory_drag::PointerAction,
    presentation::screens::Widget,
};
use ::protocol::wire::valentine::bedrock::{codec::BedrockCodec, version::v1_26_51::*};

#[test]
fn furnace_recipe_panel_lists_outputs_and_routes_filter_tabs_and_search() {
    let mut player = player_state::PlayerState::new(1);
    let mut runtime = opened(&mut player, ::protocol::WINDOW_TYPE_FURNACE, 3);
    let names = [
        "minecraft:raw_iron",
        "minecraft:iron_ingot",
        "minecraft:cooked_beef",
        "minecraft:glass",
    ];
    let registry = ::protocol::ItemRegistryEvent {
        entries: names
            .into_iter()
            .enumerate()
            .map(|(index, name)| ::protocol::ItemRegistryEntry {
                identifier: name.into(),
                network_id: index as i32 + 1,
                component_based: false,
                version: ::protocol::ItemRegistryVersion::None,
                component_digest: [0; 32],
                negotiated_max_stack_size: Some(64),
                canonical_empty_component_data: true,
                item_tags: std::sync::Arc::from([]),
            })
            .collect::<Vec<_>>()
            .into(),
    };
    runtime.publish_crafting_bootstrap(
        &mut player,
        Some(&registry),
        ::protocol::InventoryAuthority::Server,
    );
    runtime
        .inventory_ledger_mut(&mut player)
        .apply_registry(&registry);
    let mut stacks =
        vec![NetworkItemStack::empty(); usize::from(::protocol::PLAYER_INVENTORY_SLOTS)];
    stacks[0] = NetworkItemStack {
        network_id: 1,
        count: 8,
        stack_network_id: 10,
        ..NetworkItemStack::empty()
    };
    runtime
        .inventory_ledger_mut(&mut player)
        .apply(&InventoryEvent::Content(InventoryContentEvent {
            container: ContainerIdentity::window(0),
            slots: stacks.into(),
            storage_item: NetworkItemStack::empty(),
        }));
    let packet = CraftingDataPacket {
        shapeless_recipes: [
            ("minecraft:raw_iron", 2),
            ("minecraft:beef", 3),
            ("minecraft:sand", 4),
        ]
        .into_iter()
        .enumerate()
        .map(|(index, (name, output))| ShapelessRecipePayload {
            ingredients: vec![CerealizerRecipeIngredientSerializedData {
                descriptor: vec![CerealizerRecipeIngredientSerializedDataDescriptorItem {
                    key: "name".into(),
                    value: name.into(),
                }],
                aux_value: 0,
                stack_size: 1,
            }],
            results: vec![CerealizerNetworkItemInstanceDescriptorSerializedData {
                id: output,
                stacksize: 1,
                auxvalue: 0,
                block_runtime_id: if output == 4 { 1 } else { 0 },
                user_data_buffer: vec![],
            }],
            tag: "furnace".into(),
            net_id: TypedServerNetIdstructRecipeNetIdTag {
                raw_id: index as u32 + 1,
            },
            ..Default::default()
        })
        .collect(),
        clear_recipes: true,
        ..Default::default()
    };
    let mut bytes = Vec::new();
    packet.encode(&mut bytes).unwrap();
    runtime
        .enqueue_inventory_event(
            &mut player,
            1,
            3,
            InventoryEvent::Recipes(::protocol::decode_recipe_update(&bytes).unwrap()),
        )
        .unwrap();
    runtime.synchronize_crafting_frontier(&mut player, 1, Some((1, 0, Some(3))));
    runtime.drain_pending_inventory(&mut player);
    let Some(mut presentation) =
        engine_presentation_with(super::super::super::forms::pack_harness::font())
    else {
        eprintln!(
            "skipping furnace_recipe_panel_lists_outputs_and_routes_filter_tabs_and_search: missing local UI carrier; make assets"
        );
        return;
    };
    let dpi = DpiScale::new(1.0).unwrap();
    presentation
        .build(&player, &runtime, 0, [1280, 720], dpi)
        .unwrap();
    runtime.perform_pointer_action(
        &mut player,
        PointerAction::Click(InventoryCellHit::Widget(Widget::InventoryLayout(2))),
    );
    runtime.screen_state_mut().recipe_filtering = Some(false);
    presentation
        .build(&player, &runtime, 0, [1280, 720], dpi)
        .unwrap();
    let frame = presentation.engine_container_frame().unwrap();
    assert!(
        frame
            .hits
            .iter()
            .any(|hit| hit.collection.as_deref() == Some("recipe_book")),
        "opened furnace panel must expose server recipe cells"
    );
    let hit = frame
        .hits
        .iter()
        .find(|hit| hit.control_name.as_deref() == Some("toggle.enable_filtering"))
        .expect("pinned furnace filter");
    let filter = presentation
        .engine_container_hit([
            (hit.rect.x + hit.rect.w / 2.0) as f32,
            (hit.rect.y + hit.rect.h / 2.0) as f32,
        ])
        .unwrap();
    assert_eq!(filter, InventoryCellHit::Widget(Widget::RecipeFilter));
    runtime.perform_pointer_action(&mut player, PointerAction::Click(filter));
    assert_eq!(
        recipe_book_entries(&player, &runtime)
            .iter()
            .map(|entry| entry.stack().network_id)
            .collect::<Vec<_>>(),
        [2]
    );
    runtime.perform_pointer_action(&mut player, PointerAction::Click(filter));
    for (tab, output) in [(1, 3), (2, 2), (3, 4)] {
        runtime.perform_pointer_action(
            &mut player,
            PointerAction::Click(InventoryCellHit::Widget(Widget::FurnaceTab(tab))),
        );
        assert_eq!(
            recipe_book_entries(&player, &runtime)
                .iter()
                .map(|entry| entry.stack().network_id)
                .collect::<Vec<_>>(),
            [output]
        );
    }
    runtime.perform_pointer_action(
        &mut player,
        PointerAction::Click(InventoryCellHit::Widget(Widget::FurnaceTab(4))),
    );
    runtime.screen_state_mut().search = "iron".into();
    assert_eq!(
        recipe_book_entries(&player, &runtime)
            .iter()
            .map(|entry| entry.stack().network_id)
            .collect::<Vec<_>>(),
        [2]
    );
    runtime.perform_pointer_action(
        &mut player,
        PointerAction::Click(InventoryCellHit::RecipeBook(0)),
    );
    assert_eq!(
        player
            .inventory
            .ledger()
            .storage_stack(0)
            .unwrap()
            .network_id,
        1
    );
    assert_eq!(
        recipe_book_entries(&player, &runtime).len(),
        1,
        "loaded furnace ingredient keeps its recipe listed"
    );
    runtime.perform_pointer_action(
        &mut player,
        PointerAction::Click(InventoryCellHit::Widget(Widget::InventoryLayout(1))),
    );
    assert!(
        !runtime.screen_state().text_focused(),
        "hidden furnace search must release the keyboard"
    );
}
