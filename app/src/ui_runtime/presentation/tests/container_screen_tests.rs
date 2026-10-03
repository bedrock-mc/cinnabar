//! Vanilla JSON-UI container screens: every inventory and station window draws
//! through the engine by default. `CINNABAR_FORM_SNAPSHOT_DIR` writes each as a
//! PNG for inspection. Needs the gitignored UI carrier; skips when absent.

use protocol::{
    ContainerIdentity, ContainerOpenEvent, InventoryAuthority, InventoryContentEvent,
    InventoryEvent, NetworkItemStack,
};

use super::engine_hud_tests::engine_presentation_with;
use super::*;
use crate::ui_runtime::presentation::inventory_pointer::InventoryCellHit;

/// A server-authoritative session with the local language table, when built.
fn session(player_runtime: &mut crate::player_runtime::PlayerRuntime) -> UiRuntime {
    *player_runtime = crate::player_runtime::PlayerRuntime::new(1);
    let mut runtime = UiRuntime::new(1);
    runtime.publish_inventory_authority(player_runtime, InventoryAuthority::Server);
    runtime
        .publish_local_runtime_id(player_runtime, 1, 42)
        .unwrap();
    let lang = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../.local/assets/compiled/vanilla-v1.mcbelang");
    if let Some(lang) = std::fs::read(lang)
        .ok()
        .and_then(|bytes| assets::RuntimeLangCatalog::decode(&bytes).ok())
    {
        runtime.set_lang_catalog(std::sync::Arc::new(lang));
    }
    runtime
}

fn opened(
    player_runtime: &mut crate::player_runtime::PlayerRuntime,
    window_type: i8,
    cells: usize,
) -> UiRuntime {
    let mut runtime = session(player_runtime);
    // Chest-like windows name their content by the level-entity container.
    let generic = protocol::WindowKind::from_window_type(window_type)
        .and_then(protocol::WindowKind::open_cells)
        .is_some_and(|cells| matches!(cells, protocol::OpenCells::Generic(_)));
    let content = ContainerIdentity {
        slot_type: generic.then_some(protocol::CONTAINER_NAME_LEVEL_ENTITY),
        ..ContainerIdentity::window(7)
    };
    runtime
        .enqueue_inventory_event(
            player_runtime,
            1,
            1,
            InventoryEvent::Open(ContainerOpenEvent {
                container: ContainerIdentity::window(7),
                window_type,
                position: [0, 64, 0],
                runtime_entity_id: -1,
            }),
        )
        .unwrap();
    if cells > 0 {
        runtime
            .enqueue_inventory_event(
                player_runtime,
                1,
                2,
                InventoryEvent::Content(InventoryContentEvent {
                    container: content,
                    slots: vec![NetworkItemStack::empty(); cells].into(),
                    storage_item: NetworkItemStack::empty(),
                }),
            )
            .unwrap();
    }
    runtime.drain_pending_inventory(player_runtime);
    runtime
}

/// `runtime` with the open window's `ContainerSetData` properties applied.
fn with_data(
    mut runtime: UiRuntime,
    player_runtime: &mut crate::player_runtime::PlayerRuntime,
    properties: &[(i32, i32)],
) -> UiRuntime {
    for &(property, value) in properties {
        runtime
            .inventory_ledger_mut(player_runtime)
            .apply(&InventoryEvent::Data(protocol::ContainerDataEvent {
                container: ContainerIdentity::window(7),
                property,
                value,
            }));
    }
    runtime
}

/// Three offered options costing 1, 5 and 30 levels.
fn with_enchant_options(
    mut runtime: UiRuntime,
    player_runtime: &mut crate::player_runtime::PlayerRuntime,
) -> UiRuntime {
    let option = |cost: u8, network_id: u32| protocol::EnchantOption {
        cost,
        name: "abc def".into(),
        network_id,
        enchants: vec![(9, cost.min(5))].into(),
    };
    runtime
        .inventory_ledger_mut(player_runtime)
        .apply(&InventoryEvent::EnchantOptions(
            protocol::EnchantOptionsEvent {
                options: vec![option(1, 1), option(5, 2), option(30, 3)].into(),
            },
        ));
    runtime
}

