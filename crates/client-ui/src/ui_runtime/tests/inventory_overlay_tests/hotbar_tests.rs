use super::*;

/// Drains one full window-0 content event through the production queue so
/// both retained stores agree, then selects hotbar slot 0 so slot 3 stays a
/// nonselected cell for every witness below.
fn drained_inventory_runtime(player_runtime: &mut player_state::PlayerState) -> UiRuntime {
    let mut runtime = UiRuntime::new(1);
    runtime.publish_inventory_authority(player_runtime, InventoryAuthority::Server);
    let mut slots = vec![NetworkItemStack::empty(); PLAYER_INVENTORY_SLOT_COUNT];
    slots[0] = ledger_stack(745, 13, 4);
    slots[3] = ledger_stack(846, 24, 1);
    runtime
        .enqueue_inventory_event(
            player_runtime,
            1,
            1,
            InventoryEvent::Content(InventoryContentEvent {
                container: ContainerIdentity::window(0),
                slots: Arc::from(slots),
                storage_item: NetworkItemStack::empty(),
            }),
        )
        .unwrap();
    runtime.drain_pending_inventory(player_runtime);
    admit_personal_inventory(player_runtime, &mut runtime);
    player_runtime.inventory.set_local_selected_slot(0);
    runtime
}

/// Publishes one HUD frame and returns its presented hotbar stacks.
fn presented_hotbar_stacks(
    player_runtime: &player_state::PlayerState,
    runtime: &mut UiRuntime,
) -> [Option<protocol::NetworkItemStack>; 9] {
    let mut presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
    refresh_hud_frame(
        player_runtime,
        runtime,
        &mut presentation,
        Some(&world_stream()),
        semantic_input::PerspectiveMode::FirstPerson,
        1_000,
    );
    presentation.hud_frame().hotbar_stacks.clone()
}

/// Keeps the visible count and wire stack identity together in assertions.
fn cell_facts(stack: &protocol::NetworkItemStack) -> (u16, i32) {
    (stack.count, stack.stack_network_id)
}

#[test]
fn accepted_sparse_corrections_refresh_every_presented_nonselected_hotbar_consumer() {
    let mut player_runtime = player_state::PlayerState::new(1);

    let mut runtime = drained_inventory_runtime(&mut player_runtime);

    // Round-trip the nonselected sword through the cursor; the accepted place
    // response explicitly clears the cursor and corrects slot 3: a server-side
    // count change plus its authoritative identity correction.
    let take = runtime
        .inventory_ledger_mut(&mut player_runtime)
        .begin_click(3)
        .unwrap();
    accept_take(
        &mut player_runtime,
        &mut runtime,
        take,
        protocol::CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY,
        None,
        3,
        correction(0, 1, 24, "", "", 0),
    );
    let place = runtime
        .inventory_ledger_mut(&mut player_runtime)
        .begin_click(3)
        .unwrap();
    accept_place(
        &mut player_runtime,
        &mut runtime,
        place,
        protocol::CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY,
        None,
        correction(3, 2, 99, "Corrected Blade", "", 250),
    );
    let corrected = runtime
        .inventory_ledger(&player_runtime)
        .displayed_stack(3)
        .expect("the corrected cell stays present in the ledger");
    assert_eq!(cell_facts(corrected), (2, 99));

    // The presented nonselected cell and the HUD frame must show that exact
    // ledger revision, not the stale pre-correction mirror.
    assert_eq!(
        player_runtime
            .presented_hotbar_stack(3)
            .map(cell_facts)
            .expect("the corrected nonselected cell stays presented"),
        (2, 99),
        "the presented nonselected hotbar cell must follow the accepted correction",
    );
    let frame_stacks = presented_hotbar_stacks(&player_runtime, &mut runtime);
    let presented = frame_stacks[3]
        .as_ref()
        .expect("the corrected nonselected cell is presented");
    assert_eq!(
        cell_facts(presented),
        (2, 99),
        "the HUD frame must present the corrected ledger snapshot",
    );
    // The selected cell keeps presenting its own untouched authority.
    assert_eq!(
        frame_stacks[0].as_ref().map(cell_facts),
        Some((4, 13)),
        "an unrelated accepted correction must not disturb the selected cell",
    );
}

