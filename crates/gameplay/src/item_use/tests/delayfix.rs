use super::*;

#[test]
fn delay_fix_does_not_accelerate_repeated_throws_in_one_slot() {
    let mut runtime = ItemUseRuntime::default();
    runtime.set_delay_fix(true);
    let first = item_frame(1, false, stack(2, SNOWBALL, 16), "minecraft:snowball");
    runtime.observe_press(true);
    assert!(runtime.step(&first).swung);

    runtime.observe_press(true);
    let repeated = UseFrame {
        tick: 2,
        now_millis: first.now_millis + 50,
        ..first.clone()
    };
    assert!(runtime.step(&repeated).packets.is_empty());

    runtime.observe_press(true);
    let ready = UseFrame {
        tick: 6,
        now_millis: first.now_millis + 250,
        ..first
    };
    assert!(runtime.step(&ready).swung);
}

#[test]
fn delay_fix_grants_one_use_per_slot_change_then_keeps_repeat_timing() {
    for enabled in [false, true] {
        let mut runtime = ItemUseRuntime::default();
        runtime.set_delay_fix(enabled);
        let first = item_frame(1, true, stack(2, SNOWBALL, 16), "minecraft:snowball");
        runtime.observe_press(true);
        assert!(runtime.step(&first).swung);

        let switched = item_frame(2, true, stack(3, SNOWBALL, 16), "minecraft:snowball");
        assert_eq!(runtime.step(&switched).swung, enabled);
        let repeated = UseFrame {
            tick: 3,
            now_millis: 150,
            ..switched.clone()
        };
        runtime.observe_press(true);
        assert!(runtime.step(&repeated).packets.is_empty());
        assert!(
            runtime
                .step(&UseFrame {
                    tick: 4,
                    now_millis: 200,
                    ..switched.clone()
                })
                .packets
                .is_empty()
        );
        assert!(
            runtime
                .step(&UseFrame {
                    tick: 7,
                    now_millis: 350,
                    ..switched
                })
                .swung
        );
    }
}

#[test]
fn delay_fix_observes_idle_switches_but_not_stack_updates_or_toggle_cycles() {
    let mut runtime = ItemUseRuntime::default();
    runtime.set_delay_fix(true);
    let first = item_frame(1, false, stack(2, SNOWBALL, 16), "minecraft:snowball");
    runtime.observe_press(true);
    assert!(runtime.step(&first).swung);
    let replaced = item_frame(2, false, stack(2, MENU_ITEM, 16), "minecraft:snowball");
    runtime.observe_press(true);
    assert!(runtime.step(&replaced).packets.is_empty());

    let idle = item_frame(2, false, stack(3, SNOWBALL, 16), "minecraft:snowball");
    assert!(runtime.step(&idle).packets.is_empty());
    let switched_back = UseFrame {
        tick: 3,
        now_millis: 150,
        ..first.clone()
    };
    runtime.observe_press(true);
    assert!(runtime.step(&switched_back).swung);

    runtime.step(&UseFrame {
        tick: 3,
        now_millis: 150,
        ..idle
    });
    runtime.set_delay_fix(false);
    runtime.set_delay_fix(true);
    runtime.observe_press(true);
    assert!(
        runtime
            .step(&item_frame(
                4,
                false,
                stack(3, SNOWBALL, 16),
                "minecraft:snowball"
            ))
            .packets
            .is_empty()
    );
}

#[test]
fn delay_fix_allows_slot_hopping_between_frames_but_not_same_slot_repeats() {
    let mut runtime = ItemUseRuntime::default();
    runtime.set_delay_fix(true);
    for (tick, slot) in [(1, 2), (1, 3), (1, 2), (2, 3)] {
        let first = item_frame(tick, false, stack(slot, SNOWBALL, 16), "minecraft:snowball");
        runtime.observe_press(true);
        assert!(runtime.step(&first).swung);
        runtime.observe_press(true);
        assert!(runtime.step(&first).packets.is_empty());
    }
}