/// The creative inventory over a 300-item catalog across the four tabs, whose
/// first construction items fold into a named group, the second one unfolded.
fn creative(player_runtime: &mut crate::player_runtime::PlayerRuntime) -> UiRuntime {
    creative_with(player_runtime, 300)
}

pub(super) fn creative_with(
    player_runtime: &mut crate::player_runtime::PlayerRuntime,
    count: u32,
) -> UiRuntime {
    use protocol::{CreativeCategory, CreativeContentEvent, CreativeGroup, CreativeItem};
    let mut runtime = session(player_runtime);
    runtime.publish_player_game_mode(player_runtime, protocol::PlayerGameMode::Creative);
    let categories = [
        CreativeCategory::Construction,
        CreativeCategory::Nature,
        CreativeCategory::Equipment,
        CreativeCategory::Items,
    ];
    let group = |category: CreativeCategory, name: &str| CreativeGroup {
        category,
        name: name.into(),
        icon: None,
    };
    let mut groups: Vec<CreativeGroup> = categories
        .iter()
        .map(|category| group(*category, ""))
        .collect();
    groups.push(group(
        CreativeCategory::Construction,
        "itemGroup.name.planks",
    ));
    groups.push(group(
        CreativeCategory::Construction,
        "itemGroup.name.stone",
    ));
    let items = (0..count)
        .map(|index| CreativeItem {
            creative_network_id: index + 1,
            stack: NetworkItemStack {
                network_id: 1 + index as i32,
                count: 1,
                ..NetworkItemStack::empty()
            },
            group: match index {
                0..20 if index % 4 == 0 => 4,
                20..40 if index % 4 == 0 => 5,
                _ => index % 4,
            },
        })
        .collect::<Vec<_>>();
    runtime
        .inventory_ledger_mut(player_runtime)
        .apply(&InventoryEvent::Creative(CreativeContentEvent {
            groups: groups.into(),
            items: items.into(),
            skipped: 0,
        }));
    runtime.screen_state_mut().creative_expanded.insert(5);
    runtime.toggle_inventory(player_runtime);
    runtime
}

/// A writable three-page book, on its first spread or its signing cover.
fn book(player_runtime: &mut crate::player_runtime::PlayerRuntime, signing: bool) -> UiRuntime {
    use crate::ui_runtime::book_screen::{BookSource, BookState};
    let mut runtime = session(player_runtime);
    let pages = ["First page", "Second page", "Third"].map(str::to_owned);
    let mut state = BookState::new(
        BookSource::Held(0),
        pages.to_vec(),
        true,
        String::new(),
        "Steve".to_owned(),
    );
    state.signing = signing;
    if signing {
        state.title = "Book title".to_owned();
    }
    // The right page shows its edit controls, the left its edit button.
    state.editing = Some(1);
    runtime.open_book(state);
    runtime
}

fn personal(player_runtime: &mut crate::player_runtime::PlayerRuntime) -> UiRuntime {
    let mut runtime = session(player_runtime);
    runtime.toggle_inventory(player_runtime);
    runtime
}

/// Builds each screen with its own authoritative player session.
fn screen(
    name: &'static str,
    create: impl FnOnce(&mut crate::player_runtime::PlayerRuntime) -> UiRuntime,
    hits: Vec<InventoryCellHit>,
) -> (
    &'static str,
    crate::player_runtime::PlayerRuntime,
    UiRuntime,
    Vec<InventoryCellHit>,
) {
    let mut player_runtime = crate::player_runtime::PlayerRuntime::new(1);
    let runtime = create(&mut player_runtime);
    (name, player_runtime, runtime, hits)
}

