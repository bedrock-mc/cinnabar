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
