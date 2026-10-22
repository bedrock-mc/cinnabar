use super::*;
use crate::ui_runtime::presentation::{
    forms::snapshot, inventory_pointer::InventoryScreen, screens,
};
use protocol::test_support::recipes::{
    CraftingDataFixture, IngredientFixture, OutputFixture, RecipeFixture,
};
use protocol::{
    ContainerIdentity, ContainerOpenEvent, InventoryContentEvent, InventoryEvent, NetworkItemStack,
};

/// Opens an acknowledged crafting screen with one real input and one missing ingredient.
fn selected_recipe(workbench: bool) -> (player_state::PlayerState, UiRuntime) {
    let mut player = player_state::PlayerState::new(1);
    let mut runtime = crate::test_support::inventory_session(&mut player);
    player
        .facts
        .publish_player_game_mode(protocol::PlayerGameMode::Survival);
    let registry = protocol::ItemRegistryEvent {
        entries: protocol::vanilla_item_registry(),
    };
    let id = |name| {
        registry
            .entries
            .iter()
            .find(|entry| entry.identifier.as_ref() == name)
            .unwrap()
            .network_id
    };
    let log = id("minecraft:oak_log");
    let output = id("minecraft:oak_planks");
    runtime.publish_crafting_bootstrap(
        &mut player,
        Some(&registry),
        protocol::InventoryAuthority::Server,
    );
    let ledger = runtime.inventory_ledger_mut(&mut player);
    ledger.apply_registry(&registry);
    let mut slots = vec![NetworkItemStack::empty(); usize::from(protocol::PLAYER_INVENTORY_SLOTS)];
    slots[0] = NetworkItemStack {
        network_id: log,
        stack_network_id: 101,
        count: 1,
        ..NetworkItemStack::empty()
    };
    ledger.apply(&InventoryEvent::Content(InventoryContentEvent {
        container: ContainerIdentity::window(0),
        slots: slots.into(),
        storage_item: NetworkItemStack::empty(),
    }));
    let mut missing = IngredientFixture::named("minecraft:diamond", 0);
    missing.count = 2;
    let recipe = RecipeFixture {
        net_id: 77,
        ..RecipeFixture::shaped(
            "test:fallback_ghosts",
            2,
            1,
            vec![IngredientFixture::named("minecraft:oak_log", 0), missing],
            vec![OutputFixture::new(output, 4)],
        )
    };
    runtime
        .enqueue_inventory_event(
            &mut player,
            1,
            1,
            InventoryEvent::Recipes(CraftingDataFixture::clearing(vec![recipe]).update()),
        )
        .unwrap();
    runtime.synchronize_crafting_frontier(&mut player, 1, Some((1, 0, Some(1))));
    runtime.drain_pending_inventory(&mut player);
    let window_type = if workbench {
        protocol::WINDOW_TYPE_WORKBENCH
    } else {
        runtime.toggle_inventory(&mut player, None);
        assert!(
            runtime
                .inventory_ledger_mut(&mut player)
                .mark_transport_enqueued(0)
        );
        inventory::inventory_ledger::PERSONAL_INVENTORY_WINDOW_TYPE
    };
    runtime
        .enqueue_inventory_event(
            &mut player,
            1,
            2,
            InventoryEvent::Open(ContainerOpenEvent {
                container: ContainerIdentity::window(7),
                window_type,
                position: [0, 64, 0],
                runtime_entity_id: -1,
            }),
        )
        .unwrap();
    runtime.synchronize_crafting_frontier(&mut player, 1, Some((1, 0, Some(2))));
    runtime.drain_pending_inventory(&mut player);
    let recipe = runtime.book_recipes(&player, 0, 1).pop().unwrap().recipe;
    player.inventory.begin_book_placement(&recipe).unwrap();
    (player, runtime)
}

/// Provides original colored item squares with a transparent border for inspecting cell art.
fn presentation() -> UiPresentationRuntime {
    let items = [
        ("minecraft:oak_log", [139, 91, 52]),
        ("minecraft:diamond", [80, 220, 220]),
        ("minecraft:oak_planks", [211, 171, 105]),
    ];
    let sprites: Vec<_> = items
        .iter()
        .map(|(_, color)| {
            let mut rgba8 = vec![0; 16 * 16 * 4];
            for y in 3..13 {
                for x in 3..13 {
                    rgba8[(y * 16 + x) * 4..(y * 16 + x + 1) * 4]
                        .copy_from_slice(&[color[0], color[1], color[2], 255]);
                }
            }
            assets::IconSprite {
                width: 16,
                height: 16,
                rgba8: rgba8.into(),
            }
        })
        .collect();
    let entries: Vec<_> = items
        .iter()
        .enumerate()
        .map(|(index, (name, _))| assets::IconEntry {
            identifier: (*name).into(),
            metadata: 0,
            sprite: index as u32,
        })
        .collect();
    let icons = assets::RuntimeIconCatalog::decode(
        &assets::encode_icon_catalog([9; 32], &sprites, &entries).unwrap(),
    )
    .unwrap();
    UiPresentationRuntime::with_hud_and_icons(fixture_font(), fixture_hud(), Arc::new(icons))
        .unwrap()
}

