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
    runtime.toggle_inventory(&mut player);
    assert_eq!(
        runtime.inventory_controller(&player).unwrap().options,
        stored
    );
    assert_eq!(runtime.screen_state().creative_tab(), 1);
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
    runtime.toggle_inventory(&mut player);
    let before = runtime.inventory_controller(&player).unwrap();
    runtime.screen_state_mut().creative_row = 4;
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
    runtime.toggle_inventory(&mut player);
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
    runtime.toggle_inventory(&mut player);
    assert_eq!(
        runtime.inventory_controller(&player).unwrap().options,
        incoming
    );
}

#[test]
fn inventory_options_queue_rejection_and_session_reset_are_atomic() {
    let mut player = player_state::PlayerState::new(1);
    let mut runtime = crate::test_support::inventory_session(&mut player);
    runtime.toggle_inventory(&mut player);
    let shown = runtime.inventory_controller(&player).unwrap();
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
    runtime.toggle_inventory(&mut player);
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
    runtime.toggle_inventory(&mut player);
    runtime
        .change_inventory_options(&mut player, false, |controller| {
            controller.options.left_tab = InventoryTab::Nature
        })
        .unwrap();
    runtime.close_inventory(&mut player);
    assert!(runtime.screen_state().inventory.is_none());
    assert!(runtime.take_client_packet().is_none());
    runtime.toggle_inventory(&mut player);
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
        runtime.toggle_inventory(&mut player);
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
        runtime.observe_inventory_search_focus(&player);
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
        runtime.toggle_inventory(&mut player);
        runtime.perform_pointer_action(
            &mut player,
            PointerAction::Click(InventoryCellHit::CreativeSearch),
        );
        runtime.screen_state_mut().type_text("iron");
        assert!(runtime.screen_state().text_focused());
        if toggle {
            runtime.toggle_inventory(&mut player);
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