/// Every screen the engine draws by default, by snapshot name, with the window
/// cells its item slots must address.
fn screens() -> Vec<(
    &'static str,
    crate::player_runtime::PlayerRuntime,
    UiRuntime,
    Vec<InventoryCellHit>,
)> {
    use crate::ui_runtime::presentation::screens::{ReaderButton as R, Widget as W};
    use InventoryCellHit::{
        Craft, CraftOutput, CreativeSearch, CreativeTab, RecipeBook, Storage, Widget,
    };
    use protocol::*;
    let storage = |count: u8| (0..count).map(Storage).collect::<Vec<_>>();
    vec![
        screen(
            "inventory",
            personal,
            vec![
                Craft(28),
                Craft(31),
                CraftOutput,
                Widget(W::InventoryLayout(2)),
            ],
        ),
        screen(
            "book",
            |player_runtime| book(player_runtime, false),
            [
                R::NextSpread,
                R::FocusPage(0),
                R::FocusPage(1),
                R::EditPage(0),
                R::InsertPage(1),
                R::DeletePage(1),
                R::SwapLeft(1),
                R::SwapRight(1),
                R::Sign,
                R::Done,
            ]
            .map(|button| Widget(W::Reader(button)))
            .to_vec(),
        ),
        screen(
            "book_signing",
            |player_runtime| book(player_runtime, true),
            vec![Widget(W::Reader(R::Finalize))],
        ),
        screen(
            "inventory_recipe_book",
            |player_runtime| {
                let mut runtime = personal(player_runtime);
                runtime.screen_state_mut().book_open = true;
                runtime
            },
            vec![
                Widget(W::InventoryLayout(1)),
                CreativeTab(1),
                CreativeSearch,
            ],
        ),
        screen(
            "inventory_recipe_search",
            |player_runtime| {
                let mut runtime = personal(player_runtime);
                runtime.screen_state_mut().book_open = true;
                runtime
                    .screen_state_mut()
                    .select_tab(crate::ui_runtime::presentation::screens::SEARCH_TAB);
                runtime
            },
            vec![Widget(W::RecipeFilter), CreativeSearch],
        ),
        screen(
            "creative",
            creative,
            vec![
                RecipeBook(0),
                RecipeBook(20),
                CreativeTab(2),
                CreativeSearch,
                Widget(W::InventoryLayout(3)),
            ],
        ),
        screen(
            "crafting_table",
            |player_runtime| opened(player_runtime, WINDOW_TYPE_WORKBENCH, 0),
            vec![Craft(32), Craft(40), CraftOutput],
        ),
        screen(
            "chest",
            |player_runtime| opened(player_runtime, WINDOW_TYPE_CONTAINER, 27),
            storage(27),
        ),
        screen(
            "large_chest",
            |player_runtime| opened(player_runtime, WINDOW_TYPE_CONTAINER, 54),
            storage(54),
        ),
        // Half cooked, fuel half burnt.
        screen(
            "furnace",
            |player_runtime| {
                with_data(
                    opened(player_runtime, WINDOW_TYPE_FURNACE, 3),
                    player_runtime,
                    &[(0, 100), (1, 50), (2, 100)],
                )
            },
            storage(3),
        ),
        screen(
            "blast_furnace",
            |player_runtime| opened(player_runtime, WINDOW_TYPE_BLAST_FURNACE, 3),
            storage(3),
        ),
        screen(
            "smoker",
            |player_runtime| opened(player_runtime, WINDOW_TYPE_SMOKER, 3),
            storage(3),
        ),
        // Half brewed, half the fuel left.
        screen(
            "brewing_stand",
            |player_runtime| {
                with_data(
                    opened(player_runtime, WINDOW_TYPE_BREWING_STAND, 5),
                    player_runtime,
                    &[(0, 200), (1, 10), (2, 20)],
                )
            },
            storage(5),
        ),
        screen(
            "anvil",
            |player_runtime| opened(player_runtime, WINDOW_TYPE_ANVIL, 0),
            vec![Craft(1), Craft(2), CraftOutput, Widget(W::AnvilName)],
        ),
        screen(
            "enchanting_table",
            |player_runtime| {
                with_enchant_options(
                    opened(player_runtime, WINDOW_TYPE_ENCHANTMENT, 0),
                    player_runtime,
                )
            },
            vec![Craft(14), Craft(15)],
        ),
        screen(
            "grindstone",
            |player_runtime| opened(player_runtime, WINDOW_TYPE_GRINDSTONE, 0),
            vec![Craft(16), Craft(17), CraftOutput],
        ),
        screen(
            "loom",
            |player_runtime| opened(player_runtime, WINDOW_TYPE_LOOM, 0),
            vec![Craft(9), Craft(10), Craft(11), CraftOutput],
        ),
        screen(
            "smithing_table",
            |player_runtime| opened(player_runtime, WINDOW_TYPE_SMITHING_TABLE, 0),
            vec![Craft(51), Craft(52), Craft(53), CraftOutput],
        ),
        screen(
            "cartography_table",
            |player_runtime| opened(player_runtime, WINDOW_TYPE_CARTOGRAPHY, 0),
            vec![Craft(12), Craft(13), CraftOutput],
        ),
        screen(
            "stonecutter",
            |player_runtime| opened(player_runtime, WINDOW_TYPE_STONECUTTER, 0),
            vec![Craft(3), CraftOutput],
        ),
        screen(
            "beacon",
            |player_runtime| {
                let mut runtime = opened(player_runtime, WINDOW_TYPE_BEACON, 0);
                runtime.screen_state_mut().beacon = (3, 0);
                runtime
            },
            vec![
                Craft(27),
                Widget(W::BeaconEffect {
                    id: 1,
                    secondary: false,
                }),
                Widget(W::BeaconEffect {
                    id: 10,
                    secondary: true,
                }),
                Widget(W::BeaconUpgrade),
                Widget(W::BeaconConfirm),
            ],
        ),
        screen(
            "hopper",
            |player_runtime| opened(player_runtime, WINDOW_TYPE_HOPPER, 5),
            storage(5),
        ),
        screen(
            "dispenser",
            |player_runtime| opened(player_runtime, WINDOW_TYPE_DISPENSER, 9),
            storage(9),
        ),
        screen(
            "dropper",
            |player_runtime| opened(player_runtime, WINDOW_TYPE_DROPPER, 9),
            storage(9),
        ),
        screen(
            "crafter",
            |player_runtime| opened(player_runtime, WINDOW_TYPE_CRAFTER, 9),
            storage(9),
        ),
        screen(
            "horse",
            |player_runtime| opened(player_runtime, WINDOW_TYPE_HORSE, 17),
            storage(17),
        ),
        // A chested llama wears only a carpet, in container slot 1.
        screen(
            "llama",
            |player_runtime| {
                let mut runtime = opened(player_runtime, WINDOW_TYPE_HORSE, 17);
                runtime.screen_state_mut().mount_identifier = Some("minecraft:llama".into());
                runtime
            },
            (1..17).map(Storage).collect(),
        ),
    ]
}

