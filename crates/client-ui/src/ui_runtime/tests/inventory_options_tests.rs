use super::*;

#[test]
fn inventory_options_default_to_recipe_book_and_unfiltered_recipes() {
    let mut player = player_state::PlayerState::new(1);
    let runtime = crate::test_support::inventory_session(&mut player);
    assert!(super::super::presentation::forms::recipe_book_shown(
        &player, &runtime
    ));
    assert!(!runtime.recipe_filtering(&player));
}

use super::super::inventory_drag::PointerAction;
use protocol::{
    InventoryEvent, InventoryLayout, InventoryOptions, InventoryRightTab, InventoryTab,
};
use ui_screens::container_actions::{InventoryCellHit, Widget};

/// Delivers a wire-encoded preference tuple through normal inventory ingress.
fn receive(
    runtime: &mut UiRuntime,
    player: &mut player_state::PlayerState,
    sequence: u64,
    options: InventoryOptions,
) {
    let bytes = protocol::encode(&protocol::inventory_options_packet(options)).unwrap();
    let packet = protocol::decode_batch(bytes).unwrap().pop().unwrap();
    let Some(protocol::WorldEvent::Inventory(event)) =
        protocol::into_world_event(packet, 0).unwrap()
    else {
        panic!("inventory event")
    };
    runtime
        .enqueue_inventory_event(player, 1, sequence, event)
        .unwrap();
    runtime.drain_pending_inventory(player);
}

/// Presses the same widget dispatched by a container hit.
fn click(runtime: &mut UiRuntime, player: &mut player_state::PlayerState, widget: Widget) {
    runtime.perform_pointer_action(
        player,
        PointerAction::Click(InventoryCellHit::Widget(widget)),
    );
}

/// Opens a crafting table through server admission.
fn workbench(runtime: &mut UiRuntime, player: &mut player_state::PlayerState, sequence: u64) {
    runtime
        .enqueue_inventory_event(
            player,
            1,
            sequence,
            InventoryEvent::Open(protocol::ContainerOpenEvent {
                container: protocol::ContainerIdentity::window(7),
                window_type: protocol::WINDOW_TYPE_WORKBENCH,
                position: [0, 64, 0],
                runtime_entity_id: -1,
            }),
        )
        .unwrap();
    runtime.drain_pending_inventory(player);
}

#[test]
fn inventory_options_restore_without_echo_and_keep_context_layouts_separate() {
    let mut player = player_state::PlayerState::new(1);
    let mut runtime = crate::test_support::inventory_session(&mut player);
    let stored = InventoryOptions {
        left_tab: InventoryTab::Nature,
        right_tab: InventoryRightTab::Armor,
        filtering: true,
        inventory_layout: InventoryLayout::InventoryOnly,
        crafting_layout: InventoryLayout::RecipeBook,
    };
    receive(&mut runtime, &mut player, 1, stored);
    runtime.toggle_inventory(&mut player, None);
    assert_eq!(
        runtime.inventory_controller(&player).unwrap().options,
        stored
    );
    assert_eq!(runtime.creative_tab(&player), 1);
    assert!(runtime.recipe_filtering(&player));
    assert!(runtime.take_client_packet().is_none());
    click(&mut runtime, &mut player, Widget::InventoryLayout(2));
    let saved = InventoryOptions {
        inventory_layout: InventoryLayout::RecipeBook,
        ..stored
    };
    assert_eq!(
        runtime.take_client_packet(),
        Some(protocol::inventory_options_packet(saved))
    );
    runtime.close_inventory(&mut player);
    workbench(&mut runtime, &mut player, 2);
    click(&mut runtime, &mut player, Widget::InventoryLayout(1));
    assert_eq!(
        runtime.take_client_packet(),
        Some(protocol::inventory_options_packet(InventoryOptions {
            crafting_layout: InventoryLayout::InventoryOnly,
            ..saved
        }))
    );
    assert!(runtime.take_client_packet().is_none());
}

