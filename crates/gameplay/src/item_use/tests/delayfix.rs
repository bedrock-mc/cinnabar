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
fn delay_fix_allows_slot_hopping_but_never_two_uses_in_one_tick() {
    let mut runtime = ItemUseRuntime::default();
    runtime.set_delay_fix(true);
    for (tick, slot) in [(1, 2), (2, 3), (3, 2), (4, 3)] {
        let first = item_frame(tick, false, stack(slot, SNOWBALL, 16), "minecraft:snowball");
        runtime.observe_press(true);
        assert!(runtime.step(&first).swung);
        runtime.observe_press(true);
        assert!(
            runtime
                .step(&UseFrame {
                    selection: Some(stack(if slot == 2 { 3 } else { 2 }, SNOWBALL, 16)),
                    ..first
                })
                .packets
                .is_empty()
        );
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