// Every container screen draws through the engine, and a click on each of its
// window cells and player slots reaches that ledger cell.
#[test]
fn every_container_screen_draws_through_the_engine() {
    let only = std::env::var("CINNABAR_CONTAINER_SCREEN").ok();
    for (name, player_runtime, runtime, expected) in screens() {
        if only.as_deref().is_some_and(|only| only != name) {
            continue;
        }
        let Some(mut presentation) =
            engine_presentation_with(super::super::forms::pack_harness::font())
        else {
            eprintln!(
                "skipping every_container_screen_draws_through_the_engine: fixture unavailable; requires installed local carriers (make assets) and CINNABAR_CONTAINER_SCREEN"
            );
            return;
        };
        assert!(runtime.inventory_open(), "{name}");
        // Textures publish during the first builds.
        let dpi = DpiScale::new(1.0).unwrap();
        for now in [0, 500] {
            presentation
                .build(&player_runtime, &runtime, now, [1280, 720], dpi)
                .unwrap();
        }
        let input = presentation
            .build(&player_runtime, &runtime, 5_000, [1280, 720], dpi)
            .unwrap();
        super::super::forms::snapshot::write(&input, &format!("container-{name}"));
        let frame = presentation
            .engine_container_frame()
            .unwrap_or_else(|| panic!("{name} is not engine-drawn"));
        let reached: Vec<InventoryCellHit> = frame
            .hits
            .iter()
            .filter_map(|region| {
                let center = [
                    (region.rect.x + region.rect.w / 2.0) as f32,
                    (region.rect.y + region.rect.h / 2.0) as f32,
                ];
                presentation.engine_container_hit(center)
            })
            .collect();
        // Every screen but the book shows the player's inventory.
        let player_cells = if name.starts_with("book") { 0 } else { 36 };
        let player = (0..player_cells).map(InventoryCellHit::Player);
        for hit in expected.into_iter().chain(player) {
            assert!(reached.contains(&hit), "{name}: {hit:?} unreachable");
        }
    }
}

