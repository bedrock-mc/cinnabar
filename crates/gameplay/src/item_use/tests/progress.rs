use super::*;
use assets::facts::ItemUseAnimationFact;

/// Starts a thirty-two-tick accepted food use independently of frame cadence.
fn eating() -> (ItemUseRuntime, FrozenMiningSelection) {
    let selection = stack(0, 300, 2);
    let mut runtime = ItemUseRuntime::default();
    runtime.synchronize(7);
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
    (runtime, selection)
}

/// Sends one tick with the fixture's currently selected item and animation.
fn send(
    runtime: &mut ItemUseRuntime,
    tick: u64,
    selection: Option<&FrozenMiningSelection>,
    animation: Option<ItemUseAnimationFact>,
    packets: &mut Vec<protocol::Packet>,
) {
    runtime
        .send_progress_tick(tick, 42, selection, animation, |packet| {
            packets.push(packet);
            Ok::<_, ()>(())
        })
        .unwrap();
}

#[test]
fn use_progress_reports_remaining_duration_after_each_completed_tick() {
    for animation in [ItemUseAnimationFact::Eat, ItemUseAnimationFact::Drink] {
        let (mut runtime, selection) = eating();
        let mut ticks = Vec::new();
        let mut packets = Vec::new();
        for tick in 99..=140 {
            let before = packets.len();
            send(
                &mut runtime,
                tick,
                Some(&selection),
                Some(animation),
                &mut packets,
            );
            if packets.len() != before {
                ticks.push(tick);
            }
        }
        assert_eq!(ticks, [107, 111, 115, 119, 123, 127, 131]);
    }
}

#[test]
fn use_progress_retries_failed_admission_without_repeating_a_successful_event() {
    let (mut runtime, selection) = eating();
    assert_eq!(
        runtime.send_progress_tick(
            107,
            42,
            Some(&selection),
            Some(ItemUseAnimationFact::Eat),
            |_| Err("full")
        ),
        Err("full")
    );
    let mut packets = Vec::new();
    for tick in [107, 107, 106, 111] {
        send(
            &mut runtime,
            tick,
            Some(&selection),
            Some(ItemUseAnimationFact::Eat),
            &mut packets,
        );
    }
    assert_eq!(packets.len(), 2);
}

#[test]
fn use_progress_waits_for_a_rejected_release_before_reporting_a_new_use() {
    for animation in [ItemUseAnimationFact::Eat, ItemUseAnimationFact::Drink] {
        let (mut runtime, selection) = eating();
        let mut swings = SwingTracker::default();
        step_and_send(
            &mut runtime,
            &mut swings,
            &item_frame(106, false, selection.clone(), "minecraft:golden_apple"),
            42,
            6,
            |packets| {
                assert_eq!(packets.len(), 1);
                assert_eq!(use_kind(&packets[0]), "release");
                Err(BatchSendError::Full)
            },
        );
        assert!(runtime.is_using());
        let mut packets = Vec::new();
        for tick in [107, 111] {
            send(
                &mut runtime,
                tick,
                Some(&selection),
                Some(animation),
                &mut packets,
            );
        }
        assert!(packets.is_empty(), "a pending release has no new progress");

        runtime.observe_press(true);
        assert!(step_and_send(
            &mut runtime,
            &mut swings,
            &item_frame(112, true, selection.clone(), "minecraft:golden_apple"),
            42,
            6,
            |packets| {
                assert_eq!(
                    packets.iter().map(use_kind).collect::<Vec<_>>(),
                    ["release", "use"]
                );
                Ok(())
            },
        ));
        send(
            &mut runtime,
            119,
            Some(&selection),
            Some(animation),
            &mut packets,
        );
        assert_eq!(packets.len(), 1, "the admitted new use reports progress");
    }
}

#[test]
fn use_progress_stops_on_release_respawn_session_change_and_reselection() {
    for cancellation in 0..4 {
        let (mut runtime, selection) = eating();
        let mut selected = selection.clone();
        match cancellation {
            0 => {
                runtime.step(&item_frame(
                    106,
                    false,
                    selection.clone(),
                    "minecraft:golden_apple",
                ));
            }
            1 => runtime.cancel_for_respawn(),
            2 => runtime.synchronize(8),
            _ => selected.slot = 1,
        }
        let mut packets = Vec::new();
        send(
            &mut runtime,
            107,
            Some(&selected),
            Some(ItemUseAnimationFact::Eat),
            &mut packets,
        );
        send(
            &mut runtime,
            111,
            Some(&selection),
            Some(ItemUseAnimationFact::Eat),
            &mut packets,
        );
        assert!(packets.is_empty());
    }
}

#[test]
fn use_progress_requires_a_verified_eating_or_drinking_selection() {
    let (mut runtime, selection) = eating();
    let mut packets = Vec::new();
    for animation in [
        None,
        Some(ItemUseAnimationFact::Bow),
        Some(ItemUseAnimationFact::Crossbow),
        Some(ItemUseAnimationFact::None),
    ] {
        send(&mut runtime, 107, Some(&selection), animation, &mut packets);
    }
    send(
        &mut runtime,
        107,
        None,
        Some(ItemUseAnimationFact::Eat),
        &mut packets,
    );
    assert!(packets.is_empty());
    send(
        &mut runtime,
        107,
        Some(&selection),
        Some(ItemUseAnimationFact::Eat),
        &mut packets,
    );
    assert_eq!(packets.len(), 1);
}
