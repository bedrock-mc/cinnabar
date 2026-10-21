use super::*;
use protocol::{
    ContainerIdentity, ContainerOpenEvent, FurnaceKind, FurnaceLayout, FurnaceOptions,
    FurnaceOptionsEvent, FurnaceTab, InventoryEvent,
};

/// Applies one server event through the actual inventory ingress and UI drain.
fn apply(
    runtime: &mut UiRuntime,
    player: &mut player_state::PlayerState,
    sequence: u64,
    event: InventoryEvent,
) {
    runtime
        .enqueue_inventory_event(player, 1, sequence, event)
        .unwrap();
    runtime.drain_pending_inventory(player);
}

#[test]
fn furnace_options_received_before_open_restore_the_screen_choices() {
    let mut player = player_state::PlayerState::new(1);
    let mut runtime = crate::test_support::inventory_session(&mut player);
    apply(
        &mut runtime,
        &mut player,
        1,
        InventoryEvent::FurnaceOptions(FurnaceOptionsEvent {
            kind: FurnaceKind::Furnace,
            options: FurnaceOptions {
                left_tab: FurnaceTab::Food,
                filtering: false,
                layout: FurnaceLayout::RecipeBook,
            },
        }),
    );
    apply(
        &mut runtime,
        &mut player,
        2,
        InventoryEvent::Open(ContainerOpenEvent {
            container: ContainerIdentity::window(7),
            window_type: protocol::WINDOW_TYPE_FURNACE,
            position: [0, 64, 0],
            runtime_entity_id: -1,
        }),
    );
    assert_eq!(
        runtime.furnace_options(&player).unwrap().layout,
        FurnaceLayout::RecipeBook
    );
    assert_eq!(
        runtime.furnace_options(&player).unwrap().left_tab,
        FurnaceTab::Food
    );
    assert!(!runtime.recipe_filtering(&player));
    assert!(
        runtime.take_client_packet().is_none(),
        "receiving preferences must not echo them"
    );
}

/// Opens a server-owned screen without bypassing its ordered admission.
fn open(
    runtime: &mut UiRuntime,
    player: &mut player_state::PlayerState,
    sequence: u64,
    window: u8,
    window_type: i8,
) {
    apply(
        runtime,
        player,
        sequence,
        InventoryEvent::Open(ContainerOpenEvent {
            container: ContainerIdentity::window(i32::from(window)),
            window_type,
            position: [0, 64, 0],
            runtime_entity_id: -1,
        }),
    );
}

/// Routes a real furnace widget action through the input dispatcher.
fn click(
    runtime: &mut UiRuntime,
    player: &mut player_state::PlayerState,
    widget: ui_screens::container_actions::Widget,
) {
    runtime.perform_pointer_action(
        player,
        super::super::inventory_drag::PointerAction::Click(
            ui_screens::container_actions::InventoryCellHit::Widget(widget),
        ),
    );
}

#[test]
fn furnace_options_changes_save_once_and_keep_tab_separate_from_layout() {
    use ui_screens::container_actions::Widget;
    let mut player = player_state::PlayerState::new(1);
    let mut runtime = crate::test_support::inventory_session(&mut player);
    let stored = player.inventory.furnace_options(FurnaceKind::Furnace);
    open(
        &mut runtime,
        &mut player,
        1,
        7,
        protocol::WINDOW_TYPE_FURNACE,
    );
    let initial = runtime.furnace_options(&player).unwrap();
    assert_eq!(stored.layout, FurnaceLayout::None);
    assert_eq!(initial.layout, FurnaceLayout::RecipeBook);
    assert!(!initial.filtering);
    click(&mut runtime, &mut player, Widget::InventoryLayout(2));
    click(&mut runtime, &mut player, Widget::FurnaceTab(4));
    assert!(
        runtime.take_client_packet().is_none(),
        "unchanged controls do not save normalized defaults"
    );
    click(&mut runtime, &mut player, Widget::FurnaceTab(5));
    let inventory = FurnaceOptions {
        left_tab: FurnaceTab::Inventory,
        ..initial
    };
    assert_eq!(
        runtime.take_client_packet(),
        Some(protocol::furnace_options_packet(
            FurnaceKind::Furnace,
            inventory
        ))
    );
    assert_eq!(runtime.furnace_options(&player), Some(inventory));
    click(&mut runtime, &mut player, Widget::InventoryLayout(1));
    let narrow = FurnaceOptions {
        layout: FurnaceLayout::InventoryOnly,
        ..inventory
    };
    assert_eq!(
        runtime.take_client_packet(),
        Some(protocol::furnace_options_packet(
            FurnaceKind::Furnace,
            narrow
        ))
    );
    assert_eq!(
        runtime.furnace_options(&player).unwrap().left_tab,
        FurnaceTab::Inventory
    );
    click(&mut runtime, &mut player, Widget::RecipeFilter);
    let filtered = FurnaceOptions {
        filtering: true,
        ..narrow
    };
    assert_eq!(
        runtime.take_client_packet(),
        Some(protocol::furnace_options_packet(
            FurnaceKind::Furnace,
            filtered
        ))
    );
    assert!(runtime.take_client_packet().is_none());
    assert_eq!(
        player.inventory.furnace_options(FurnaceKind::Furnace),
        filtered
    );
    assert_eq!(
        player.inventory.furnace_options(FurnaceKind::Smoker),
        stored
    );
}

