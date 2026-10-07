use super::*;

/// Exercises gameplay against explicit bounded admission outcomes.
fn send_use(
    runtime: &mut ItemUseRuntime,
    swings: &mut SwingTracker,
    frame: &UseFrame,
    network: &AdmissionQueue,
) -> bool {
    step_and_send(runtime, swings, frame, 7, 6, |packets| {
        network.send_inventory_packets(packets)
    })
}

/// A rejected throw of the last item leaves the slot for the ledger untouched.
#[test]
fn a_rejected_last_throw_reports_no_emptied_slot() {
    let (full, _open) = AdmissionQueue::with_command_capacity(1);
    let (ready, _open) = AdmissionQueue::with_command_capacity(2);
    let mut runtime = ItemUseRuntime::default();
    let mut swings = SwingTracker::default();
    let last = item_frame(100, false, stack(4, SNOWBALL, 1), "minecraft:snowball");
    runtime.observe_press(true);
    send_use(&mut runtime, &mut swings, &last, &full);
    assert_eq!(runtime.take_emptied_slot(), None);
    send_use(&mut runtime, &mut swings, &last, &ready);
    assert_eq!(runtime.take_emptied_slot(), Some((4, 1)));
}

#[test]
fn a_throw_never_queues_its_swing_without_its_transaction() {
    let (full, _open) = AdmissionQueue::with_command_capacity(1);
    let mut runtime = ItemUseRuntime::default();
    let mut swings = SwingTracker::default();
    let throw = |tick, held| item_frame(tick, held, stack(4, SNOWBALL, 16), "minecraft:snowball");
    runtime.observe_press(true);
    assert!(!send_use(
        &mut runtime,
        &mut swings,
        &throw(100, true),
        &full
    ));
    assert_eq!(
        full.pending_command_count(),
        0,
        "the whole batch is rejected"
    );
    assert_eq!(swings.take_started(), None);
    assert!(runtime.predicted.is_none());
    assert!(runtime.rearm_millis.is_none());
    assert_eq!(runtime.last_legacy_request_id, 0);

    let (ready, _open) = AdmissionQueue::with_command_capacity(2);
    let mut sent = Vec::new();
    step_and_send(
        &mut runtime,
        &mut swings,
        &throw(105, false),
        7,
        6,
        |packets| {
            sent = packets.iter().map(wire).collect();
            ready.send_inventory_packets(packets)
        },
    );
    assert_eq!(ready.pending_command_count(), 2);
    assert_eq!(swings.take_started(), Some(6));
    assert!(sent[0].contains("AnimatePacket"));
    assert_eq!(
        summary(&sent[1]),
        (
            "-4".to_owned(),
            1,
            vec!["16".to_owned(), "15".to_owned(), "16".to_owned()],
            vec![
                "Some(41)".to_owned(),
                "Some(-4)".to_owned(),
                "Some(41)".to_owned()
            ],
        )
    );
    assert!(
        !runtime.has_work(false),
        "the admitted press does not repeat"
    );

    runtime.observe_press(true);
    send_use(&mut runtime, &mut swings, &throw(110, false), &full);
    assert_eq!(runtime.predicted.as_ref().unwrap().stack.count(), 15);
    assert_eq!(runtime.last_legacy_request_id, -4);
    let (ready, _open) = AdmissionQueue::with_command_capacity(2);
    step_and_send(
        &mut runtime,
        &mut swings,
        &throw(111, false),
        7,
        6,
        |packets| {
            let (id, _, sizes, ids) = summary(&wire(packets.last().unwrap()));
            assert_eq!(id, "-6");
            assert_eq!((sizes[0].as_str(), ids[0].as_str()), ("15", "Some(-4)"));
            ready.send_inventory_packets(packets)
        },
    );
    assert_eq!(runtime.predicted.as_ref().unwrap().stack.count(), 14);
}

