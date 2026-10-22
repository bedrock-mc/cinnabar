//! Server `UsingItem` writes in the flag history: captures, corrections,
//! replay carry and copy-back, and the hand-off to the item-use owner.

use super::*;

/// Runs one tick using an item or not, reported first as the item-use owner
/// publishes local starts and ends.
fn tick_using(
    physics: &mut LocalPhysicsController,
    ticker: &mut MovementTicker,
    using: bool,
) -> PhysicsMovementSample {
    physics.write_local_using_item(using);
    let input = MovementInput {
        item_use_movement_modifier: using.then_some(0.35),
        ..MovementInput::default()
    };
    tick_with(physics, ticker, input, PhysicsSampleContext::default())
}

/// An unstamped `UsingItem` write always reaches the item-use owner, which
/// compares it with its use; a stamped one only when it differs from the
/// following frame's capture.
#[test]
fn server_item_use_follows_the_local_use_at_its_stamp() {
    let (mut physics, mut ticker) = airborne();
    for using in [false, true, true] {
        tick_using(&mut physics, &mut ticker, using);
    }
    let using = |value| flags(|flags| flags.using_item = Some(value));
    // Each stamp compares with the following frame's capture: 102 and 103 use.
    for (tick, value, adopted) in [
        (101, true, None),
        (102, true, None),
        (101, false, Some(false)),
        (102, false, Some(false)),
        (0, true, Some(true)),
        (0, false, Some(false)),
    ] {
        physics.apply_server_movement_flags(tick, using(value));
        assert_eq!(
            physics.take_server_using_item(),
            adopted,
            "stamp {tick}, using {value}"
        );
    }
}

/// A later write with the same stamp compares with the frame's capture, as
/// vanilla's flag correction does, so a clear matching it leaves the start.
#[test]
fn a_same_stamp_clear_compares_with_the_frame_capture() {
    let (mut physics, mut ticker) = airborne();
    for _ in 0..3 {
        tick(&mut physics, &mut ticker, elytra(true));
    }
    let set = |value| {
        flags(|flags| {
            flags.gliding = Some(value);
            flags.spinning = Some(value);
            flags.using_item = Some(value);
        })
    };
    assert_eq!(physics.apply_server_movement_flags(101, set(true)), None);
    assert_eq!(physics.apply_server_movement_flags(101, set(false)), None);
    reconcile_timeline_rewind(&mut ticker, &mut physics, 101, &VersionedFloor(1)).unwrap();
    assert_eq!(physics.mode(), sim::MovementMode::Gliding);
    assert!(physics.state().unwrap().spin_attack_ticks.is_some());
    assert_eq!(physics.take_server_using_item(), Some(true));
    assert_eq!(count(&ticker, PlayerInputFlags::START_GLIDING), 0);
    assert_eq!(count(&ticker, PlayerInputFlags::START_SPIN_ATTACK), 0);
}

/// Respawn discards a server item-use write the old life left pending.
#[test]
fn respawn_discards_a_pending_server_item_use() {
    let (mut physics, mut ticker) = airborne();
    tick(&mut physics, &mut ticker, PhysicsSampleContext::default());
    let using = flags(|flags| flags.using_item = Some(true));
    physics.apply_server_movement_flags(0, using);
    ticker.discard_respawn_actions(&mut physics);
    assert_eq!(physics.take_server_using_item(), None);
}

/// One stamped update fixes its comparison bases before recording any field,
/// so an item-use change cannot make its pose compare with the live glide.
#[test]
fn one_update_compares_every_flag_with_the_same_base() {
    let (mut physics, mut ticker) = airborne();
    let jump = |jumping| MovementInput {
        jumping,
        ..MovementInput::default()
    };
    for jumping in [false, false, true] {
        tick_with(&mut physics, &mut ticker, jump(jumping), elytra(true));
    }
    assert_eq!(physics.mode(), sim::MovementMode::Gliding);
    let update = flags(|flags| {
        flags.using_item = Some(true);
        flags.gliding = Some(true);
    });
    assert_eq!(physics.apply_server_movement_flags(101, update), None);
}

/// An item-only stamped write replays like other flags, so once applied a
/// later write equal to the frame's capture changes nothing.
#[test]
fn an_item_only_stamped_write_settles_through_replay() {
    let (mut physics, mut ticker) = airborne();
    for _ in 0..3 {
        tick(&mut physics, &mut ticker, PhysicsSampleContext::default());
    }
    let using = |value| flags(|flags| flags.using_item = Some(value));
    assert_eq!(physics.apply_server_movement_flags(101, using(true)), None);
    reconcile_timeline_rewind(&mut ticker, &mut physics, 101, &VersionedFloor(1)).unwrap();
    assert_eq!(physics.take_server_using_item(), Some(true));
    physics.apply_server_movement_flags(101, using(false));
    assert_eq!(physics.take_server_using_item(), None);
}