#[test]
fn furnace_options_do_not_share_families_or_crafting_filtering() {
    use ui_screens::container_actions::Widget;
    let mut player = player_state::PlayerState::new(1);
    let mut runtime = crate::test_support::inventory_session(&mut player);
    let furnace = FurnaceOptions {
        left_tab: FurnaceTab::Food,
        filtering: false,
        layout: FurnaceLayout::InventoryOnly,
    };
    let smoker = FurnaceOptions {
        left_tab: FurnaceTab::Blocks,
        filtering: true,
        layout: FurnaceLayout::RecipeBook,
    };
    for (sequence, kind, options) in [
        (1, FurnaceKind::Furnace, furnace),
        (2, FurnaceKind::Smoker, smoker),
    ] {
        apply(
            &mut runtime,
            &mut player,
            sequence,
            InventoryEvent::FurnaceOptions(FurnaceOptionsEvent { kind, options }),
        );
    }
    open(
        &mut runtime,
        &mut player,
        3,
        7,
        protocol::WINDOW_TYPE_FURNACE,
    );
    assert_eq!(runtime.furnace_options(&player), Some(furnace));
    click(&mut runtime, &mut player, Widget::FurnaceTab(2));
    let changed = FurnaceOptions {
        left_tab: FurnaceTab::Items,
        ..furnace
    };
    assert_eq!(
        runtime.take_client_packet(),
        Some(protocol::furnace_options_packet(
            FurnaceKind::Furnace,
            changed
        ))
    );
    open(
        &mut runtime,
        &mut player,
        4,
        8,
        protocol::WINDOW_TYPE_SMOKER,
    );
    assert_eq!(runtime.furnace_options(&player), Some(smoker));
    open(
        &mut runtime,
        &mut player,
        5,
        9,
        protocol::WINDOW_TYPE_WORKBENCH,
    );
    assert!(
        !runtime.recipe_filtering(&player),
        "crafting keeps its own unfiltered default"
    );
    open(
        &mut runtime,
        &mut player,
        6,
        10,
        protocol::WINDOW_TYPE_FURNACE,
    );
    assert_eq!(runtime.furnace_options(&player), Some(changed));
    assert!(!runtime.recipe_filtering(&player));
    open(
        &mut runtime,
        &mut player,
        7,
        11,
        protocol::WINDOW_TYPE_BLAST_FURNACE,
    );
    assert!(
        !runtime.recipe_filtering(&player),
        "blast furnace does not inherit crafting or smoker filtering"
    );
    assert!(runtime.take_client_packet().is_none());
}

#[test]
fn furnace_options_received_while_open_do_not_replace_the_controller_or_echo() {
    use ui_screens::container_actions::Widget;
    let mut player = player_state::PlayerState::new(1);
    let mut runtime = crate::test_support::inventory_session(&mut player);
    open(
        &mut runtime,
        &mut player,
        1,
        7,
        protocol::WINDOW_TYPE_FURNACE,
    );
    let shown = runtime.furnace_options(&player).unwrap();
    let received = FurnaceOptions {
        left_tab: FurnaceTab::Food,
        filtering: true,
        layout: FurnaceLayout::InventoryOnly,
    };
    apply(
        &mut runtime,
        &mut player,
        2,
        InventoryEvent::FurnaceOptions(FurnaceOptionsEvent {
            kind: FurnaceKind::Furnace,
            options: received,
        }),
    );
    assert_eq!(
        player.inventory.furnace_options(FurnaceKind::Furnace),
        received
    );
    assert_eq!(runtime.furnace_options(&player), Some(shown));
    click(&mut runtime, &mut player, Widget::InventoryLayout(2));
    assert!(
        runtime.take_client_packet().is_none(),
        "unchanged controller input must not overwrite received preferences"
    );
    click(&mut runtime, &mut player, Widget::FurnaceTab(2));
    let saved = FurnaceOptions {
        left_tab: FurnaceTab::Items,
        ..shown
    };
    assert_eq!(
        runtime.take_client_packet(),
        Some(protocol::furnace_options_packet(
            FurnaceKind::Furnace,
            saved
        ))
    );
    assert_eq!(
        player.inventory.furnace_options(FurnaceKind::Furnace),
        saved
    );
    assert!(runtime.take_client_packet().is_none());
    player.begin_session(2);
    runtime.begin_session(2);
    assert!(runtime.screen_state().furnace.is_none());
    assert_eq!(
        player
            .inventory
            .furnace_options(FurnaceKind::Furnace)
            .layout,
        FurnaceLayout::None
    );
}