#[test]
fn inventory_options_controls_deduplicate_and_keep_false_state_changes_local() {
    let mut player = player_state::PlayerState::new(1);
    let mut runtime = crate::test_support::inventory_session(&mut player);
    runtime.toggle_inventory(&mut player, None);
    let before = runtime.inventory_controller(&player).unwrap();
    runtime.screen_state_mut().creative_row = 4;
    runtime.screen_state_mut().book_page = 3;
    runtime
        .screen_state_mut()
        .container_scroll
        .insert("recipe_list".into(), 12.0);
    click(&mut runtime, &mut player, Widget::InventoryLayout(2));
    runtime.perform_pointer_action(
        &mut player,
        PointerAction::Click(InventoryCellHit::CreativeSearch),
    );
    assert!(runtime.take_client_packet().is_none());
    assert_eq!(
        runtime.screen_state().creative_row,
        4,
        "reselecting a tab keeps its row"
    );
    assert_eq!(runtime.screen_state().book_page, 3);
    assert_eq!(
        runtime.screen_state().container_scroll.get("recipe_list"),
        Some(&12.0)
    );
    runtime
        .change_inventory_options(&mut player, false, |controller| {
            controller.options.left_tab = InventoryTab::Equipment
        })
        .unwrap();
    assert_eq!(
        player.inventory.inventory_options(),
        InventoryOptions::default()
    );
    assert!(runtime.take_client_packet().is_none());
    click(&mut runtime, &mut player, Widget::RecipeFilter);
    let saved = InventoryOptions {
        left_tab: InventoryTab::Equipment,
        filtering: true,
        ..before.options
    };
    assert_eq!(
        runtime.take_client_packet(),
        Some(protocol::inventory_options_packet(saved))
    );
    click(&mut runtime, &mut player, Widget::InventoryRightTab(3));
    let saved = InventoryOptions {
        right_tab: InventoryRightTab::Armor,
        ..saved
    };
    assert_eq!(
        runtime.take_client_packet(),
        Some(protocol::inventory_options_packet(saved))
    );
    click(&mut runtime, &mut player, Widget::InventoryRightTab(3));
    click(&mut runtime, &mut player, Widget::InventoryRightTab(1));
    click(&mut runtime, &mut player, Widget::InventoryLayout(3));
    assert_eq!(
        runtime.inventory_controller(&player).unwrap().options,
        saved
    );
    assert!(runtime.take_client_packet().is_none());
}

#[test]
fn inventory_options_incoming_updates_do_not_replace_open_controller_or_lose_last_queued_choice() {
    let mut player = player_state::PlayerState::new(1);
    let mut runtime = crate::test_support::inventory_session(&mut player);
    runtime.toggle_inventory(&mut player, None);
    let shown = runtime.inventory_controller(&player).unwrap();
    runtime.perform_pointer_action(
        &mut player,
        PointerAction::Click(InventoryCellHit::CreativeTab(3)),
    );
    let earlier = runtime.take_client_packet().unwrap();
    runtime.requeue_client_packet(earlier.clone());
    let received = InventoryOptions {
        left_tab: InventoryTab::Nature,
        ..shown.options
    };
    receive(&mut runtime, &mut player, 1, received);
    assert_eq!(
        runtime
            .inventory_controller(&player)
            .unwrap()
            .options
            .left_tab,
        InventoryTab::Items
    );
    assert_eq!(player.inventory.inventory_options(), received);
    runtime.perform_pointer_action(
        &mut player,
        PointerAction::Click(InventoryCellHit::CreativeTab(1)),
    );
    assert_eq!(runtime.take_client_packet(), Some(earlier));
    assert_eq!(
        runtime.take_client_packet(),
        Some(protocol::inventory_options_packet(received))
    );
    assert!(runtime.take_client_packet().is_none());
    let incoming = InventoryOptions {
        crafting_layout: InventoryLayout::Unknown(89),
        inventory_layout: InventoryLayout::InventoryOnly,
        ..received
    };
    receive(&mut runtime, &mut player, 2, incoming);
    assert_eq!(
        runtime.inventory_controller(&player).unwrap().options,
        received
    );
    click(&mut runtime, &mut player, Widget::InventoryLayout(2));
    assert!(runtime.take_client_packet().is_none());
    click(&mut runtime, &mut player, Widget::RecipeFilter);
    let saved = InventoryOptions {
        filtering: true,
        ..received
    };
    assert_eq!(
        runtime.take_client_packet(),
        Some(protocol::inventory_options_packet(saved))
    );
    assert_eq!(player.inventory.inventory_options(), saved);
    runtime.close_inventory(&mut player);
    receive(&mut runtime, &mut player, 3, incoming);
    runtime.toggle_inventory(&mut player, None);
    assert_eq!(
        runtime.inventory_controller(&player).unwrap().options,
        incoming
    );
}

