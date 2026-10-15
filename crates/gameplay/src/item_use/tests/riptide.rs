use super::*;

#[path = "riptide/snapshot.rs"]
mod snapshot;

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
        assert_eq!(
            runtime.take_riptide_launch().map(|launch| launch.level),
            launch
        );
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
        assert_eq!(
            runtime.take_riptide_launch().map(|launch| launch.level),
            launch
        );
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

#[test]
fn deferred_riptide_release_keeps_edge_eligibility_and_cannot_ignore_reselection() {
    for ticks in [RIPTIDE_CHARGE_TICKS - 1, RIPTIDE_CHARGE_TICKS] {
        for invalid in ["none", "dry", "mounted", "broken", "hidden", "reselected"] {
            let mut runtime = ItemUseRuntime::default();
            runtime.observe_press(true);
            runtime.step(&trident(100, true, 3, true));
            let mut edge = trident(100 + ticks, false, 3, true);
            match invalid {
                "dry" => edge.riptide.as_mut().unwrap().wet = false,
                "mounted" => edge.riptide.as_mut().unwrap().mounted = true,
                "broken" => edge.riptide.as_mut().unwrap().usable = false,
                "hidden" => edge.selection = None,
                "reselected" => edge.selection.as_mut().unwrap().slot += 1,
                _ => {}
            }
            runtime.observe_riptide_release(
                edge.tick,
                edge.held,
                edge.riptide,
                edge.selection.as_ref(),
                edge.riptide_facing,
            );
            let later = trident(150, false, 3, true);
            runtime.observe_riptide_release(
                later.tick,
                later.held,
                later.riptide,
                later.selection.as_ref(),
                later.riptide_facing,
            );
            assert_eq!(runtime.is_using(), invalid != "reselected");
            assert_eq!(runtime.take_riptide_launch(), None);
            // Recovery of the environment and selection must not upgrade the observed edge.
            let outcome = runtime.step(&trident(160, false, 3, true));
            assert_eq!(
                kinds(&outcome),
                if invalid == "reselected" {
                    vec![]
                } else {
                    vec!["release"]
                }
            );
            assert_eq!(
                runtime.take_riptide_launch().map(|launch| launch.level),
                (invalid == "none" && ticks == RIPTIDE_CHARGE_TICKS).then_some(3)
            );
            assert!(!runtime.is_using());
        }
    }
    let mut runtime = ItemUseRuntime::default();
    runtime.observe_press(true);
    runtime.step(&trident(100, true, 3, true));
    let edge = trident(110, false, 3, true);
    runtime.observe_riptide_release(
        edge.tick,
        edge.held,
        edge.riptide,
        edge.selection.as_ref(),
        edge.riptide_facing,
    );
    let mut changed = trident(150, false, 3, true);
    changed.selection.as_mut().unwrap().slot += 1;
    assert!(runtime.step(&changed).packets.is_empty());
    assert_eq!(runtime.take_riptide_launch(), None);
}

#[test]
fn observing_riptide_release_facts_leaves_other_held_items_on_their_admission_clock() {
    for identifier in ["minecraft:bow", "minecraft:crossbow"] {
        let mut runtime = ItemUseRuntime::default();
        runtime.observe_press(true);
        let start = item_frame(100, true, selection(2, MENU_ITEM), identifier);
        assert!(runtime.step(&start).started);
        runtime.observe_riptide_release(
            101,
            false,
            None,
            start.selection.as_ref(),
            start.riptide_facing,
        );
        assert!(
            runtime
                .step(&UseFrame {
                    tick: 101,
                    ..start.clone()
                })
                .packets
                .is_empty()
        );
        assert!(runtime.is_using());
        assert_eq!(
            kinds(&runtime.step(&UseFrame {
                tick: 102,
                held: false,
                ..start
            })),
            ["release"]
        );
        assert_eq!(runtime.take_riptide_launch(), None);
    }
}