#[test]
fn furnace_options_do_not_commit_a_change_when_the_packet_queue_is_full() {
    use ui_screens::container_actions::Widget;
    let mut player = player_state::PlayerState::new(1);
    let mut runtime = crate::test_support::inventory_session(&mut player);
    open(
        &mut runtime,
        &mut player,
        1,
        7,
        protocol::WINDOW_TYPE_FURNACE,
    );
    let shown = runtime.furnace_options(&player).unwrap();
    let stored = player.inventory.furnace_options(FurnaceKind::Furnace);
    let packet = protocol::furnace_options_packet(FurnaceKind::Smoker, shown);
    for _ in 0..super::super::book_screen::MAX_QUEUED_CLIENT_PACKETS {
        assert!(runtime.queue_client_packet(packet.clone()));
    }
    click(&mut runtime, &mut player, Widget::FurnaceTab(1));
    assert_eq!(runtime.furnace_options(&player), Some(shown));
    assert_eq!(
        player.inventory.furnace_options(FurnaceKind::Furnace),
        stored
    );
    while runtime.take_client_packet().is_some() {}
    click(&mut runtime, &mut player, Widget::FurnaceTab(1));
    assert_eq!(
        runtime.take_client_packet(),
        Some(protocol::furnace_options_packet(
            FurnaceKind::Furnace,
            FurnaceOptions {
                left_tab: FurnaceTab::Food,
                ..shown
            }
        ))
    );
    assert!(runtime.take_client_packet().is_none());
}

#[test]
fn furnace_options_latest_selection_follows_a_conflicting_pending_update() {
    use ui_screens::container_actions::Widget;
    let mut player = player_state::PlayerState::new(1);
    let mut runtime = crate::test_support::inventory_session(&mut player);
    open(
        &mut runtime,
        &mut player,
        1,
        7,
        protocol::WINDOW_TYPE_FURNACE,
    );
    let initial = runtime.furnace_options(&player).unwrap();
    click(&mut runtime, &mut player, Widget::FurnaceTab(2));
    let queued = FurnaceOptions {
        left_tab: FurnaceTab::Items,
        ..initial
    };
    let received = FurnaceOptions {
        left_tab: FurnaceTab::Food,
        ..initial
    };
    apply(
        &mut runtime,
        &mut player,
        2,
        InventoryEvent::FurnaceOptions(FurnaceOptionsEvent {
            kind: FurnaceKind::Furnace,
            options: received,
        }),
    );
    click(&mut runtime, &mut player, Widget::FurnaceTab(1));
    assert_eq!(runtime.furnace_options(&player), Some(received));
    assert_eq!(
        runtime.take_client_packet(),
        Some(protocol::furnace_options_packet(
            FurnaceKind::Furnace,
            queued
        ))
    );
    assert_eq!(
        runtime.take_client_packet(),
        Some(protocol::furnace_options_packet(
            FurnaceKind::Furnace,
            received
        )),
        "the last packet must retain the user's latest selection"
    );
    assert!(runtime.take_client_packet().is_none());
}

#[test]
fn furnace_options_pending_updates_only_compare_the_latest_packet_for_the_same_family() {
    use ui_screens::container_actions::Widget;
    for pending_same_family in [false, true] {
        let mut player = player_state::PlayerState::new(1);
        let mut runtime = crate::test_support::inventory_session(&mut player);
        open(
            &mut runtime,
            &mut player,
            1,
            7,
            protocol::WINDOW_TYPE_FURNACE,
        );
        let initial = runtime.furnace_options(&player).unwrap();
        let received = FurnaceOptions {
            left_tab: FurnaceTab::Food,
            ..initial
        };
        let first = protocol::furnace_options_packet(
            if pending_same_family {
                FurnaceKind::Furnace
            } else {
                FurnaceKind::Smoker
            },
            initial,
        );
        assert!(runtime.queue_client_packet(first.clone()));
        let latest = protocol::furnace_options_packet(FurnaceKind::Furnace, received);
        if pending_same_family {
            assert!(runtime.queue_client_packet(latest.clone()));
        }
        apply(
            &mut runtime,
            &mut player,
            2,
            InventoryEvent::FurnaceOptions(FurnaceOptionsEvent {
                kind: FurnaceKind::Furnace,
                options: received,
            }),
        );
        click(&mut runtime, &mut player, Widget::FurnaceTab(1));
        assert_eq!(runtime.take_client_packet(), Some(first));
        if pending_same_family {
            assert_eq!(runtime.take_client_packet(), Some(latest));
        }
        assert!(runtime.take_client_packet().is_none());
        assert_eq!(runtime.furnace_options(&player), Some(received));
    }
}