#[test]
fn inventory_options_queue_rejection_and_session_reset_are_atomic() {
    let mut player = player_state::PlayerState::new(1);
    let mut runtime = crate::test_support::inventory_session(&mut player);
    runtime.toggle_inventory(&mut player, None);
    let shown = runtime.inventory_controller(&player).unwrap();
    runtime.screen_state_mut().creative_row = 4;
    runtime.screen_state_mut().book_page = 3;
    runtime
        .screen_state_mut()
        .container_scroll
        .insert("recipes".into(), 24.0);
    for _ in 0..super::super::book_screen::MAX_QUEUED_CLIENT_PACKETS {
        assert!(
            runtime.queue_client_packet(protocol::inventory_options_packet(
                InventoryOptions::default()
            ))
        );
    }
    runtime.perform_pointer_action(
        &mut player,
        PointerAction::Click(InventoryCellHit::CreativeTab(0)),
    );
    assert_eq!(runtime.inventory_controller(&player), Some(shown));
    assert_eq!(runtime.screen_state().creative_row, 4);
    assert_eq!(runtime.screen_state().book_page, 3);
    assert_eq!(runtime.screen_state().container_scroll["recipes"], 24.0);
    assert_eq!(
        player.inventory.inventory_options(),
        InventoryOptions::default()
    );
    assert!(!runtime.screen_state().search_focused);
    while runtime.take_client_packet().is_some() {}
    click(&mut runtime, &mut player, Widget::InventoryLayout(1));
    assert!(runtime.take_client_packet().is_some());
    player.begin_session(2);
    runtime.begin_session(2);
    assert!(runtime.screen_state().inventory.is_none());
    assert_eq!(
        player.inventory.inventory_options(),
        InventoryOptions::default()
    );
    assert!(runtime.take_client_packet().is_none());
    assert_eq!(
        runtime.inventory_controller(&player).unwrap().layout(),
        InventoryLayout::RecipeBook
    );
}

#[test]
fn inventory_options_creative_bypasses_effective_filter_and_workbench_rejects_wide_layout() {
    let mut player = player_state::PlayerState::new(1);
    let mut runtime = crate::test_support::inventory_session(&mut player);
    player
        .facts
        .publish_player_game_mode(protocol::PlayerGameMode::Creative);
    runtime.toggle_inventory(&mut player, None);
    assert_eq!(
        runtime.inventory_controller(&player).unwrap().layout(),
        InventoryLayout::RecipeBookOnly
    );
    click(&mut runtime, &mut player, Widget::RecipeFilter);
    assert!(
        runtime
            .inventory_controller(&player)
            .unwrap()
            .options
            .filtering
    );
    assert!(!runtime.recipe_filtering(&player));
    assert!(runtime.take_client_packet().is_some());
    runtime.close_inventory(&mut player);
    workbench(&mut runtime, &mut player, 1);
    let shown = runtime.inventory_controller(&player).unwrap();
    assert_eq!(shown.layout(), InventoryLayout::RecipeBook);
    click(&mut runtime, &mut player, Widget::InventoryLayout(3));
    assert_eq!(runtime.inventory_controller(&player), Some(shown));
    assert!(runtime.take_client_packet().is_none());
}

#[test]
fn inventory_options_unsaved_choices_retire_on_close_without_a_flush() {
    let mut player = player_state::PlayerState::new(1);
    let mut runtime = crate::test_support::inventory_session(&mut player);
    runtime.toggle_inventory(&mut player, None);
    runtime
        .change_inventory_options(&mut player, false, |controller| {
            controller.options.left_tab = InventoryTab::Nature
        })
        .unwrap();
    runtime.close_inventory(&mut player);
    assert!(runtime.screen_state().inventory.is_none());
    assert!(runtime.take_client_packet().is_none());
    runtime.toggle_inventory(&mut player, None);
    assert_eq!(
        runtime
            .inventory_controller(&player)
            .unwrap()
            .options
            .left_tab,
        InventoryTab::Search
    );
    assert_eq!(
        player.inventory.inventory_options(),
        InventoryOptions::default()
    );
}

#[test]
fn inventory_options_search_focus_does_not_follow_into_a_book_reader() {
    use crate::ui_runtime::book_screen::{BookSource, BookState};

    for editable in [false, true] {
        let mut player = player_state::PlayerState::new(1);
        let mut runtime = crate::test_support::inventory_session(&mut player);
        runtime.toggle_inventory(&mut player, None);
        runtime.perform_pointer_action(
            &mut player,
            PointerAction::Click(InventoryCellHit::CreativeSearch),
        );
        assert!(runtime.screen_state().search_focused);
        runtime.open_book(BookState::new(
            BookSource::Held(0),
            vec!["page".into()],
            editable,
            String::new(),
            String::new(),
        ));
        runtime.observe_inventory_window(&player);
        runtime.observe_inventory_search_focus(&player, true);
        assert!(!runtime.screen_state().search_focused);
        assert_eq!(runtime.screen_state().text_focused(), editable);
        assert_eq!(runtime.screen_state().search, "");
        assert!(runtime.take_client_packet().is_none());
    }
}