#[test]
fn a_rejected_pearl_does_not_start_its_cooldown() {
    let (full, _open) = AdmissionQueue::with_command_capacity(1);
    let mut runtime = ItemUseRuntime::default();
    let mut swings = SwingTracker::default();
    let pearl = |tick| item_frame(tick, false, stack(2, 422, 16), "minecraft:ender_pearl");
    runtime.observe_press(true);
    send_use(&mut runtime, &mut swings, &pearl(100), &full);
    assert!(runtime.cooldowns.is_empty());
    let (ready, _open) = AdmissionQueue::with_command_capacity(2);
    send_use(&mut runtime, &mut swings, &pearl(110), &ready);
    assert_eq!(runtime.cooldowns, [("ender_pearl", 130)]);
    assert_eq!(ready.pending_command_count(), 2);
    assert_eq!(runtime.predicted.as_ref().unwrap().stack.count(), 15);
}

#[test]
fn a_rejected_hold_starts_only_on_the_admitted_tick() {
    let (full, _open) = AdmissionQueue::with_command_capacity(1);
    full.send_inventory_packet(protocol::swing_arm_packet(
        7,
        protocol::SwingSource::ThrowItem,
    ))
    .unwrap();
    let mut runtime = ItemUseRuntime::default();
    let mut swings = SwingTracker::default();
    runtime.observe_press(true);
    assert!(!send_use(
        &mut runtime,
        &mut swings,
        &frame(100, true),
        &full
    ));
    assert!(!runtime.is_using());
    assert_eq!(runtime.movement_modifier(), None);
    assert!(runtime.has_work(false));

    let (ready, _open) = AdmissionQueue::with_command_capacity(1);
    assert!(send_use(
        &mut runtime,
        &mut swings,
        &frame(105, true),
        &ready
    ));
    assert_eq!(runtime.active.as_ref().unwrap().started_tick, 105);
    assert_eq!(runtime.movement_modifier(), Some(0.35));
}

#[test]
fn a_rejected_release_still_precedes_a_subsequent_press() {
    let (full, _open) = AdmissionQueue::with_command_capacity(1);
    let mut runtime = ItemUseRuntime::default();
    let mut swings = SwingTracker::default();
    let bread = |tick, held| item_frame(tick, held, stack(0, 257, 8), "minecraft:bread");
    runtime.observe_press(true);
    assert!(send_use(
        &mut runtime,
        &mut swings,
        &bread(100, true),
        &full
    ));
    assert!(!send_use(
        &mut runtime,
        &mut swings,
        &bread(110, false),
        &full
    ));
    assert!(runtime.is_using());

    runtime.observe_press(true);
    let (ready, _open) = AdmissionQueue::with_command_capacity(2);
    let mut sent = Vec::new();
    assert!(step_and_send(
        &mut runtime,
        &mut swings,
        &bread(111, true),
        7,
        6,
        |packets| {
            sent = packets.iter().map(wire).collect();
            ready.send_inventory_packets(packets)
        }
    ));
    assert_eq!(sent.len(), 2);
    assert!(sent[0].contains("action_type: Release"));
    assert!(sent[1].contains("ItemUseInventoryTransaction("));
    assert_eq!(runtime.active.as_ref().unwrap().started_tick, 111);
    assert!(!runtime.release_pending);
    let (ready, _open) = AdmissionQueue::with_command_capacity(1);
    assert!(send_use(
        &mut runtime,
        &mut swings,
        &bread(143, true),
        &ready
    ));
    assert_eq!(runtime.active.as_ref().unwrap().started_tick, 143);
}

#[test]
fn a_release_can_be_admitted_while_its_deferred_restart_waits_for_verification() {
    let (full, _open) = AdmissionQueue::with_command_capacity(1);
    let mut runtime = ItemUseRuntime::default();
    let mut swings = SwingTracker::default();
    let crossbow = |tick, held| UseFrame {
        air_use: classify("minecraft:crossbow", false, 0, None),
        ..frame(tick, held)
    };
    runtime.observe_press(true);
    assert!(send_use(
        &mut runtime,
        &mut swings,
        &crossbow(100, true),
        &full
    ));
    send_use(&mut runtime, &mut swings, &crossbow(110, false), &full);
    runtime.observe_press(true);
    send_use(&mut runtime, &mut swings, &crossbow(111, true), &full);

    let hidden = UseFrame {
        selection: None,
        ..crossbow(112, true)
    };
    let (release_queue, _open) = AdmissionQueue::with_command_capacity(1);
    assert!(!step_and_send(
        &mut runtime,
        &mut swings,
        &hidden,
        7,
        6,
        |packets| {
            assert_eq!(packets.len(), 1);
            assert!(wire(&packets[0]).contains("action_type: Release"));
            release_queue.send_inventory_packets(packets)
        }
    ));
    assert!(!runtime.is_using());
    assert!(runtime.has_work(false));
    assert!(!runtime.release_pending);
    assert!(!step_and_send(
        &mut runtime,
        &mut swings,
        &hidden,
        7,
        6,
        |_| { panic!("the deferred restart still needs its verified selection") }
    ));

    let (ready, _open) = AdmissionQueue::with_command_capacity(1);
    assert!(send_use(
        &mut runtime,
        &mut swings,
        &crossbow(113, true),
        &ready
    ));
    assert_eq!(runtime.active.as_ref().unwrap().started_tick, 113);
    assert!(!runtime.latched_press);
    assert!(runtime.deferred_selection.is_none());
}

