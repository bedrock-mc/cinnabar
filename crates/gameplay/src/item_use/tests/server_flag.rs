use super::*;

/// A bread frame whose use lasts 32 ticks.
fn bread(tick: u64, held: bool) -> UseFrame {
    item_frame(tick, held, selection(2, MENU_ITEM), "minecraft:bread")
}

/// A server `UsingItem` set starts the selected held use at its movement tick
/// with no transaction; without the button, button-up never releases it.
#[test]
fn a_server_flag_starts_the_selected_use_without_a_transaction() {
    let mut runtime = ItemUseRuntime::default();
    runtime.adopt_server_using_item(true);
    assert!(runtime.resolve_server_using_item(&frame(10, false)));
    assert!(runtime.is_using());
    assert_eq!(runtime.movement_modifier(), Some(0.35));
    assert!(!runtime.is_releasing(false));
    let outcome = runtime.step(&frame(11, false));
    assert!(outcome.packets.is_empty());
    assert!(runtime.is_using());
}

/// A server-started use completes locally when its duration runs out.
#[test]
fn a_server_started_use_completes_without_a_release() {
    let mut runtime = ItemUseRuntime::default();
    runtime.adopt_server_using_item(true);
    assert!(runtime.resolve_server_using_item(&bread(10, false)));
    assert!(runtime.step(&bread(41, false)).packets.is_empty());
    assert!(runtime.is_using());
    assert!(runtime.step(&bread(42, false)).packets.is_empty());
    assert!(!runtime.is_using());
}

/// Once the button holds a server-started use, button-up releases it as usual.
#[test]
fn the_button_releases_a_server_started_use_it_held() {
    let mut runtime = ItemUseRuntime::default();
    runtime.adopt_server_using_item(true);
    runtime.resolve_server_using_item(&frame(10, false));
    assert!(runtime.step(&frame(11, true)).packets.is_empty());
    assert!(runtime.is_releasing(false));
    assert_eq!(kinds(&runtime.step(&frame(12, false))), ["release"]);
    assert!(!runtime.is_using());
}

/// Holding the use button between ticks owns a server-started use, so
/// button-up releases it even before held-use maintenance runs again.
#[test]
fn the_use_button_owns_a_server_start_outside_maintenance() {
    let mut runtime = ItemUseRuntime::default();
    runtime.adopt_server_using_item(true);
    assert!(runtime.resolve_server_using_item(&frame(10, false)));
    runtime.observe_use_button(true);
    assert!(runtime.is_releasing(false));
}

/// A server `UsingItem` clear stops an accepted use at its movement tick
/// without a release transaction.
#[test]
fn a_server_clear_stops_the_use_without_a_release() {
    let mut runtime = ItemUseRuntime::default();
    runtime.observe_press(true);
    assert_eq!(kinds(&runtime.step(&frame(10, true))), ["use"]);
    runtime.adopt_server_using_item(false);
    assert!(!runtime.resolve_server_using_item(&frame(11, false)));
    assert!(!runtime.is_using());
    assert!(!runtime.has_server_using_item());
    assert!(runtime.step(&frame(11, false)).packets.is_empty());
}

/// A server start waits for a verified selection and starts nothing for an
/// item without a held use.
#[test]
fn a_server_start_needs_a_verified_held_use() {
    let mut runtime = ItemUseRuntime::default();
    runtime.adopt_server_using_item(true);
    let unverified = UseFrame {
        selection: None,
        ..frame(10, false)
    };
    assert!(!runtime.resolve_server_using_item(&unverified));
    assert!(!runtime.is_using());
    assert!(runtime.has_server_using_item());
    let stick = UseFrame {
        air_use: None,
        ..frame(11, false)
    };
    assert!(!runtime.resolve_server_using_item(&stick));
    assert!(!runtime.is_using());
    assert!(!runtime.has_server_using_item());
}

/// A server start resolved before physics keeps receiving held-use maintenance
/// once its tick completes, so its duration can run out.
#[test]
fn a_pre_physics_server_start_keeps_its_maintenance() {
    use crate::test_support::survival_mining::{completed, evidence, ticker_with_ticks};
    let flush = |movement: &mut crate::movement::MovementTicker| {
        crate::movement::flush_player_auth_inputs(movement, 8, Some(evidence()), |_, _| {
            Ok::<_, ()>(())
        })
        .unwrap();
    };
    let mut movement = ticker_with_ticks(1);
    flush(&mut movement);
    let mut runtime = ItemUseRuntime::default();
    runtime.adopt_server_using_item(true);
    let tick = movement.next_tick();
    assert!(runtime.resolve_server_using_item(&frame(tick, false)));
    assert!(runtime.frame_sample(&movement, false, true).is_none());
    movement.enqueue_completed_physics(completed(tick)).unwrap();
    flush(&mut movement);
    assert!(
        runtime.frame_sample(&movement, false, true).is_some(),
        "the completed tick resumes maintenance"
    );
}