#[test]
fn rejected_nonselected_gestures_present_the_pre_gesture_cells_again() {
    let mut player_runtime = player_state::PlayerState::new(1);

    let mut runtime = drained_inventory_runtime(&mut player_runtime);

    // Mid-flight, the nonselected cell presents its predicted half exactly
    // like the selected-cell authority already does.
    let request_id = runtime
        .inventory_ledger_mut(&mut player_runtime)
        .begin_click(3)
        .unwrap();
    assert_eq!(player_runtime.presented_hotbar_stack(3), None);

    runtime
        .inventory_ledger_mut(&mut player_runtime)
        .apply(&InventoryEvent::Response(ItemStackResponseEvent {
            responses: Arc::from([StackResponse {
                status: StackResponseStatus::Rejected,
                request_id,
                containers: Arc::from([]),
            }]),
        }));

    // Rejection restores every presented consumer to the exact pre-gesture
    // facts; nothing else about the presented row moves.
    let restored = presented_hotbar_stacks(&player_runtime, &mut runtime);
    assert_eq!(restored[3].as_ref().map(cell_facts), Some((1, 24)));
    assert_eq!(restored[0].as_ref().map(cell_facts), Some((4, 13)));
}

#[test]
fn session_reset_presents_no_hotbar_cells_from_either_store() {
    let mut player_runtime = player_state::PlayerState::new(1);

    let mut runtime = drained_inventory_runtime(&mut player_runtime);
    assert!(presented_hotbar_stacks(&player_runtime, &mut runtime)[3].is_some());

    player_runtime.begin_session(2);
    runtime.begin_session(2);

    let reset = presented_hotbar_stacks(&player_runtime, &mut runtime);
    assert!(
        reset.iter().all(Option::is_none),
        "a session reset must clear every presented hotbar cell"
    );
    for slot in 0..9u8 {
        assert_eq!(player_runtime.presented_hotbar_stack(slot), None);
    }
}

#[test]
fn resolved_hotbar_shortcut_swaps_both_cells_without_waiting_for_a_response() {
    use crate::ui_runtime::interaction::dispatch_inventory_key;
    use crate::ui_runtime::presentation::inventory_pointer::InventoryCellHit;
    let mut player = player_state::PlayerState::new(1);
    let mut runtime = drained_inventory_runtime(&mut player);
    let hit = Some(InventoryCellHit::Player(3));
    assert!(
        dispatch_inventory_key(
            &mut player,
            &mut runtime,
            hit,
            bevy::prelude::KeyCode::Digit1,
            false,
            None,
            false
        )
        .is_none()
    );
    assert_eq!(player.presented_hotbar_stack(0).unwrap().network_id, 745);
    dispatch_inventory_key(
        &mut player,
        &mut runtime,
        hit,
        bevy::prelude::KeyCode::KeyR,
        false,
        Some(0),
        false,
    )
    .unwrap()
    .unwrap();
    assert_eq!(player.presented_hotbar_stack(0).unwrap().network_id, 846);
    assert_eq!(player.presented_hotbar_stack(3).unwrap().network_id, 745);
    assert_eq!(player.selected_hotbar_slot(), Some(0));
    runtime.screen_state_mut().search_focused = true;
    assert!(
        crate::ui_runtime::interaction::dispatch_inventory_hotbar(
            &mut player,
            &mut runtime,
            hit,
            0
        )
        .is_none()
    );
    assert_eq!(player.presented_hotbar_stack(0).unwrap().network_id, 846);
    runtime.screen_state_mut().search_focused = false;
    assert!(
        crate::ui_runtime::interaction::dispatch_inventory_hotbar(
            &mut player,
            &mut runtime,
            hit,
            protocol::HOTBAR_SLOT_COUNT
        )
        .is_none()
    );
}

#[test]
fn focused_inventory_text_does_not_dispatch_drop_requests() {
    use crate::ui_runtime::interaction::dispatch_inventory_key;
    use crate::ui_runtime::presentation::inventory_pointer::InventoryCellHit;
    let mut player = player_state::PlayerState::new(1);
    let mut runtime = drained_inventory_runtime(&mut player);
    runtime.screen_state_mut().search_focused = true;
    let before = player.presented_hotbar_stack(3).unwrap().clone();
    let pending = runtime.inventory_ledger(&player).pending_request_count();
    assert!(
        dispatch_inventory_key(
            &mut player,
            &mut runtime,
            Some(InventoryCellHit::Player(3)),
            bevy::prelude::KeyCode::KeyQ,
            false,
            None,
            true,
        )
        .is_none()
    );
    assert_eq!(
        runtime.inventory_ledger(&player).pending_request_count(),
        pending
    );
    assert_eq!(player.presented_hotbar_stack(3), Some(&before));
}