#[test]
fn inventory_options_closing_search_releases_keyboard_without_flushing_preferences() {
    for toggle in [false, true] {
        let mut player = player_state::PlayerState::new(1);
        let mut runtime = crate::test_support::inventory_session(&mut player);
        runtime.toggle_inventory(&mut player, None);
        runtime.perform_pointer_action(
            &mut player,
            PointerAction::Click(InventoryCellHit::CreativeSearch),
        );
        runtime.screen_state_mut().type_text("iron");
        assert!(runtime.screen_state().text_focused());
        if toggle {
            runtime.toggle_inventory(&mut player, None);
        } else {
            runtime.close_inventory(&mut player);
        }
        assert!(!runtime.inventory_open());
        assert!(!runtime.screen_state().text_focused());
        assert_eq!(runtime.screen_state().search, "iron");
        assert_eq!(
            player.inventory.inventory_options(),
            InventoryOptions::default()
        );
        assert!(runtime.take_client_packet().is_none());
    }
}

/// A server close releases personal search focus without relying on a storage generation change.
#[test]
fn inventory_options_server_close_releases_personal_search_focus() {
    let mut player = player_state::PlayerState::new(1);
    let mut runtime = crate::test_support::inventory_session(&mut player);
    runtime.toggle_inventory(&mut player, None);
    assert!(player.inventory.ledger_mut().mark_transport_enqueued(0));
    let container = protocol::ContainerIdentity::window(protocol::PLAYER_INVENTORY_WINDOW_ID);
    let window_type = inventory::inventory_ledger::PERSONAL_INVENTORY_WINDOW_TYPE;
    runtime
        .enqueue_inventory_event(
            &mut player,
            1,
            1,
            InventoryEvent::Open(protocol::ContainerOpenEvent {
                container,
                window_type,
                position: [0, 64, 0],
                runtime_entity_id: -1,
            }),
        )
        .unwrap();
    runtime.drain_pending_inventory(&mut player);
    runtime.perform_pointer_action(
        &mut player,
        PointerAction::Click(InventoryCellHit::CreativeSearch),
    );
    runtime.screen_state_mut().type_text("iron");
    assert!(runtime.screen_state().text_focused());
    assert_eq!(player.inventory.ledger().storage_generation(), None);
    runtime
        .enqueue_inventory_event(
            &mut player,
            1,
            2,
            InventoryEvent::Close(protocol::ContainerCloseEvent {
                container,
                window_type,
                server_initiated: true,
            }),
        )
        .unwrap();
    runtime.drain_pending_inventory(&mut player);
    assert!(!runtime.inventory_open());
    assert!(runtime.screen_state().inventory.is_none());
    assert!(!runtime.screen_state().text_focused());
    assert_eq!(runtime.screen_state().search, "iron");
    assert!(runtime.take_client_packet().is_none());
}

/// Captures the restored category with original fixture icons when snapshots are enabled.
fn snapshot_category(player: &player_state::PlayerState, runtime: &UiRuntime, name: &str) {
    use super::super::presentation::{UiPresentationRuntime, forms::snapshot};
    use std::sync::Arc;

    let Some(folder) = std::env::var_os("CINNABAR_FORM_SNAPSHOT_DIR") else {
        return;
    };
    std::fs::create_dir_all(folder).unwrap();
    let icons = assets::encode_icon_catalog(
        [8; 32],
        &[assets::IconSprite {
            width: 16,
            height: 16,
            rgba8: [78, 166, 80, 255].repeat(16 * 16).into(),
        }],
        &[assets::IconEntry {
            identifier: "custom:catalog".into(),
            metadata: 0,
            sprite: 0,
        }],
    )
    .unwrap();
    let mut presentation = UiPresentationRuntime::with_hud_and_icons(
        ui_engine::test_support::fixture_font(),
        crate::test_support::fixture_hud(),
        Arc::new(assets::RuntimeIconCatalog::decode(&icons).unwrap()),
    )
    .unwrap();
    let input = presentation
        .build(
            player,
            runtime,
            0,
            [1280, 720],
            ui::DpiScale::new(1.0).unwrap(),
        )
        .unwrap();
    snapshot::write(&input, name);
}