// A disabled crafter slot shows its button over the cell; pressing it asks
// the server to re-enable the slot, and clicking an empty slot disables it.
#[test]
fn crafter_slots_toggle_through_their_buttons() {
    let mut player_runtime = crate::player_runtime::PlayerRuntime::new(1);

    use crate::ui_runtime::inventory_drag::PointerAction;
    use crate::ui_runtime::presentation::screens::Widget as W;
    let Some(mut presentation) =
        engine_presentation_with(super::super::forms::pack_harness::font())
    else {
        eprintln!(
            "skipping crafter_slots_toggle_through_their_buttons: fixture unavailable; requires installed local carriers (make assets)"
        );
        return;
    };
    let mut runtime = opened(&mut player_runtime, protocol::WINDOW_TYPE_CRAFTER, 9);
    runtime.screen_state_mut().crafter.observe(0b101, true, 0);
    let dpi = DpiScale::new(1.0).unwrap();
    for now in [0, 500] {
        presentation
            .build(&player_runtime, &runtime, now, [1280, 720], dpi)
            .unwrap();
    }
    let frame = presentation.engine_container_frame().unwrap();
    let reached: Vec<InventoryCellHit> = frame
        .hits
        .iter()
        .filter_map(|region| {
            let center = [
                (region.rect.x + region.rect.w / 2.0) as f32,
                (region.rect.y + region.rect.h / 2.0) as f32,
            ];
            presentation.engine_container_hit(center)
        })
        .collect();
    for hit in [
        InventoryCellHit::Widget(W::CrafterSlot(0)),
        InventoryCellHit::Widget(W::CrafterSlot(2)),
        InventoryCellHit::Storage(1),
    ] {
        assert!(reached.contains(&hit), "{hit:?} unreachable");
    }
    assert!(!reached.contains(&InventoryCellHit::Storage(0)));
    runtime.perform_pointer_action(
        &mut player_runtime,
        PointerAction::Click(InventoryCellHit::Widget(W::CrafterSlot(0))),
    );
    runtime.perform_pointer_action(
        &mut player_runtime,
        PointerAction::Click(InventoryCellHit::Storage(4)),
    );
    assert_eq!(runtime.screen_state().crafter.shown_disabled(), 0b1_0100);
    let toggles: Vec<String> = std::iter::from_fn(|| runtime.take_client_packet())
        .map(|packet| format!("{:?}", packet.data))
        .collect();
    assert_eq!(toggles.len(), 2);
    assert!(toggles[0].contains("slot_index: 0") && toggles[0].contains("is_disabled: false"));
    assert!(toggles[1].contains("slot_index: 4") && toggles[1].contains("is_disabled: true"));
}

// A llama's single equip cell is its carpet slot, never the saddle's.
#[test]
fn llama_equip_cell_addresses_the_carpet_slot() {
    let mut player_runtime = crate::player_runtime::PlayerRuntime::new(1);

    let Some(mut presentation) =
        engine_presentation_with(super::super::forms::pack_harness::font())
    else {
        eprintln!(
            "skipping llama_equip_cell_addresses_the_carpet_slot: fixture unavailable; requires installed local carriers (make assets)"
        );
        return;
    };
    let mut runtime = opened(&mut player_runtime, protocol::WINDOW_TYPE_HORSE, 17);
    runtime.screen_state_mut().mount_identifier = Some("minecraft:llama".into());
    let dpi = DpiScale::new(1.0).unwrap();
    for now in [0, 500] {
        presentation
            .build(&player_runtime, &runtime, now, [1280, 720], dpi)
            .unwrap();
    }
    let frame = presentation.engine_container_frame().unwrap();
    let reached: Vec<InventoryCellHit> = frame
        .hits
        .iter()
        .filter_map(|region| {
            let center = [
                (region.rect.x + region.rect.w / 2.0) as f32,
                (region.rect.y + region.rect.h / 2.0) as f32,
            ];
            presentation.engine_container_hit(center)
        })
        .collect();
    assert!(reached.contains(&InventoryCellHit::Storage(1)));
    assert!(!reached.contains(&InventoryCellHit::Storage(0)));
}