#[test]
fn a_deferred_click_waits_for_verification_and_cancels_on_selection_change() {
    let (full, _open) = AdmissionQueue::with_command_capacity(1);
    let mut runtime = ItemUseRuntime::default();
    let mut swings = SwingTracker::default();
    let original = item_frame(100, true, stack(4, SNOWBALL, 16), "minecraft:snowball");
    runtime.observe_press(true);
    send_use(&mut runtime, &mut swings, &original, &full);
    assert!(!step_and_send(
        &mut runtime,
        &mut swings,
        &UseFrame {
            selection: None,
            ..original.clone()
        },
        7,
        6,
        |_| panic!("an unverified click must wait")
    ));
    assert!(runtime.has_work(false));

    let replaced = item_frame(105, false, stack(4, SNOWBALL, 15), "minecraft:snowball");
    assert!(!step_and_send(
        &mut runtime,
        &mut swings,
        &replaced,
        7,
        6,
        |_| { panic!("a replaced stack must not receive the old press") }
    ));
    assert!(!runtime.has_work(false));
    assert!(runtime.predicted.is_none());
}

#[test]
fn focus_loss_cancels_a_deferred_click_but_releases_an_admitted_use() {
    let (full, _open) = AdmissionQueue::with_command_capacity(1);
    let mut runtime = ItemUseRuntime::default();
    let mut swings = SwingTracker::default();
    runtime.observe_press(true);
    send_use(
        &mut runtime,
        &mut swings,
        &item_frame(100, true, stack(4, SNOWBALL, 16), "minecraft:snowball"),
        &full,
    );
    runtime.cancel_pending_input();
    assert!(!runtime.has_work(false));

    runtime.observe_press(true);
    assert!(send_use(
        &mut runtime,
        &mut swings,
        &frame(105, true),
        &full
    ));
    runtime.cancel_pending_input();
    let (ready, _open) = AdmissionQueue::with_command_capacity(1);
    assert!(!send_use(
        &mut runtime,
        &mut swings,
        &frame(110, false),
        &ready
    ));
    assert_eq!(ready.pending_command_count(), 1);
    assert!(!runtime.is_using());
}

#[test]
fn a_closed_connection_drops_a_deferred_use_without_starting_it() {
    let (full, _open) = AdmissionQueue::with_command_capacity(1);
    full.send_inventory_packet(protocol::swing_arm_packet(
        7,
        protocol::SwingSource::ThrowItem,
    ))
    .unwrap();
    let mut runtime = ItemUseRuntime::default();
    let mut swings = SwingTracker::default();
    runtime.observe_press(true);
    send_use(&mut runtime, &mut swings, &frame(100, true), &full);
    assert!(!send_use(
        &mut runtime,
        &mut swings,
        &frame(105, true),
        &AdmissionQueue::disconnected()
    ));
    assert!(!runtime.is_using());
    assert!(!runtime.has_work(false));
    assert_eq!(runtime.movement_modifier(), None);
    assert_eq!(swings.take_started(), None);
}

#[test]
fn a_new_session_cancels_a_deferred_click() {
    let (full, _open) = AdmissionQueue::with_command_capacity(1);
    let mut runtime = ItemUseRuntime::default();
    let mut swings = SwingTracker::default();
    runtime.synchronize(1);
    runtime.observe_press(true);
    send_use(
        &mut runtime,
        &mut swings,
        &item_frame(100, true, stack(4, SNOWBALL, 16), "minecraft:snowball"),
        &full,
    );
    runtime.synchronize(2);
    assert!(!runtime.has_work(false));
    assert!(runtime.deferred_selection.is_none());
    assert!(!runtime.release_pending);
}