/// A current-stamped item-use write reaches the item-use owner once; the
/// tick that captures it does not hand it over again.
#[test]
fn a_queued_item_use_write_reaches_the_owner_once() {
    let (mut physics, mut ticker) = airborne();
    tick(&mut physics, &mut ticker, PhysicsSampleContext::default());
    let using = flags(|flags| flags.using_item = Some(true));
    physics.apply_server_movement_flags(101, using);
    assert_eq!(physics.take_server_using_item(), Some(true));
    tick(&mut physics, &mut ticker, PhysicsSampleContext::default());
    assert_eq!(physics.take_server_using_item(), None);
}

/// An unstamped item-use write always reaches the item-use owner, which
/// compares it with the use it actually holds.
#[test]
fn an_unstamped_item_use_write_always_reaches_the_owner() {
    let (mut physics, mut ticker) = airborne();
    tick(&mut physics, &mut ticker, PhysicsSampleContext::default());
    let using = flags(|flags| flags.using_item = Some(false));
    physics.apply_server_movement_flags(0, using);
    assert_eq!(physics.take_server_using_item(), Some(false));
}

/// Replay refreshes later frames' item-use captures with a corrected item-use
/// bit, so a later stamped clear compares with the use the correction began.
#[test]
fn replay_carries_a_corrected_item_use_into_later_captures() {
    let (mut physics, mut ticker) = airborne();
    for _ in 0..4 {
        tick(&mut physics, &mut ticker, PhysicsSampleContext::default());
    }
    let using = |value| flags(|flags| flags.using_item = Some(value));
    assert_eq!(physics.apply_server_movement_flags(101, using(true)), None);
    reconcile_timeline_rewind(&mut ticker, &mut physics, 101, &VersionedFloor(1)).unwrap();
    assert_eq!(physics.take_server_using_item(), Some(true));
    assert_eq!(physics.apply_server_movement_flags(102, using(false)), None);
    assert_eq!(physics.take_server_using_item(), Some(false));
}

/// A current-stamped item-use write keeps its correction for the next frame
/// even when an unstamped write follows, so replay through that frame
/// restores it.
#[test]
fn a_current_stamped_item_use_write_keeps_its_correction() {
    let (mut physics, mut ticker) = airborne();
    tick(&mut physics, &mut ticker, PhysicsSampleContext::default());
    let using = |value| flags(|flags| flags.using_item = Some(value));
    physics.apply_server_movement_flags(101, using(true));
    physics.apply_server_movement_flags(0, using(false));
    for _ in 0..2 {
        tick(&mut physics, &mut ticker, PhysicsSampleContext::default());
    }
    physics.take_server_using_item();
    reconcile_timeline_rewind(&mut ticker, &mut physics, 101, &VersionedFloor(1)).unwrap();
    assert_eq!(physics.take_server_using_item(), Some(true));
}

/// Replay hands the item-use bit its corrections leave to the item-use owner,
/// whatever packet arrived last.
#[test]
fn replay_hands_the_final_item_use_to_the_owner() {
    let (mut physics, mut ticker) = airborne();
    for using in [false, false, false, true] {
        tick_using(&mut physics, &mut ticker, using);
    }
    let using = |value| flags(|flags| flags.using_item = Some(value));
    physics.apply_server_movement_flags(103, using(false));
    physics.apply_server_movement_flags(101, using(true));
    assert_eq!(physics.take_server_using_item(), Some(true));
    reconcile_timeline_rewind(&mut ticker, &mut physics, 101, &VersionedFloor(1)).unwrap();
    assert_eq!(physics.take_server_using_item(), Some(false));
}

/// A current-stamped item-use write compares with a local release made
/// after the last capture, so its correction is retained and a replay
/// through the next frame restores it.
#[test]
fn a_stamped_item_use_write_compares_with_a_local_release() {
    let (mut physics, mut ticker) = airborne();
    tick_using(&mut physics, &mut ticker, true);
    physics.write_local_using_item(false);
    let write = |value| flags(|flags| flags.using_item = Some(value));
    physics.apply_server_movement_flags(101, write(true));
    physics.apply_server_movement_flags(0, write(false));
    for _ in 0..2 {
        tick(&mut physics, &mut ticker, PhysicsSampleContext::default());
    }
    physics.take_server_using_item();
    physics.queue_server_motion([0.0; 3], 101);
    reconcile_timeline_rewind(&mut ticker, &mut physics, 101, &VersionedFloor(1)).unwrap();
    assert_eq!(physics.take_server_using_item(), Some(true));
}