// Creative's wide list drops the player inventory for the catalog and keeps
// the hotbar beneath it; its toggles pick each layout.
#[test]
fn creative_wide_layout_keeps_only_the_hotbar_under_the_catalog() {
    let mut player_runtime = crate::player_runtime::PlayerRuntime::new(1);

    use crate::ui_runtime::presentation::screens::Widget as W;
    let Some(mut presentation) =
        engine_presentation_with(super::super::forms::pack_harness::font())
    else {
        eprintln!(
            "skipping creative_wide_layout_keeps_only_the_hotbar_under_the_catalog: fixture unavailable; requires installed local carriers (make assets)"
        );
        return;
    };
    let mut runtime = creative(&mut player_runtime);
    runtime.screen_state_mut().creative_wide = true;
    let dpi = DpiScale::new(1.0).unwrap();
    for now in [0, 500] {
        presentation
            .build(&player_runtime, &runtime, now, [1280, 720], dpi)
            .unwrap();
    }
    let input = presentation
        .build(&player_runtime, &runtime, 5_000, [1280, 720], dpi)
        .unwrap();
    super::super::forms::snapshot::write(&input, "container-creative_wide");
    let frame = presentation.engine_container_frame().unwrap();
    let reached: Vec<InventoryCellHit> = frame
        .hits
        .iter()
        .filter_map(|region| {
            let center = [
                (region.rect.x + region.rect.w / 2.0) as f32,
                (region.rect.y + region.rect.h / 2.0) as f32,
            ];
            presentation.engine_container_hit(center)
        })
        .collect();
    for hit in [
        InventoryCellHit::RecipeBook(0),
        InventoryCellHit::Player(0),
        InventoryCellHit::Player(8),
        InventoryCellHit::Widget(W::InventoryLayout(2)),
    ] {
        assert!(reached.contains(&hit), "{hit:?} unreachable");
    }
    assert!(!reached.contains(&InventoryCellHit::Player(9)));
}

// Moving the pointer across slots only changes which hover states show: the
// screen never lays out again, however long the creative catalog.
#[test]
fn hovering_slots_never_lays_the_screen_out_again() {
    let mut player_runtime = crate::player_runtime::PlayerRuntime::new(1);

    let Some(mut presentation) =
        engine_presentation_with(super::super::forms::pack_harness::font())
    else {
        eprintln!(
            "skipping hovering_slots_never_lays_the_screen_out_again: fixture unavailable; requires installed local carriers (make assets)"
        );
        return;
    };
    let mut runtime = creative_with(&mut player_runtime, 1500);
    let dpi = DpiScale::new(1.0).unwrap();
    for now in [0, 500] {
        presentation
            .build(&player_runtime, &runtime, now, [1280, 720], dpi)
            .unwrap();
    }
    let layouts = presentation.engine_container_layouts();
    let hits = std::sync::Arc::clone(&presentation.engine_container_frame().unwrap().hits);
    let mut frames = Vec::new();
    for frame in 0..bench_frames(24) {
        let step = frame % 24;
        let point = [
            60.0 + (step % 8) as f32 * 18.0,
            70.0 + (step / 8) as f32 * 18.0,
        ];
        runtime.set_inventory_pointer_gui(Some(point));
        let started = std::time::Instant::now();
        presentation
            .build(&player_runtime, &runtime, 1_000 + frame, [1280, 720], dpi)
            .unwrap();
        frames.push(started.elapsed());
        assert!(std::sync::Arc::ptr_eq(
            &hits,
            &presentation.engine_container_frame().unwrap().hits
        ));
    }
    frames.sort();
    eprintln!(
        "hover frame with 1500 catalog entries: median {:?}, fastest {:?}",
        frames[frames.len() / 2],
        frames[0]
    );
    assert_eq!(presentation.engine_container_layouts(), layouts);
}

