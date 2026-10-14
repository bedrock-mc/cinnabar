use super::*;

/// Supplies a trident with authoritative enchantment and environment observations.
fn trident(tick: u64, held: bool, level: u8, wet: bool) -> UseFrame {
    UseFrame {
        riptide: Some(RiptideUse {
            level,
            wet,
            mounted: false,
            usable: true,
        }),
        ..item_frame(tick, held, selection(2, MENU_ITEM), "minecraft:trident")
    }
}

#[test]
fn riptide_requires_ten_charge_ticks_and_wetness_at_both_edges() {
    for (ticks, wet_start, wet_release, launch) in [
        (9, true, true, None),
        (10, true, true, Some(3)),
        (30, false, true, None),
        (30, true, false, None),
    ] {
        let mut runtime = ItemUseRuntime::default();
        runtime.observe_press(true);
        let started = runtime.step(&trident(100, true, 3, wet_start));
        assert_eq!(started.started, wet_start);
        assert_eq!(runtime.take_riptide_launch(), None);
        let released = runtime.step(&trident(100 + ticks, false, 3, wet_release));
        assert_eq!(
            kinds(&released),
            if wet_start { vec!["release"] } else { vec![] }
        );
        assert_eq!(runtime.take_riptide_launch(), launch);
        assert_eq!(runtime.take_riptide_launch(), None);
    }
}

#[test]
fn rejected_release_cannot_launch_or_turn_a_short_charge_into_a_full_one() {
    for (charge, launch) in [(9, None), (10, Some(2))] {
        let mut runtime = ItemUseRuntime::default();
        let mut swings = SwingTracker::default();
        runtime.observe_press(true);
        step_and_send(
            &mut runtime,
            &mut swings,
            &trident(100, true, 2, true),
            7,
            6,
            |_| Ok(()),
        );
        step_and_send(
            &mut runtime,
            &mut swings,
            &trident(100 + charge, false, 2, true),
            7,
            6,
            |_| Err(BatchSendError::Full),
        );
        assert_eq!(runtime.take_riptide_launch(), None);
        step_and_send(
            &mut runtime,
            &mut swings,
            &trident(150, false, 2, true),
            7,
            6,
            |_| Ok(()),
        );
        assert_eq!(runtime.take_riptide_launch(), launch);
        assert!(!runtime.is_using());
    }
}

#[test]
fn ordinary_tridents_mounts_reselection_and_broken_stacks_do_not_launch() {
    for level in [0, 3] {
        let mut runtime = ItemUseRuntime::default();
        runtime.observe_press(true);
        runtime.step(&trident(100, true, level, true));
        let mut release = trident(120, false, level, true);
        release.riptide.as_mut().unwrap().mounted = true;
        runtime.step(&release);
        assert_eq!(runtime.take_riptide_launch(), None);
    }
    let mut runtime = ItemUseRuntime::default();
    runtime.observe_press(true);
    let mut broken = trident(100, true, 3, true);
    broken.riptide.as_mut().unwrap().usable = false;
    assert!(!runtime.step(&broken).started);
    runtime.observe_press(true);
    runtime.step(&trident(110, true, 3, true));
    let mut switched = trident(130, false, 3, true);
    switched.selection.as_mut().unwrap().slot += 1;
    assert!(runtime.step(&switched).packets.is_empty());
    assert_eq!(runtime.take_riptide_launch(), None);
}