#[test]
fn inventory_options_adoption_resets_list_offsets() {
    use super::super::inventory_actions::visible_creative_entries;
    use super::super::presentation::screens::GRID_COLUMNS;

    for update_while_open in [false, true] {
        let mut player = player_state::PlayerState::new(1);
        let mut runtime = crate::test_support::creative_with(&mut player, 240);
        let entries = player
            .inventory
            .ledger()
            .creative_catalog()
            .unwrap()
            .items
            .iter()
            .map(|item| protocol::ItemRegistryEntry {
                network_id: item.stack.network_id,
                identifier: "custom:catalog".into(),
                component_based: false,
                components: None,
                version: protocol::ItemRegistryVersion::None,
                component_digest: [0; 32],
                negotiated_max_stack_size: Some(64),
                canonical_empty_component_data: true,
                item_tags: Arc::from([]),
            })
            .collect();
        player
            .inventory
            .ledger_mut()
            .apply_registry(&protocol::ItemRegistryEvent { entries });
        runtime.close_inventory(&mut player);
        let stored = InventoryOptions {
            inventory_layout: InventoryLayout::InventoryOnly,
            ..InventoryOptions::default()
        };
        receive(&mut runtime, &mut player, 1, stored);
        runtime.toggle_inventory(&mut player, None);
        let count = visible_creative_entries(&player, &runtime).len();
        runtime.screen_state_mut().scroll_creative(100, count);
        let row = runtime.screen_state().creative_row;
        runtime.screen_state_mut().book_page = 3;
        runtime
            .screen_state_mut()
            .container_scroll
            .insert("recipes".into(), 24.0);
        let incoming = InventoryOptions {
            left_tab: InventoryTab::Nature,
            ..stored
        };
        if !update_while_open {
            runtime.close_inventory(&mut player);
        }
        receive(&mut runtime, &mut player, 2, incoming);
        if update_while_open {
            runtime.observe_inventory_window(&player);
            assert_eq!(
                runtime.inventory_controller(&player).unwrap().options,
                stored
            );
            assert_eq!(runtime.screen_state().creative_row, row);
            assert_eq!(runtime.screen_state().book_page, 3);
            assert_eq!(runtime.screen_state().container_scroll["recipes"], 24.0);
            runtime.close_inventory(&mut player);
        }
        runtime.toggle_inventory(&mut player, None);
        assert!(player.inventory.ledger_mut().mark_transport_enqueued(0));
        runtime
            .enqueue_inventory_event(
                &mut player,
                1,
                3,
                InventoryEvent::Open(protocol::ContainerOpenEvent {
                    container: protocol::ContainerIdentity::window(
                        protocol::PLAYER_INVENTORY_WINDOW_ID,
                    ),
                    window_type: inventory::inventory_ledger::PERSONAL_INVENTORY_WINDOW_TYPE,
                    position: [0, 64, 0],
                    runtime_entity_id: -1,
                }),
            )
            .unwrap();
        runtime.drain_pending_inventory(&mut player);
        let count = visible_creative_entries(&player, &runtime).len();
        assert!(count > 0 && row * GRID_COLUMNS >= count);
        assert_eq!(
            runtime.inventory_controller(&player).unwrap().options,
            incoming
        );
        snapshot_category(
            &player,
            &runtime,
            &format!("creative-restored-category-{update_while_open}"),
        );
        assert_eq!(runtime.screen_state().creative_row, 0);
        assert_eq!(runtime.screen_state().book_page, 0);
        assert!(runtime.screen_state().container_scroll.is_empty());
        assert!(runtime.take_client_packet().is_none());
        runtime
            .drop_creative_hit(&mut player, InventoryCellHit::CreativeGrid(0), false)
            .expect("the restored category has a reachable first cell")
            .unwrap();
    }
}

#[test]
fn inventory_options_changes_reset_all_list_offsets() {
    for save in [false, true] {
        let mut player = player_state::PlayerState::new(1);
        let mut runtime = crate::test_support::inventory_session(&mut player);
        runtime.toggle_inventory(&mut player, None);
        runtime.screen_state_mut().creative_row = 20;
        runtime.screen_state_mut().book_page = 3;
        runtime
            .screen_state_mut()
            .container_scroll
            .insert("recipes".into(), 24.0);
        runtime
            .change_inventory_options(&mut player, save, |controller| {
                controller.options.left_tab = InventoryTab::Nature;
            })
            .unwrap();
        assert_eq!(runtime.screen_state().creative_row, 0);
        assert_eq!(runtime.screen_state().book_page, 0);
        assert!(runtime.screen_state().container_scroll.is_empty());
    }
}