/// An accepted use marks its own unsent sample after the standalone batch is admitted.
#[test]
fn accepted_use_updates_only_its_exact_tick_before_the_movement_flush() {
    use crate::test_support::survival_mining::{evidence, ticker_with_ticks};
    for admission in [
        Ok(()),
        Err(BatchSendError::Full),
        Err(BatchSendError::Closed),
    ] {
        let mut runtime = ItemUseRuntime::default();
        runtime.observe_press(true);
        let mut swings = SwingTracker::default();
        let mut movement = ticker_with_ticks(2);
        let tick = movement.newest_unsent_sample().unwrap().tick;
        let use_frame = frame(tick, true);
        let before = movement.pending_snapshots();
        let mut packet_order = Vec::new();
        admit_on_tick(
            &mut runtime,
            &mut swings,
            &mut movement,
            &use_frame,
            1,
            6,
            |packets| {
                if admission.is_ok() {
                    packet_order.extend(packets.into_iter().map(|_| "item"));
                }
                admission
            },
        );
        let after = movement.pending_snapshots();
        assert_eq!(after[0], before[0]);
        for (previous, current) in before.iter().zip(&after) {
            let expected = if admission.is_ok() && current.tick == tick {
                previous.flags | protocol::PlayerInputFlags::START_USING_ITEM
            } else {
                previous.flags
            };
            assert_eq!(current.flags, expected);
        }
        assert_eq!(runtime.is_using(), admission.is_ok());
        crate::movement::flush_player_auth_inputs(&mut movement, 8, Some(evidence()), |_, _| {
            packet_order.push("movement");
            Ok::<_, ()>(())
        })
        .unwrap();
        if admission.is_ok() {
            assert_eq!(packet_order, ["item", "movement", "movement"]);
        } else {
            assert_eq!(packet_order, ["movement", "movement"]);
        }
    }
}

/// A throw retry preserves its swing when render publication occurred between sends.
#[test]
fn a_backpressured_throw_retries_after_its_tick_was_published() {
    let mut runtime = ItemUseRuntime::default();
    let mut swings = SwingTracker::default();
    let effects = crate::movement::LocalMovementEffectTimeline::default();
    let throw = item_frame(100, true, stack(4, SNOWBALL, 16), "minecraft:snowball");
    swings.sync_ticks((1, 1), throw.tick, &effects);
    runtime.observe_press(true);
    step_and_send(&mut runtime, &mut swings, &throw, 7, 6, |packets| {
        assert_eq!(packets.len(), 2);
        Err(BatchSendError::Full)
    });
    assert_eq!(swings.take_started(), None);
    assert_eq!(swings.published_progress(throw.tick).java, [0.0; 2]);
    step_and_send(&mut runtime, &mut swings, &throw, 7, 6, |packets| {
        assert_eq!(packets.len(), 2, "the throw retry retains its swing");
        assert_eq!(format!("{:?}", packets[0].header.id), "AnimatePacket");
        assert_eq!(
            format!("{:?}", packets[1].header.id),
            "InventoryTransactionPacket"
        );
        Ok(())
    });
    assert_eq!(swings.take_started(), Some(6));
    assert!(!swings.try_swing(throw.tick, 6));
    assert_eq!(
        swings.published_progress(throw.tick + 1).java,
        [0.0, 1.0 / 6.0]
    );
}

#[test]
fn a_fresh_throw_waits_for_an_unpublished_tick() {
    let mut runtime = ItemUseRuntime::default();
    let mut swings = SwingTracker::default();
    let effects = crate::movement::LocalMovementEffectTimeline::default();
    swings.sync_ticks((1, 1), 100, &effects);
    swings.published_progress(100);
    runtime.observe_press(true);
    let throw = |tick| item_frame(tick, false, stack(4, SNOWBALL, 16), "minecraft:snowball");
    let mut sends = 0;
    step_and_send(&mut runtime, &mut swings, &throw(100), 7, 6, |_| {
        sends += 1;
        Ok(())
    });
    assert_eq!(
        sends, 0,
        "fresh throws wait instead of omitting their swing"
    );
    assert!(runtime.has_work(false));
    assert!(runtime.predicted.is_none());
    assert_eq!(runtime.last_legacy_request_id, 0);
    step_and_send(&mut runtime, &mut swings, &throw(101), 7, 6, |packets| {
        assert_eq!(packets.len(), 2);
        sends += 1;
        Ok(())
    });
    assert_eq!(sends, 1);
    assert_eq!(swings.take_started(), Some(6));
    assert!(!runtime.has_work(false));
}