/// Checks ghost cell art, item icons and real input preservation in one fallback screen.
fn assert_ghost_cells(workbench: bool) {
    let (player, runtime) = selected_recipe(workbench);
    let ledger = player.inventory.ledger();
    let first = ledger.crafting_grid().slots().next().unwrap();
    let real = ledger
        .target_stack(inventory::inventory_ledger::InventoryTarget::Craft(first))
        .unwrap();
    let missing = ledger.crafting_ghost_stack(first + 1).unwrap();
    let output = ledger.crafting_ghost_output().unwrap();
    assert_eq!(missing.count, 1);
    let mut presentation = presentation();
    for (index, name) in [(0, "minecraft:oak_log"), (1, "minecraft:diamond")] {
        presentation.hud_frame_mut().crafting.icons[index] = presentation.item_icon(name, 0);
    }
    presentation.hud_frame_mut().crafting.output = Some((
        presentation.item_icon("minecraft:oak_planks", 0),
        output.clone(),
    ));
    let size = [1280, 720];
    let input = presentation
        .build(&player, &runtime, 500, size, DpiScale::new(1.0).unwrap())
        .unwrap();
    assert!(presentation.engine_container_frame().is_none());
    snapshot::write(
        &input,
        if workbench {
            "fallback-ghost-workbench"
        } else {
            "fallback-ghost-personal"
        },
    );
    let raster = snapshot::rasterize(&input);
    let geometry = HudGeometry::new(
        size,
        1.0,
        presentation.safe_area,
        presentation.gui_scale_preference,
    )
    .unwrap();
    let screen = InventoryScreen::of_runtime(&player, &runtime);
    let origin = screens::panel_origin(screen, [geometry.gui_width, geometry.gui_height]);
    let (grid, result) = if workbench {
        (
            crate::ui_runtime::presentation::inventory_pointer::WORKBENCH_GRID,
            crate::ui_runtime::presentation::inventory_pointer::WORKBENCH_OUTPUT,
        )
    } else {
        ([98.0, 18.0], [152.0, 28.0])
    };
    let pixel = |at: [f32; 2]| {
        *raster.get_pixel(
            ((origin[0] + at[0]) * geometry.scale) as u32,
            ((origin[1] + at[1]) * geometry.scale) as u32,
        )
    };
    let real_background = pixel([grid[0] + 2.0, grid[1] + 2.0]);
    let ghost_background = pixel([grid[0] + 20.0, grid[1] + 2.0]);
    assert_ne!(
        ghost_background, real_background,
        "missing input has distinct cell art"
    );
    assert_eq!(
        pixel([result[0] + 2.0, result[1] + 2.0]),
        ghost_background,
        "ghost result uses the missing-input cell art"
    );
    assert_ne!(
        pixel([grid[0] + 26.0, grid[1] + 8.0]),
        ghost_background,
        "the missing ingredient icon is painted"
    );
    assert_ne!(
        pixel([grid[0] + 8.0, grid[1] + 8.0]),
        real_background,
        "the real input remains painted"
    );
    assert_eq!(real.count, 1);
    presentation.hud_frame_mut().crafting.output =
        Some((presentation.item_icon("minecraft:oak_log", 0), real.clone()));
    let normal = presentation
        .build(&player, &runtime, 500, size, DpiScale::new(1.0).unwrap())
        .unwrap();
    let normal = snapshot::rasterize(&normal);
    let result_background = normal.get_pixel(
        ((origin[0] + result[0] + 2.0) * geometry.scale) as u32,
        ((origin[1] + result[1] + 2.0) * geometry.scale) as u32,
    );
    assert_eq!(
        *result_background, real_background,
        "an ordinary result keeps its normal cell art"
    );
}

#[test]
fn fallback_crafting_draws_personal_ghosts() {
    assert_ghost_cells(false);
}

#[test]
fn fallback_crafting_draws_workbench_ghosts() {
    assert_ghost_cells(true);
}
