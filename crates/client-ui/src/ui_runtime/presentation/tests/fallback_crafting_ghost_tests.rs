use super::*;
use crate::ui_runtime::presentation::{
    forms::snapshot, inventory_pointer::InventoryScreen, screens,
};
use protocol::test_support::recipes::{
    CraftingDataFixture, IngredientFixture, OutputFixture, RecipeFixture, UnlockingFixture,
    recipe_unlock_event,
};
use protocol::{
    ContainerIdentity, ContainerOpenEvent, InventoryContentEvent, InventoryEvent, NetworkItemStack,
};

/// Opens an acknowledged crafting screen with one real input and one missing ingredient.
fn selected_recipe(
    workbench: bool,
    ordinary_result: bool,
) -> (player_state::PlayerState, UiRuntime) {
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
        unlocking: Some(UnlockingFixture {
            context: 1,
            ..Default::default()
        }),
        ..RecipeFixture::shaped(
            "test:fallback_ghosts",
            2,
            1,
            vec![IngredientFixture::named("minecraft:oak_log", 0), missing],
            vec![OutputFixture::new(output, 4)],
        )
    };
    let mut recipes = vec![recipe];
    if ordinary_result {
        recipes.push(RecipeFixture {
            net_id: 78,
            ..RecipeFixture::shaped(
                "test:ordinary_result",
                1,
                1,
                vec![IngredientFixture::named("minecraft:oak_log", 0)],
                vec![OutputFixture::new(output, 4)],
            )
        });
    }
    runtime
        .enqueue_inventory_event(
            &mut player,
            1,
            1,
            InventoryEvent::Recipes(CraftingDataFixture::clearing(recipes).update()),
        )
        .unwrap();
    runtime
        .enqueue_inventory_event(&mut player, 1, 2, recipe_unlock_event(0, &[]))
        .unwrap();
    runtime.synchronize_crafting_frontier(&mut player, 1, Some((1, 0, Some(2))));
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
            3,
            InventoryEvent::Open(ContainerOpenEvent {
                container: ContainerIdentity::window(7),
                window_type,
                position: [0, 64, 0],
                runtime_entity_id: -1,
            }),
        )
        .unwrap();
    runtime.synchronize_crafting_frontier(&mut player, 1, Some((1, 0, Some(3))));
    runtime.drain_pending_inventory(&mut player);
    let recipe = runtime.book_recipes(&player, 0, 1).pop().unwrap().recipe;
    assert_eq!(recipe.network_id(), 77);
    player.inventory.begin_book_placement(&recipe).unwrap();
    assert_eq!(
        matches!(
            player.inventory.crafting_match(),
            inventory::CraftGridMatch::Unique(_)
        ),
        ordinary_result
    );
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
    let mut entries: Vec<_> = items
        .iter()
        .enumerate()
        .map(|(index, (name, _))| assets::IconEntry {
            identifier: (*name).into(),
            metadata: 0,
            sprite: index as u32,
        })
        .collect();
    entries.sort_unstable_by(|left, right| {
        (&left.identifier, left.metadata).cmp(&(&right.identifier, right.metadata))
    });
    let icons = assets::RuntimeIconCatalog::decode(
        &assets::encode_icon_catalog([9; 32], &sprites, &entries).unwrap(),
    )
    .unwrap();
    UiPresentationRuntime::with_hud_and_icons(fixture_font(), fixture_hud(), Arc::new(icons))
        .unwrap()
}

/// Reads the engine's result-cell background through its public collection binding.
fn assert_engine_result(
    player: &player_state::PlayerState,
    runtime: &UiRuntime,
    frame: &HudFrame,
    workbench: bool,
    ordinary_result: bool,
) {
    use crate::ui_runtime::presentation::forms::containers::{ContainerAuthority, container_frame};
    use json_ui::{BindState, EmptyLibrary, ResolvedControl, bind_stateful};
    use serde_json::json;
    use ui_screens::containers::{ScreenLayout, screen_data};
    let source = ContainerAuthority::new(player, runtime);
    let layout = if workbench {
        ScreenLayout::Workbench { book: false }
    } else {
        ScreenLayout::Personal { book: false }
    };
    let data = screen_data(
        &source,
        &container_frame(frame),
        layout,
        "Crafting",
        &mut Vec::new(),
        (&mut None, &mut None),
    );
    let control = Arc::new(ResolvedControl {
        name: "result".into(), control_type: Some("panel".into()), base: None, unresolved_base: None,
        factory: None, children: Vec::new(),
        properties: [
            ("collection_name".into(), json!("crafting_output_items")),
            ("collection_index".into(), json!(0)),
            ("bindings".into(), json!([{
                "binding_type": "collection", "binding_collection_name": "crafting_output_items",
                "binding_name": "#container_item_background"
            }])),
        ].into(),
    });
    let (bound, notes) = bind_stateful(&control, &data, &EmptyLibrary, &mut BindState::new());
    assert!(notes.is_empty(), "{notes:?}");
    assert_eq!(
        bound.properties["#container_item_background"],
        json!(if ordinary_result { 0 } else { 3 })
    );
}