#[test]
fn another_action_cannot_authorize_a_fresh_throw_on_a_published_tick() {
    let mut runtime = ItemUseRuntime::default();
    let mut swings = SwingTracker::default();
    let effects = crate::movement::LocalMovementEffectTimeline::default();
    swings.sync_ticks((1, 1), 100, &effects);
    let mut rejected = swings.clone();
    assert!(rejected.try_swing(100, 6));
    swings.defer_unadmitted_attempt(&rejected);
    swings.published_progress(100);
    runtime.observe_press(true);
    let throw = item_frame(100, false, stack(4, SNOWBALL, 16), "minecraft:snowball");
    let mut sends = 0;
    step_and_send(&mut runtime, &mut swings, &throw, 7, 6, |_| {
        sends += 1;
        Ok(())
    });
    assert_eq!(sends, 0);
    assert!(runtime.has_work(false));
    assert_eq!(swings.take_started(), None);
}

#[test]
fn a_new_press_cannot_reuse_a_canceled_throw_retry() {
    for cancellation in 0..3 {
        let mut runtime = ItemUseRuntime::default();
        runtime.synchronize(1);
        let mut swings = SwingTracker::default();
        let effects = crate::movement::LocalMovementEffectTimeline::default();
        swings.sync_ticks((1, 1), 100, &effects);
        let throw = item_frame(100, false, stack(4, SNOWBALL, 16), "minecraft:snowball");
        runtime.observe_press(true);
        step_and_send(&mut runtime, &mut swings, &throw, 7, 6, |_| {
            Err(BatchSendError::Full)
        });
        swings.published_progress(100);
        match cancellation {
            0 => runtime.cancel_pending_input(),
            1 => runtime.synchronize(2),
            _ => {}
        }
        runtime.observe_press(true);
        let mut sends = 0;
        step_and_send(&mut runtime, &mut swings, &throw, 7, 6, |_| {
            sends += 1;
            Ok(())
        });
        assert_eq!(sends, 0);
        assert!(runtime.has_work(false));
    }
}

#[test]
fn a_published_tick_does_not_block_a_use_without_a_swing() {
    let mut runtime = ItemUseRuntime::default();
    let mut swings = SwingTracker::default();
    let effects = crate::movement::LocalMovementEffectTimeline::default();
    swings.sync_ticks((1, 1), 100, &effects);
    swings.published_progress(100);
    runtime.observe_press(true);
    assert!(step_and_send(
        &mut runtime,
        &mut swings,
        &frame(100, true),
        7,
        6,
        |packets| {
            assert_eq!(packets.len(), 1);
            Ok(())
        }
    ));
    assert!(runtime.is_using());
    assert_eq!(swings.take_started(), None);
}

#[test]
fn a_position_correction_revokes_an_old_throw_retry() {
    let mut runtime = ItemUseRuntime::default();
    runtime.synchronize(1);
    let mut swings = SwingTracker::default();
    let effects = crate::movement::LocalMovementEffectTimeline::default();
    let throw = item_frame(100, false, stack(4, SNOWBALL, 16), "minecraft:snowball");
    swings.sync_ticks((1, 1), 100, &effects);
    runtime.observe_press(true);
    step_and_send(&mut runtime, &mut swings, &throw, 7, 6, |_| {
        Err(BatchSendError::Full)
    });
    swings.published_progress(100);
    swings.sync_ticks((1, 2), 100, &effects);
    swings.published_progress(100);
    let mut sends = 0;
    step_and_send(&mut runtime, &mut swings, &throw, 7, 6, |_| {
        sends += 1;
        Ok(())
    });
    assert_eq!(
        sends, 0,
        "the corrected authority cannot retry the old swing"
    );
    assert!(runtime.has_work(false));
}