/// Focus leaving gameplay releases a server-started use, as vanilla's focus
/// and chat handlers do for any use.
#[test]
fn losing_focus_releases_a_server_started_use() {
    let mut runtime = ItemUseRuntime::default();
    runtime.observe_gameplay_focus(true);
    runtime.adopt_server_using_item(true);
    assert!(runtime.resolve_server_using_item(&frame(10, false)));
    assert!(!runtime.is_releasing(false));
    runtime.observe_gameplay_focus(false);
    assert!(runtime.is_releasing(false));
    assert_eq!(kinds(&runtime.step(&frame(11, false))), ["release"]);
}

/// A clear and a set before the next movement tick leave the established use
/// as it was: only the value a tick reads matters.
#[test]
fn a_clear_and_set_before_a_tick_keep_the_use() {
    let mut runtime = ItemUseRuntime::default();
    runtime.adopt_server_using_item(true);
    assert!(runtime.resolve_server_using_item(&bread(10, false)));
    runtime.step(&bread(11, false));
    runtime.adopt_server_using_item(false);
    runtime.adopt_server_using_item(true);
    assert!(!runtime.resolve_server_using_item(&bread(12, false)));
    assert_eq!(runtime.active_timing(), Some((10, 32)));
}

/// A clear awaiting its movement tick leaves the use in place, so a button
/// release in between still sends the release transaction.
#[test]
fn a_release_before_a_pending_clear_runs_sends_its_transaction() {
    let mut runtime = ItemUseRuntime::default();
    runtime.observe_press(true);
    assert_eq!(kinds(&runtime.step(&frame(10, true))), ["use"]);
    runtime.adopt_server_using_item(false);
    assert_eq!(kinds(&runtime.step(&frame(11, false))), ["release"]);
}

/// A server start still waiting for its movement tick follows the selection
/// that tick sees, so an earlier reselection to an item without a held use
/// starts nothing.
#[test]
fn a_pending_server_start_follows_a_reselection() {
    let mut runtime = ItemUseRuntime::default();
    runtime.adopt_server_using_item(true);
    runtime.retire_reselected_use(5);
    let snowball = UseFrame {
        selection: Some(selection(5, SNOWBALL)),
        air_use: classify("minecraft:snowball", false, 0, None),
        ..frame(12, false)
    };
    assert!(!runtime.resolve_server_using_item(&snowball));
    assert!(!runtime.is_using());
}

/// A local release after a server clear and set supersedes them, as the
/// release clears vanilla's flag, so the next tick starts nothing.
#[test]
fn a_local_release_supersedes_pending_server_writes() {
    let mut runtime = ItemUseRuntime::default();
    runtime.observe_press(true);
    assert_eq!(kinds(&runtime.step(&frame(10, true))), ["use"]);
    runtime.adopt_server_using_item(false);
    runtime.adopt_server_using_item(true);
    assert_eq!(kinds(&runtime.step(&frame(11, false))), ["release"]);
    assert!(!runtime.resolve_server_using_item(&frame(12, false)));
    assert!(!runtime.is_using());
}

/// Replacing a use within one step is a local start, which supersedes an
/// older pending server clear.
#[test]
fn a_replacement_use_supersedes_a_pending_server_clear() {
    let mut runtime = ItemUseRuntime::default();
    runtime.observe_press(true);
    assert_eq!(kinds(&runtime.step(&frame(10, true))), ["use"]);
    runtime.adopt_server_using_item(false);
    runtime.observe_press(true);
    assert!(runtime.step(&bread(20, true)).started);
    assert!(!runtime.resolve_server_using_item(&bread(21, true)));
    assert!(runtime.is_using());
}

/// Local starts and ends write the item-use flag for movement's live word,
/// which later server writes compare with.
#[test]
fn local_starts_and_ends_write_the_item_use_flag() {
    let mut runtime = ItemUseRuntime::default();
    assert_eq!(runtime.take_local_using_item(), None);
    runtime.observe_press(true);
    assert_eq!(kinds(&runtime.step(&frame(10, true))), ["use"]);
    assert_eq!(runtime.take_local_using_item(), Some(true));
    runtime.step(&frame(11, true));
    assert_eq!(runtime.take_local_using_item(), None);
    assert_eq!(kinds(&runtime.step(&frame(12, false))), ["release"]);
    assert_eq!(runtime.take_local_using_item(), Some(false));
}

/// A Riptide release cancelled by a changed selection is a local stop: it
/// supersedes a pending server start and clears the live item-use flag.
#[test]
fn a_cancelled_riptide_release_supersedes_a_pending_server_start() {
    let trident = |tick, held| UseFrame {
        riptide: Some(RiptideUse {
            level: 3,
            wet: true,
            mounted: false,
            usable: true,
        }),
        ..item_frame(tick, held, selection(2, MENU_ITEM), "minecraft:trident")
    };
    let mut runtime = ItemUseRuntime::default();
    runtime.observe_press(true);
    runtime.step(&trident(100, true));
    assert_eq!(runtime.take_local_using_item(), Some(true));
    runtime.adopt_server_using_item(true);
    let mut edge = trident(110, false);
    edge.selection.as_mut().unwrap().slot += 1;
    runtime.observe_riptide_release(
        edge.tick,
        edge.held,
        edge.riptide,
        edge.selection.as_ref(),
        edge.riptide_facing,
    );
    assert!(!runtime.is_using());
    assert!(!runtime.has_server_using_item());
    assert_eq!(runtime.take_local_using_item(), Some(false));
}

