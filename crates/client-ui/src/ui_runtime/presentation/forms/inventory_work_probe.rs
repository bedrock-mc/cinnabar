//! Probe: allocations per unchanged frame with the inventory's Blocks category open.

use std::sync::Arc;

use assets::{RuntimeEntityAssets, RuntimeEquipmentCatalog, RuntimeIconCatalog, carriers};
use protocol::test_support::recipes::{
    CraftingDataFixture, IngredientFixture, OutputFixture, RecipeFixture,
};
use protocol::{
    ContainerIdentity, ContainerOpenEvent, CreativeCategory, CreativeContentEvent, CreativeGroup,
    CreativeItem, InventoryContentEvent, InventoryEvent,
};

use crate::test_support::fixture_hud;
use crate::ui_runtime::UiRuntime;
use crate::ui_runtime::presentation::UiPresentationRuntime;

/// Presentation with the installed UI, icons and GUI block models, plus block item identifiers.
fn presentation() -> Option<(UiPresentationRuntime, Vec<Arc<str>>)> {
    let compiled = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(carriers::COMPILED_DIR);
    let read = |carrier: &carriers::Carrier| std::fs::read(compiled.join(carrier.output)).ok();
    let (Some(world), Some(entities), Some(icons), Some(equipment), Some(carrier)) = (
        read(&carriers::WORLD),
        read(&carriers::ENTITY),
        read(&carriers::ICON),
        read(&carriers::EQUIPMENT),
        ui_engine::test_support::carrier(),
    ) else {
        return None;
    };
    let world = assets::RuntimeAssets::decode(&world).ok()?;
    let entities = RuntimeEntityAssets::decode(&entities).ok()?;
    let icons = Arc::new(RuntimeIconCatalog::decode(&icons).ok()?);
    let equipment = Arc::new(RuntimeEquipmentCatalog::decode(&equipment).ok()?);
    let blocks = entities
        .item_visuals()
        .iter()
        .filter(|visual| {
            visual.key.metadata == 0
                && matches!(
                    visual.route,
                    assets::ItemVisualDefinitionRoute::BlockItem { .. }
                )
        })
        .map(|visual| Arc::from(&*visual.key.identifier))
        .collect();
    let mut presentation = UiPresentationRuntime::with_hud_and_icons(
        ui_engine::test_support::fixture_font(),
        fixture_hud(),
        icons,
    )
    .ok()?;
    presentation.enable_json_ui(carrier).ok()?;
    presentation.set_equipment_catalog(Some(equipment));
    presentation.set_gui_models(&world, &entities).ok()?;
    Some((presentation, blocks))
}

/// Creative inventory whose Blocks tab lists `blocks`.
fn creative(blocks: &[Arc<str>]) -> (player_state::PlayerState, UiRuntime) {
    let mut player = player_state::PlayerState::new(1);
    let mut runtime = crate::test_support::inventory_session(&mut player);
    player
        .facts
        .publish_player_game_mode(protocol::PlayerGameMode::Creative);
    let registry = protocol::ItemRegistryEvent {
        entries: protocol::vanilla_item_registry(),
    };
    runtime
        .inventory_ledger_mut(&mut player)
        .apply_registry(&registry);
    let items: Vec<_> = registry
        .entries
        .iter()
        .filter(|entry| blocks.contains(&entry.identifier))
        .enumerate()
        .map(|(index, entry)| CreativeItem {
            creative_network_id: index as u32 + 1,
            stack: protocol::NetworkItemStack {
                network_id: entry.network_id,
                count: 1,
                ..protocol::NetworkItemStack::empty()
            },
            group: 0,
        })
        .collect();
    runtime
        .inventory_ledger_mut(&mut player)
        .apply(&InventoryEvent::Creative(CreativeContentEvent {
            groups: Arc::from([CreativeGroup {
                category: CreativeCategory::Construction,
                name: "".into(),
                icon: None,
            }]),
            items: items.into(),
            skipped: 0,
        }));
    runtime.toggle_inventory(&mut player, None);
    runtime.screen_state_mut().select_tab(0);
    (player, runtime)
}