/// Checks ghost cell art, item icons and real input preservation in one fallback screen.
fn assert_ghost_cells(workbench: bool, ordinary_result: bool) {
    let (player, runtime) = selected_recipe(workbench, ordinary_result);
    let ledger = player.inventory.ledger();
    let first = ledger.crafting_grid().slots().next().unwrap();
    let real = ledger
        .target_stack(inventory::inventory_ledger::InventoryTarget::Craft(first))
        .unwrap();
    let missing = ledger.crafting_ghost_stack(first + 1).unwrap();
    let output = ledger.crafting_ghost_output().unwrap();
    assert_eq!(missing.count, 1);
    let mut presentation = presentation();
    let contents = ui_screens::inventory_capture::capture_contents(
        ledger,
        true,
        || player.inventory.crafting_match(),
        |stack, _| {
            ledger
                .negotiated_item_entry(stack.network_id)
                .and_then(|entry| presentation.item_icon(&entry.identifier, stack.metadata))
        },
        |_| None,
    );
    assert_eq!(
        &contents.crafting.output.as_ref().unwrap().1,
        output,
        "the ordinary and ghost results have exactly the same stack identity"
    );
    use ui_screens::inventory_capture::cache::{
        InventoryProjectionCache, ProjectionInput, ProjectionRefresh,
    };
    let mut cache = InventoryProjectionCache::default();
    // A recipe refresh must publish an origin change even when the result bytes are identical.
    for real in if ordinary_result {
        vec![false, true, false, true]
    } else {
        vec![false]
    } {
        let update = cache.capture(
            ProjectionInput {
                ledger,
                inventory_open: true,
                refresh: ProjectionRefresh {
                    recipes: true,
                    ..Default::default()
                },
                animation: Default::default(),
            },
            || {
                if real {
                    player.inventory.crafting_match()
                } else {
                    inventory::CraftGridMatch::NoMatch
                }
            },
            |stack, _| ui_screens::inventory_capture::cache::ProjectedIcon {
                icon: ledger
                    .negotiated_item_entry(stack.network_id)
                    .and_then(|entry| presentation.item_icon(&entry.identifier, stack.metadata)),
                dial: None,
            },
            |_, _| None,
            |_| None,
        );
        let cached = update
            .contents
            .expect("a changed result origin must be published")
            .crafting;
        assert_eq!(cached.output, contents.crafting.output);
        assert_eq!(cached.output_is_ghost, !real);
        if real == ordinary_result {
            assert_eq!(cached, contents.crafting);
        }
    }
    presentation.hud_frame_mut().crafting = contents.crafting;
    assert_engine_result(
        &player,
        &runtime,
        presentation.hud_frame(),
        workbench,
        ordinary_result,
    );
    let size = [1280, 720];
    let input = presentation
        .build(&player, &runtime, 500, size, DpiScale::new(1.0).unwrap())
        .unwrap();
    assert!(presentation.engine_container_frame().is_none());
    snapshot::write(
        &input,
        &format!(
            "fallback-{}-{}",
            if ordinary_result { "ordinary" } else { "ghost" },
            if workbench { "workbench" } else { "personal" }
        ),
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
        if ordinary_result {
            real_background
        } else {
            ghost_background
        },
        "result cell art retains the output's real or ghost origin"
    );
    assert_ne!(
        pixel([grid[0] + 26.0, grid[1] + 8.0]),
        ghost_background,
        "the missing ingredient icon is painted"
    );
    assert_ne!(
        // The personal inventory's book toggle covers the middle of this slot.
        pixel([grid[0] + 5.0, grid[1] + 8.0]),
        real_background,
        "the real input remains painted"
    );
    assert_eq!(real.count, 1);
}

#[test]
fn fallback_crafting_draws_personal_ghosts() {
    assert_ghost_cells(false, false);
}

#[test]
fn fallback_crafting_draws_workbench_ghosts() {
    assert_ghost_cells(true, false);
}

#[test]
fn fallback_crafting_draws_identical_real_personal_result_normally() {
    assert_ghost_cells(false, true);
}

#[test]
fn fallback_crafting_draws_identical_real_workbench_result_normally() {
    assert_ghost_cells(true, true);
}
