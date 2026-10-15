use super::*;

/// Supplies wet trident eligibility on the retained selected stack.
fn trident(tick: u64, held: bool) -> UseFrame {
    UseFrame {
        riptide: Some(RiptideUse {
            level: 3,
            wet: true,
            mounted: false,
            usable: true,
        }),
        ..item_frame(tick, held, selection(2, MENU_ITEM), "minecraft:trident")
    }
}

#[test]
fn respawn_cancels_active_and_backpressured_riptide_releases_without_rearming_holds() {
    for rejected_release in [false, true] {
        let mut runtime = ItemUseRuntime::default();
        let mut swings = SwingTracker::default();
        runtime.observe_press(true);
        assert!(step_and_send(
            &mut runtime,
            &mut swings,
            &trident(100, true),
            7,
            6,
            |_| Ok(())
        ));
        if rejected_release {
            step_and_send(
                &mut runtime,
                &mut swings,
                &trident(110, false),
                7,
                6,
                |_| Err(BatchSendError::Full),
            );
            runtime.observe_press(true);
        }
        runtime.cancel_for_respawn();
        assert_eq!(runtime.active_timing(), None);
        assert_eq!(runtime.movement_modifier(), None);
        assert_eq!(runtime.take_riptide_launch(), None);
        for (tick, held) in [(150, true), (151, false)] {
            step_and_send(
                &mut runtime,
                &mut swings,
                &trident(tick, held),
                7,
                6,
                |_| panic!("old-life actions cannot retry"),
            );
            assert!(!runtime.is_using());
            assert_eq!(runtime.take_riptide_launch(), None);
        }
        runtime.observe_press(true);
        assert!(step_and_send(
            &mut runtime,
            &mut swings,
            &trident(160, true),
            7,
            6,
            |_| Ok(())
        ));
        assert_eq!(runtime.active_timing().unwrap().0, 160);
        runtime.step(&trident(169, false));
        assert_eq!(
            runtime.take_riptide_launch(),
            None,
            "new charges start from zero"
        );
    }
}

#[test]
fn respawn_discards_rejected_clicks_and_unconsumed_launches() {
    let mut runtime = ItemUseRuntime::default();
    let mut swings = SwingTracker::default();
    runtime.observe_press(true);
    step_and_send(&mut runtime, &mut swings, &trident(100, true), 7, 6, |_| {
        Err(BatchSendError::Full)
    });
    runtime.cancel_for_respawn();
    step_and_send(&mut runtime, &mut swings, &trident(110, true), 7, 6, |_| {
        panic!("a queued old-life press cannot retry")
    });
    assert!(!runtime.is_using());
    runtime.observe_press(true);
    runtime.step(&trident(120, true));
    runtime.step(&trident(130, false));
    assert_eq!(runtime.riptide_launch.map(|launch| launch.level), Some(3));
    runtime.cancel_for_respawn();
    assert_eq!(runtime.take_riptide_launch(), None);
}

#[test]
fn respawn_preserves_session_inventory_cooldowns_and_rearm_policy() {
    let mut runtime = ItemUseRuntime::default();
    runtime.synchronize(7);
    runtime.set_delay_fix(true);
    runtime.observe_press(true);
    runtime.step(&item_frame(
        100,
        true,
        stack(0, SNOWBALL, 2),
        "minecraft:ender_pearl",
    ));
    let change = runtime.inventory_change.clone();
    let predicted = runtime.predicted.clone();
    let cooldowns = runtime.cooldowns.clone();
    assert!(change.is_some() && predicted.is_some() && !cooldowns.is_empty());
    let rearm = runtime.rearm_millis;
    runtime.observe_selected_slot(Some(1));
    runtime.cancel_for_respawn();
    assert_eq!(runtime.session, Some(7));
    assert!(runtime.delay_fix && runtime.slot_change_pending);
    assert_eq!(runtime.selected_slot, Some(1));
    assert_eq!(runtime.rearm_millis, rearm);
    assert_eq!(runtime.cooldowns, cooldowns);
    assert_eq!(runtime.predicted, predicted);
    assert_eq!(runtime.take_inventory_change(), change);
}