// Scrolling the creative catalog lays out only what the viewport shows.
#[test]
fn scrolling_the_creative_catalog_stays_interactive() {
    let mut player_runtime = crate::player_runtime::PlayerRuntime::new(1);

    let Some(mut presentation) =
        engine_presentation_with(super::super::forms::pack_harness::font())
    else {
        eprintln!(
            "skipping scrolling_the_creative_catalog_stays_interactive: fixture unavailable; requires installed local carriers (make assets)"
        );
        return;
    };
    let mut runtime = creative_with(&mut player_runtime, 1500);
    let dpi = DpiScale::new(1.0).unwrap();
    presentation
        .build(&player_runtime, &runtime, 0, [1280, 720], dpi)
        .unwrap();
    let frame = presentation.engine_container_frame().unwrap();
    let (key, metrics) = frame
        .report
        .scrolls
        .iter()
        .max_by(|a, b| a.1.content.total_cmp(&b.1.content))
        .map(|(key, metrics)| (key.clone(), metrics.clone()))
        .unwrap();
    assert!(metrics.max_offset() > 700.0, "{metrics:?}");
    let mut frames = Vec::new();
    let samples = bench_frames(12);
    for frame in 1..=samples {
        let step = (frame - 1) % 12 + 1;
        runtime
            .screen_state_mut()
            .container_scroll
            .insert(key.clone(), step as f64 * 60.0);
        let started = std::time::Instant::now();
        presentation
            .build(&player_runtime, &runtime, frame, [1280, 720], dpi)
            .unwrap();
        frames.push(started.elapsed());
    }
    frames.sort();
    eprintln!(
        "scroll frame with 1500 catalog entries: median {:?}, fastest {:?}",
        frames[frames.len() / 2],
        frames[0]
    );
    let frame = presentation.engine_container_frame().unwrap();
    assert_eq!(
        frame.report.scrolls[&key].offset,
        ((samples - 1) % 12 + 1) as f64 * 60.0
    );
}

// The inventory's live player renderer faces the viewer and turns toward the
// pointer: pointers on either side of the model draw different rasters.
#[test]
fn inventory_player_model_turns_toward_the_pointer() {
    let mut player_runtime = crate::player_runtime::PlayerRuntime::new(1);

    let Some(mut presentation) =
        engine_presentation_with(super::super::forms::pack_harness::font())
    else {
        return;
    };
    let mut runtime = personal(&mut player_runtime);
    let dpi = DpiScale::new(1.0).unwrap();
    let skin = steve_skin();
    let mut rasters = Vec::new();
    for (name, pointer) in [
        ("left", [40.0, 60.0]),
        ("centre", [178.0, 70.0]),
        ("right", [400.0, 60.0]),
    ] {
        runtime.set_inventory_pointer_gui(Some(pointer));
        for now in [0, 500] {
            presentation
                .build(&player_runtime, &runtime, now, [1280, 720], dpi)
                .unwrap();
            presentation.sync_player_preview(skin.as_deref(), Default::default(), true, false, 0.0);
            presentation.hud_frame_mut().player_preview = presentation.player_preview_icon();
        }
        let input = presentation
            .build(&player_runtime, &runtime, 1_000, [1280, 720], dpi)
            .unwrap();
        super::super::forms::snapshot::write(&input, &format!("doll-{name}"));
        rasters.push(presentation.player_preview_raster());
        if let Ok(dir) = std::env::var("CINNABAR_FORM_SNAPSHOT_DIR") {
            let raster = rasters.last().unwrap().clone();
            image::RgbaImage::from_raw(96, 112, raster)
                .unwrap()
                .save(format!("{dir}/raster-{name}.png"))
                .unwrap();
        }
    }
    assert!(
        rasters
            .iter()
            .all(|raster| raster.iter().any(|byte| *byte != 0))
    );
    assert_ne!(rasters[0], rasters[2], "the model turns with the pointer");
}

/// Steve from the local vanilla pack, else `None` (the built-in skin).
fn steve_skin() -> Option<Vec<u8>> {
    let steve = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(
        "../.local/assets/bedrock-samples/v1.26.30.32-preview/full/resource_pack/textures/entity/steve.png",
    );
    image::open(steve)
        .ok()
        .map(|image| image.to_rgba8().into_raw())
}