/// Survival inventory whose recipe book Blocks tab lists one log recipe per block item.
fn survival(blocks: &[Arc<str>]) -> (player_state::PlayerState, UiRuntime) {
    let mut player = player_state::PlayerState::new(1);
    let mut runtime = crate::test_support::inventory_session(&mut player);
    player
        .facts
        .publish_player_game_mode(protocol::PlayerGameMode::Survival);
    let registry = protocol::ItemRegistryEvent {
        entries: protocol::vanilla_item_registry(),
    };
    runtime.publish_crafting_bootstrap(
        &mut player,
        Some(&registry),
        protocol::InventoryAuthority::Server,
    );
    runtime
        .inventory_ledger_mut(&mut player)
        .apply_registry(&registry);
    let log = registry
        .entries
        .iter()
        .find(|entry| entry.identifier.as_ref() == "minecraft:oak_log")
        .unwrap()
        .network_id;
    let mut slots =
        vec![protocol::NetworkItemStack::empty(); usize::from(protocol::PLAYER_INVENTORY_SLOTS)];
    slots[0] = protocol::NetworkItemStack {
        network_id: log,
        stack_network_id: 101,
        count: 16,
        ..protocol::NetworkItemStack::empty()
    };
    runtime
        .inventory_ledger_mut(&mut player)
        .apply(&InventoryEvent::Content(InventoryContentEvent {
            container: ContainerIdentity::window(0),
            slots: slots.into(),
            storage_item: protocol::NetworkItemStack::empty(),
        }));
    let recipes = registry
        .entries
        .iter()
        .filter(|entry| blocks.contains(&entry.identifier))
        .enumerate()
        .map(|(index, entry)| RecipeFixture {
            net_id: index as u32 + 1,
            ..RecipeFixture::shapeless(
                &format!("probe:block_{index}"),
                vec![IngredientFixture::named("minecraft:oak_log", 0)],
                vec![OutputFixture::new(entry.network_id, 1)],
            )
        })
        .collect();
    runtime
        .enqueue_inventory_event(
            &mut player,
            1,
            1,
            InventoryEvent::Recipes(CraftingDataFixture::clearing(recipes).update()),
        )
        .unwrap();
    runtime.synchronize_crafting_frontier(&mut player, 1, Some((1, 0, Some(1))));
    runtime.drain_pending_inventory(&mut player);
    runtime.toggle_inventory(&mut player, None);
    runtime
        .inventory_ledger_mut(&mut player)
        .mark_transport_enqueued(0);
    runtime
        .enqueue_inventory_event(
            &mut player,
            1,
            2,
            InventoryEvent::Open(ContainerOpenEvent {
                container: ContainerIdentity::window(7),
                window_type: inventory::inventory_ledger::PERSONAL_INVENTORY_WINDOW_TYPE,
                position: [0, 64, 0],
                runtime_entity_id: -1,
            }),
        )
        .unwrap();
    runtime.synchronize_crafting_frontier(&mut player, 1, Some((1, 0, Some(2))));
    runtime.drain_pending_inventory(&mut player);
    runtime.screen_state_mut().select_tab(0);
    (player, runtime)
}

/// Captures comparable allocation counts without using timing as a CI assertion.
#[test]
fn inventory_work_probe() {
    if std::env::var_os("CINNABAR_INVENTORY_WORK_PROBE").is_none() {
        return;
    }
    for (name, setup) in [
        ("creative", creative as fn(&[Arc<str>]) -> _),
        ("survival", survival),
    ] {
        let Some((mut presentation, blocks)) = presentation() else {
            eprintln!("skipping inventory_work_probe: missing local carriers (make assets)");
            return;
        };
        let (player, runtime) = setup(&blocks);
        let dpi = ui::DpiScale::new(1.0).unwrap();
        let mut now = 0;
        for _ in 0..30 {
            now += 16;
            presentation
                .build(&player, &runtime, now, [1280, 720], dpi)
                .unwrap();
        }
        let (builds, redrawn) = (presentation.tree_builds, presentation.redrawn_nodes);
        let mut total = 0;
        let mut vertices = 0;
        for _ in 0..20 {
            now += 16;
            let (input, allocations) = test_alloc::count(|| {
                presentation
                    .build(&player, &runtime, now, [1280, 720], dpi)
                    .unwrap()
            });
            total += allocations;
            vertices = input.vertices.len();
        }
        eprintln!(
            "inventory-probe {name} blocks={} book_entries={} vertices={vertices} allocations_per_frame={} tree_builds={} redrawn_nodes={}",
            blocks.len(),
            runtime.book_recipes(&player, 0, usize::MAX).len(),
            total / 20,
            presentation.tree_builds - builds,
            presentation.redrawn_nodes - redrawn
        );
    }
}