#[test]
fn delay_fix_preserves_shared_item_cooldowns_across_slot_changes() {
    let mut runtime = ItemUseRuntime::default();
    runtime.set_delay_fix(true);
    let first = item_frame(1, false, stack(2, SNOWBALL, 16), "minecraft:ender_pearl");
    runtime.observe_press(true);
    assert!(runtime.step(&first).swung);
    for tick in 2..20 {
        let switched = item_frame(
            tick,
            false,
            stack((tick % 2) as u8 + 2, SNOWBALL, 16),
            "minecraft:ender_pearl",
        );
        runtime.observe_press(true);
        assert!(!runtime.step(&switched).swung);
    }
    runtime.observe_press(true);
    assert!(
        runtime
            .step(&item_frame(
                21,
                false,
                stack(4, SNOWBALL, 16),
                "minecraft:ender_pearl"
            ))
            .swung
    );
}

#[test]
fn a_fresh_slot_change_throw_sends_in_the_next_frame_between_ticks() {
    for tick_delta in [0, 1] {
        let mut runtime = ItemUseRuntime::default();
        runtime.set_delay_fix(true);
        let mut swings = SwingTracker::default();
        let first = item_frame(1, false, stack(0, SNOWBALL, 16), "minecraft:splash_potion");
        runtime.observe_press(true);
        let mut sent = Vec::new();
        step_and_send(&mut runtime, &mut swings, &first, 7, 6, |packets| {
            sent.extend(packets);
            Ok(())
        });
        assert_eq!(sent.len(), 2);
        sent.clear();
        let switched = UseFrame {
            tick: first.tick + tick_delta,
            now_millis: first.now_millis + 16,
            ..item_frame(1, false, stack(1, SNOWBALL, 16), "minecraft:egg")
        };
        runtime.observe_selected_slot(Some(1));
        runtime.observe_press(true);
        assert_eq!(runtime.press_drop_reason(&switched), None);
        step_and_send(&mut runtime, &mut swings, &switched, 7, 6, |packets| {
            sent.extend(packets);
            Ok(())
        });
        assert_eq!(sent.len(), 1, "slot 2 throws in its press frame");
        assert_eq!(summary(&wire(&sent[0])).1, 1, "the throw consumes an item");
    }
}

#[test]
fn native_rearm_applies_to_fresh_presses_and_holds_after_slot_changes() {
    for fresh_press in [false, true] {
        let mut runtime = ItemUseRuntime::default();
        let first = item_frame(1, true, stack(0, SNOWBALL, 16), "minecraft:snowball");
        runtime.observe_press(true);
        assert!(runtime.step(&first).swung);
        let switched = UseFrame {
            tick: 2,
            now_millis: first.now_millis + 50,
            ..item_frame(2, true, stack(1, MENU_ITEM, 16), "minecraft:splash_potion")
        };
        runtime.observe_press(fresh_press);
        assert!(runtime.step(&switched).packets.is_empty());
        runtime.observe_press(fresh_press);
        assert!(
            runtime
                .step(&UseFrame {
                    tick: 5,
                    now_millis: first.now_millis + USE_REARM_MILLIS,
                    ..switched.clone()
                })
                .packets
                .is_empty()
        );
        runtime.observe_press(fresh_press);
        assert!(
            runtime
                .step(&UseFrame {
                    tick: 5,
                    now_millis: first.now_millis + USE_REARM_MILLIS + 1,
                    ..switched
                })
                .swung
        );
    }
}