/// A local item-use start or end supersedes a server item-use write still
/// waiting for the item-use owner, as vanilla's start and stop write the
/// actual flag the next tick reads.
#[test]
fn a_local_item_use_write_supersedes_a_pending_server_write() {
    let (mut physics, mut ticker) = airborne();
    tick(&mut physics, &mut ticker, PhysicsSampleContext::default());
    let using = flags(|flags| flags.using_item = Some(false));
    physics.apply_server_movement_flags(0, using);
    physics.write_local_using_item(true);
    assert_eq!(physics.take_server_using_item(), None);
}

/// Replay copies its item-use bit back even when no correction wrote it, so
/// a live set queued for a later frame gives way to the replayed clear.
#[test]
fn replay_copies_back_an_item_use_bit_no_correction_wrote() {
    let (mut physics, mut ticker) = airborne();
    for _ in 0..4 {
        tick(&mut physics, &mut ticker, PhysicsSampleContext::default());
    }
    let using = flags(|flags| flags.using_item = Some(true));
    physics.apply_server_movement_flags(104, using);
    assert_eq!(physics.take_server_using_item(), Some(true));
    physics.queue_server_motion([0.0; 3], 101);
    reconcile_timeline_rewind(&mut ticker, &mut physics, 101, &VersionedFloor(1)).unwrap();
    assert_eq!(physics.take_server_using_item(), Some(false));
}

/// Replay carries the replayed item-use bit into every later capture, as
/// vanilla refreshes each capture from the replayed flag component, so a
/// capture an unretained write changed gives way to it and a later stamped
/// clear compares with the carried bit.
#[test]
fn replay_carries_the_replayed_item_use_through_later_captures() {
    let (mut physics, mut ticker) = airborne();
    for using in [false, false, true, false] {
        tick_using(&mut physics, &mut ticker, using);
    }
    let using = |value| flags(|flags| flags.using_item = Some(value));
    physics.apply_server_movement_flags(101, using(true));
    physics.take_server_using_item();
    physics.queue_server_motion([0.0; 3], 101);
    reconcile_timeline_rewind(&mut ticker, &mut physics, 101, &VersionedFloor(1)).unwrap();
    physics.take_server_using_item();
    physics.apply_server_movement_flags(103, using(false));
    assert_eq!(physics.take_server_using_item(), Some(false));
}

/// Vanilla's item-use system does not run on replayed ticks, so replay carries
/// the replayed bit over a local start and copies the clear back; the owner
/// then stops the use, as vanilla's next tick does.
#[test]
fn replay_carries_its_bit_over_a_local_item_use_start() {
    let (mut physics, mut ticker) = airborne();
    for using in [false, false, true, true] {
        tick_using(&mut physics, &mut ticker, using);
    }
    physics.queue_server_motion([0.0; 3], 101);
    reconcile_timeline_rewind(&mut ticker, &mut physics, 101, &VersionedFloor(1)).unwrap();
    assert_eq!(physics.take_server_using_item(), Some(false));
}

/// A server start the item-use owner cannot reconcile yet still leaves the
/// flag set in the next capture, so a later stamped clear compares with it.
#[test]
fn a_capture_keeps_a_server_start_the_owner_has_not_reconciled() {
    let (mut physics, mut ticker) = airborne();
    tick(&mut physics, &mut ticker, PhysicsSampleContext::default());
    let write = |value| flags(|flags| flags.using_item = Some(value));
    physics.apply_server_movement_flags(0, write(true));
    physics.take_server_using_item();
    tick(&mut physics, &mut ticker, PhysicsSampleContext::default());
    physics.apply_server_movement_flags(101, write(false));
    assert_eq!(physics.take_server_using_item(), Some(false));
}

/// The live item-use flag is what the correction sync reports: a server start
/// the item-use owner has not reconciled yet still counts.
#[test]
fn the_live_item_use_flag_follows_server_and_local_writes() {
    let (mut physics, mut ticker) = airborne();
    tick(&mut physics, &mut ticker, PhysicsSampleContext::default());
    assert!(!physics.live_using_item());
    let using = flags(|flags| flags.using_item = Some(true));
    physics.apply_server_movement_flags(0, using);
    assert!(physics.live_using_item());
    physics.write_local_using_item(false);
    assert!(!physics.live_using_item());
}

/// Respawn and dimension changes clear the live item-use flag with the old
/// life's use, so a later server start compares with a clear flag.
#[test]
fn discarding_server_writes_clears_the_live_item_use_flag() {
    let (mut physics, mut ticker) = airborne();
    tick(&mut physics, &mut ticker, PhysicsSampleContext::default());
    let using = flags(|flags| flags.using_item = Some(true));
    physics.apply_server_movement_flags(0, using);
    physics.take_server_using_item();
    physics.discard_server_flag_writes();
    tick(&mut physics, &mut ticker, PhysicsSampleContext::default());
    physics.apply_server_movement_flags(101, using);
    assert_eq!(physics.take_server_using_item(), Some(true));
}