/// A food use that progress observation stops on reselection is a local
/// stop: it supersedes a pending server start and clears the item-use flag.
#[test]
fn a_progress_reselection_stop_supersedes_a_pending_server_start() {
    let selection = stack(0, 300, 2);
    let mut runtime = ItemUseRuntime::default();
    runtime.observe_press(true);
    assert!(
        runtime
            .step(&item_frame(
                100,
                true,
                selection.clone(),
                "minecraft:golden_apple"
            ))
            .started
    );
    assert_eq!(runtime.take_local_using_item(), Some(true));
    runtime.adopt_server_using_item(true);
    let mut reselected = selection;
    reselected.slot = 1;
    runtime
        .send_progress_tick(
            101,
            42,
            Some(&reselected),
            Some(assets::facts::ItemUseAnimationFact::Eat),
            |_| Ok::<_, ()>(()),
        )
        .unwrap();
    assert!(!runtime.is_using());
    assert!(!runtime.has_server_using_item());
    assert_eq!(runtime.take_local_using_item(), Some(false));
}

/// A count-only stack update keeps a server-started use the button never
/// held, as vanilla refreshes the in-use stack while ignoring its count.
#[test]
fn a_count_change_keeps_an_unowned_server_started_use() {
    let mut runtime = ItemUseRuntime::default();
    runtime.adopt_server_using_item(true);
    let two = stack(2, MENU_ITEM, 2);
    assert!(runtime.resolve_server_using_item(&item_frame(10, false, two, "minecraft:bread")));
    let three = stack(2, MENU_ITEM, 3);
    let outcome = runtime.step(&item_frame(11, false, three, "minecraft:bread"));
    assert!(outcome.packets.is_empty());
    assert!(runtime.is_using());
}

/// A dimension change drops a server item-use value adopted in the old
/// world, whatever input device drove the frame.
#[test]
fn a_world_change_drops_a_pending_server_value() {
    let mut runtime = ItemUseRuntime::default();
    runtime.observe_world(SessionGeneration::new(7), 0);
    runtime.adopt_server_using_item(true);
    runtime.observe_world(SessionGeneration::new(7), 0);
    assert!(runtime.has_server_using_item());
    runtime.observe_world(SessionGeneration::new(7), 1);
    assert!(!runtime.has_server_using_item());
}

/// Entering a game mode without item use, such as spectator, stops the use
/// without a release transaction and drops a pending server start; a later
/// server start in that mode still reconciles, as vanilla's tick has no
/// spectator gate.
#[test]
fn entering_a_mode_without_item_use_stops_the_use_once() {
    let mut runtime = ItemUseRuntime::default();
    runtime.adopt_server_using_item(true);
    assert!(runtime.resolve_server_using_item(&bread(10, false)));
    runtime.take_local_using_item();
    runtime.adopt_server_using_item(true);
    runtime.observe_item_use_allowed(false);
    assert!(!runtime.is_using());
    assert!(!runtime.has_server_using_item());
    assert_eq!(runtime.take_local_using_item(), Some(false));
    runtime.adopt_server_using_item(true);
    runtime.observe_item_use_allowed(false);
    assert!(runtime.resolve_server_using_item(&bread(11, false)));
    assert!(runtime.is_using());
}

/// A server start while the use button is already held belongs to that
/// button, so the button-up that follows releases it with its transaction.
#[test]
fn a_server_start_under_a_held_button_releases_on_button_up() {
    let mut runtime = ItemUseRuntime::default();
    runtime.observe_use_button(true);
    runtime.adopt_server_using_item(true);
    assert!(runtime.resolve_server_using_item(&bread(10, false)));
    assert!(runtime.is_releasing(false));
}

/// Entering a mode without item use while a server start still awaits its
/// movement tick also clears the item-use flag, which the start had set.
#[test]
fn entering_a_mode_without_item_use_clears_a_pending_start_flag() {
    let mut runtime = ItemUseRuntime::default();
    runtime.adopt_server_using_item(true);
    runtime.observe_item_use_allowed(false);
    assert!(!runtime.has_server_using_item());
    assert_eq!(runtime.take_local_using_item(), Some(false));
}

/// Respawn forgets the use button's last state, so a server start before
/// the button is observed again does not belong to a button that was held
/// before the respawn.
#[test]
fn respawn_forgets_the_held_use_button() {
    let mut runtime = ItemUseRuntime::default();
    runtime.observe_use_button(true);
    runtime.cancel_for_respawn();
    runtime.adopt_server_using_item(true);
    assert!(runtime.resolve_server_using_item(&bread(10, false)));
    assert!(!runtime.is_releasing(false));
}