#[test]
fn chorus_fruit_cooldown_survives_release_and_reselection() {
    let mut runtime = ItemUseRuntime::default();
    runtime.set_delay_fix(true);
    let first = item_frame(1, true, stack(0, MENU_ITEM, 16), "minecraft:chorus_fruit");
    runtime.observe_press(true);
    assert!(runtime.step(&first).started);
    runtime.step(&UseFrame {
        tick: 2,
        now_millis: 100,
        held: false,
        ..first.clone()
    });
    let switched = item_frame(2, true, stack(1, MENU_ITEM, 16), "minecraft:chorus_fruit");
    runtime.observe_press(true);
    assert!(
        !runtime.step(&switched).started,
        "the category cooldown covers another slot"
    );
    let throw = item_frame(3, false, stack(2, SNOWBALL, 16), "minecraft:snowball");
    runtime.observe_press(true);
    assert!(
        runtime.step(&throw).swung,
        "chorus fruit does not cool down snowballs"
    );
    let cooldown = first.air_use.unwrap().cooldown().unwrap();
    let until = first.tick + u64::from(cooldown.ticks);
    runtime.observe_press(true);
    assert!(
        runtime
            .step(&UseFrame {
                tick: until,
                now_millis: until * 50,
                ..switched
            })
            .started
    );
}

#[test]
fn slot_change_throws_keep_cooldowns_in_their_item_categories() {
    for identifier in ["minecraft:ender_pearl", "minecraft:wind_charge"] {
        let mut runtime = ItemUseRuntime::default();
        runtime.set_delay_fix(true);
        let first = item_frame(1, false, stack(0, SNOWBALL, 16), identifier);
        let cooldown = first.air_use.unwrap().cooldown().unwrap();
        runtime.observe_press(true);
        assert!(runtime.step(&first).swung);
        let blocked = UseFrame {
            now_millis: first.now_millis + 16,
            ..item_frame(1, false, stack(1, SNOWBALL, 16), identifier)
        };
        runtime.observe_press(true);
        let outcome = runtime.step(&blocked);
        assert!(!outcome.swung);
        assert_eq!(kinds(&outcome), ["use"]);
        assert_eq!(summary(&wire(&outcome.packets[0])).1, 0);
        assert_eq!(runtime.cooldown_progress(cooldown, first.tick), 1.0);
        for (slot, other) in [
            (2, "minecraft:splash_potion"),
            (3, "minecraft:snowball"),
            (4, "minecraft:egg"),
        ] {
            let unrelated = UseFrame {
                now_millis: blocked.now_millis + u64::from(slot) * 16,
                ..item_frame(1, false, stack(slot, MENU_ITEM, 16), other)
            };
            runtime.observe_press(true);
            assert!(
                runtime.step(&unrelated).swung,
                "{identifier} does not cool down {other}"
            );
        }
        let until = first.tick + u64::from(cooldown.ticks);
        let before = item_frame(until - 1, false, stack(5, SNOWBALL, 16), identifier);
        runtime.observe_press(true);
        assert!(!runtime.step(&before).swung);
        let expired = item_frame(until, false, stack(6, SNOWBALL, 16), identifier);
        runtime.observe_press(true);
        assert!(runtime.step(&expired).swung);
    }
}

#[test]
fn a_rejected_chorus_fruit_use_starts_no_cooldown() {
    let mut runtime = ItemUseRuntime::default();
    let mut swings = SwingTracker::default();
    let first = item_frame(1, true, stack(0, MENU_ITEM, 16), "minecraft:chorus_fruit");
    let cooldown = first.air_use.unwrap().cooldown().unwrap();
    runtime.observe_press(true);
    assert!(!step_and_send(
        &mut runtime,
        &mut swings,
        &first,
        7,
        6,
        |_| { Err(BatchSendError::Full) }
    ));
    assert!(!runtime.is_using());
    assert_eq!(runtime.cooldown_progress(cooldown, first.tick), 0.0);
    assert!(step_and_send(
        &mut runtime,
        &mut swings,
        &first,
        7,
        6,
        |_| Ok(())
    ));
    assert!(runtime.is_using());
    assert_eq!(runtime.cooldown_progress(cooldown, first.tick), 1.0);
}