// The pause screen's paper doll shows the model from the front, turned by its
// starting rotation, not the player's world facing.
#[test]
fn pause_paper_doll_faces_the_viewer() {
    let mut player_runtime = crate::player_runtime::PlayerRuntime::new(1);

    let Some(mut presentation) =
        engine_presentation_with(super::super::forms::pack_harness::font())
    else {
        return;
    };
    let mut menu = crate::menu::MenuRuntime::new(true, 2, "Player".to_owned());
    menu.mark_connected();
    menu.open_pause();
    presentation.set_menu_view(Some(menu.view()));
    let runtime = session(&mut player_runtime);
    let skin = steve_skin();
    let dpi = DpiScale::new(1.0).unwrap();
    for now in [0, 500] {
        presentation
            .build(&player_runtime, &runtime, now, [1280, 720], dpi)
            .unwrap();
        // The world facing must not turn the doll.
        let pose = super::super::player_preview::PlayerPreviewPose::new(97.0, 97.0, 0.0, false);
        presentation.sync_player_preview(skin.as_deref(), pose, true, false, 0.0);
        presentation.hud_frame_mut().player_preview = presentation.player_preview_icon();
    }
    let input = presentation
        .build(&player_runtime, &runtime, 1_000, [1280, 720], dpi)
        .unwrap();
    super::super::forms::snapshot::write(&input, "pause-doll");
    assert!(
        presentation
            .player_preview_raster()
            .iter()
            .any(|byte| *byte != 0)
    );
}

/// A pack texture as preview art, when the local vanilla pack has it.
fn pack_texture(path: &str) -> Option<super::super::player_preview::PreviewTexture> {
    let file = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../.local/assets/bedrock-samples/v1.26.30.32-preview/full/resource_pack")
        .join(path);
    let image = image::open(file).ok()?.to_rgba8();
    Some(super::super::player_preview::PreviewTexture {
        width: image.width() as u16,
        height: image.height() as u16,
        rgba: image.into_raw().into(),
        tint: None,
    })
}

// Worn armor and the held item draw on the model over the bare skin.
#[test]
fn inventory_player_model_wears_armor_and_holds_items() {
    let mut player_runtime = crate::player_runtime::PlayerRuntime::new(1);

    let Some(mut presentation) =
        engine_presentation_with(super::super::forms::pack_harness::font())
    else {
        return;
    };
    let (Some(layer_1), Some(layer_2), Some(sword)) = (
        pack_texture("textures/models/armor/diamond_1.png"),
        pack_texture("textures/models/armor/diamond_2.png"),
        pack_texture("textures/items/diamond_sword.png"),
    ) else {
        return;
    };
    let mut runtime = personal(&mut player_runtime);
    runtime.set_inventory_pointer_gui(Some([260.0, 40.0]));
    let skin = steve_skin();
    let dpi = DpiScale::new(1.0).unwrap();
    let mut rasters = Vec::new();
    for gear in [
        super::super::player_preview::PreviewEquipment::default(),
        super::super::player_preview::PreviewEquipment {
            armor: [
                Some(layer_1.clone()),
                Some(layer_1.clone()),
                Some(layer_2),
                Some(layer_1),
            ],
            held: Some(sword),
            ..Default::default()
        },
    ] {
        presentation.player_preview_gear = gear;
        for now in [0, 500] {
            presentation
                .build(&player_runtime, &runtime, now, [1280, 720], dpi)
                .unwrap();
            presentation.sync_player_preview(skin.as_deref(), Default::default(), true, false, 0.0);
            presentation.hud_frame_mut().player_preview = presentation.player_preview_icon();
        }
        let input = presentation
            .build(&player_runtime, &runtime, 1_000, [1280, 720], dpi)
            .unwrap();
        let name = if rasters.is_empty() {
            "doll-bare"
        } else {
            "doll-armored"
        };
        super::super::forms::snapshot::write(&input, name);
        rasters.push(presentation.player_preview_raster());
    }
    assert_ne!(
        rasters[0], rasters[1],
        "armor and the held item change the model"
    );
}

/// Repeat the measured frames long enough for a sampling profiler when requested.
fn bench_frames(default: u64) -> u64 {
    std::env::var("CINNABAR_CONTAINER_BENCH_FRAMES")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
        .max(1)
}
