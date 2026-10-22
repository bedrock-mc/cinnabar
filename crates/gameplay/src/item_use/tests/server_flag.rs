use super::*;

/// A bread frame whose use lasts 32 ticks.
fn bread(tick: u64, held: bool) -> UseFrame {
    item_frame(tick, held, selection(2, MENU_ITEM), "minecraft:bread")
}

/// A server `UsingItem` set starts the selected held use with StartUsingItem and
/// no transaction; without the button, button-up never releases it.
#[test]
fn a_server_flag_starts_the_selected_use_without_a_transaction() {
    let mut runtime = ItemUseRuntime::default();
    runtime.adopt_server_using_item(true);
    assert!(runtime.has_work(false));
    let mut swings = SwingTracker::default();
    assert!(step_and_send(
        &mut runtime,
        &mut swings,
        &frame(10, false),
        7,
        6,
        |_| panic!("a server start sends nothing")
    ));
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
    assert!(runtime.step(&bread(10, false)).started);
    let outcome = runtime.step(&bread(41, false));
    assert!(outcome.packets.is_empty());
    assert!(runtime.is_using());
    let outcome = runtime.step(&bread(42, false));
    assert!(outcome.packets.is_empty());
    assert!(!runtime.is_using());
}

/// Once the button holds a server-started use, button-up releases it as usual.
#[test]
fn the_button_releases_a_server_started_use_it_held() {
    let mut runtime = ItemUseRuntime::default();
    runtime.adopt_server_using_item(true);
    runtime.step(&frame(10, false));
    assert!(runtime.step(&frame(11, true)).packets.is_empty());
    assert!(runtime.is_releasing(false));
    assert_eq!(kinds(&runtime.step(&frame(12, false))), ["release"]);
    assert!(!runtime.is_using());
}

/// A server `UsingItem` clear stops an accepted use without a release transaction.
#[test]
fn a_server_clear_stops_the_use_without_a_release() {
    let mut runtime = ItemUseRuntime::default();
    runtime.observe_press(true);
    assert_eq!(kinds(&runtime.step(&frame(10, true))), ["use"]);
    runtime.adopt_server_using_item(false);
    let outcome = runtime.step(&frame(11, true));
    assert!(outcome.packets.is_empty());
    assert!(!runtime.is_using());
    assert!(!runtime.has_work(false));
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
    assert!(!runtime.step(&unverified).started);
    assert!(!runtime.is_using());
    assert!(runtime.has_work(false));
    let stick = UseFrame {
        air_use: None,
        ..frame(11, false)
    };
    assert!(!runtime.step(&stick).started);
    assert!(!runtime.is_using());
    assert!(!runtime.has_work(false));
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
    let sample = runtime
        .frame_sample(&movement, false, true)
        .expect("a server write resolves between ticks");
    assert!(runtime.resolve_server_using_item(&frame(sample.tick, false)));
    assert!(runtime.frame_sample(&movement, false, true).is_none());
    movement
        .enqueue_completed_physics(completed(sample.tick))
        .unwrap();
    flush(&mut movement);
    assert!(
        runtime.frame_sample(&movement, false, true).is_some(),
        "the completed tick resumes maintenance"
    );
}

/// A server clear before the started use's tick runs withdraws its
/// `StartUsingItem`, as vanilla only starts the use when that tick reads the flag.
#[test]
fn a_clear_before_the_start_tick_withdraws_start_using_item() {
    use crate::test_support::survival_mining::{completed, evidence, ticker_with_ticks};
    let mut movement = ticker_with_ticks(1);
    crate::movement::flush_player_auth_inputs(&mut movement, 8, Some(evidence()), |_, _| {
        Ok::<_, ()>(())
    })
    .unwrap();
    let tick = movement.next_tick();
    let mut runtime = ItemUseRuntime::default();
    runtime.adopt_server_using_item(true);
    assert!(runtime.resolve_server_using_item(&frame(tick, false)));
    assert!(movement.mark_started_using_item(tick));
    runtime.adopt_server_using_item(false);
    assert!(!runtime.resolve_server_using_item(&frame(tick, false)));
    assert_eq!(runtime.take_withdrawn_start(), Some(tick));
    movement.withdraw_started_using_item(tick);
    movement.enqueue_completed_physics(completed(tick)).unwrap();
    let snapshot = movement.pending_snapshots().pop().unwrap();
    assert_eq!(snapshot.tick, tick);
    assert_eq!(
        snapshot.flags.bits() & protocol::PlayerInputFlags::START_USING_ITEM.bits(),
        0
    );
}
