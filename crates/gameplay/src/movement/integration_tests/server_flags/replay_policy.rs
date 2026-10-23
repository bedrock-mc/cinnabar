use super::*;

/// Checks retained comparisons against the selected capture rather than the live word.
fn retained_change(write: impl Fn(bool) -> client_world::MovementFlagUpdate) {
    let (mut physics, mut ticker) = airborne();
    for _ in 0..3 {
        tick(&mut physics, &mut ticker, elytra(true));
    }
    assert_eq!(physics.apply_server_movement_flags(101, write(false)), None);
    assert_eq!(physics.apply_server_movement_flags(0, write(true)), None);
    let live = physics.prediction_sync_flags();
    // The incoming clear matches the selected capture even though live is set.
    assert_eq!(physics.apply_server_movement_flags(101, write(false)), None);
    assert_eq!(physics.prediction_sync_flags(), live);
    // The set matches live, but differs from the selected capture and must replay.
    assert_eq!(
        physics.apply_server_movement_flags(101, write(true)),
        Some(101)
    );
}

/// A retained glide difference requests replay even when it already matches live.
#[test]
fn server_flag_replay_policy_retained_glide() {
    retained_change(|set| flags(|flags| flags.gliding = Some(set)));
}

/// A retained swim difference requests replay even when it already matches live.
#[test]
fn server_flag_replay_policy_retained_swim() {
    retained_change(|set| flags(|flags| flags.swimming = Some(set)));
}

/// A retained crawl difference requests replay even when it already matches live.
#[test]
fn server_flag_replay_policy_retained_crawl() {
    retained_change(|set| flags(|flags| flags.crawling = Some(set)));
}

/// A retained spin difference requests replay even when it already matches live.
#[test]
fn server_flag_replay_policy_retained_spin() {
    retained_change(|set| flags(|flags| flags.spinning = Some(set)));
}

/// Frame availability distinguishes live placement from a clamped retained replay.
#[test]
fn server_flag_replay_policy_respects_available_frames() {
    let spin = flags(|flags| flags.spinning = Some(true));
    for offset in [None, Some(0), Some(1)] {
        let (mut physics, mut ticker) = airborne();
        for _ in 0..3 {
            tick(&mut physics, &mut ticker, elytra(true));
        }
        let stamp = offset.map_or(0, |offset| physics.state().unwrap().tick + offset);
        assert_eq!(physics.apply_server_movement_flags(stamp, spin), None);
    }
    let (mut physics, mut ticker) = airborne();
    tick(&mut physics, &mut ticker, elytra(true));
    assert_eq!(physics.apply_server_movement_flags(50, spin), None);
    assert!(physics.state().unwrap().spin_attack_ticks.is_some());

    let (mut physics, mut ticker) = airborne();
    physics.set_rewind_history_size(2);
    physics.reanchor_network_position(physics.network_position().unwrap(), 100, false);
    for _ in 0..4 {
        tick(&mut physics, &mut ticker, elytra(true));
    }
    let oldest = physics.oldest_retained_tick().unwrap();
    assert_eq!(physics.history_len(), physics.history_capacity());
    assert!(oldest < physics.state().unwrap().tick);
    assert_eq!(physics.apply_server_movement_flags(50, spin), Some(oldest));
}

/// Committed flags replay pending samples without fabricating a server start action.
#[test]
fn server_flag_replay_policy_committed_control_rebuilds_samples() {
    let (mut physics, mut ticker) = airborne();
    for _ in 0..3 {
        tick(&mut physics, &mut ticker, elytra(true));
    }
    assert_eq!(
        physics.sample_at(102).unwrap().processed.mode,
        sim::MovementMode::Walking
    );
    let mut effects = crate::movement::LocalMovementEffectTimeline::default();
    let mut speed = crate::movement::LocalMovementSpeedAuthority::default();
    crate::committed_control::CommittedGameplayState {
        movement: &mut ticker,
        physics: &mut physics,
        effects: &mut effects,
        speed: &mut speed,
        session_generation: 7,
        dimension: 0,
        dimension_transfer_active: false,
    }
    .apply(
        client_world::CommittedControlEvent::LocalMovementFlags {
            sequence: 1,
            tick: 101,
            flags: flags(|flags| flags.gliding = Some(true)),
        },
        &VersionedFloor(1),
        |_| {},
    );
    assert_eq!(
        physics.sample_at(102).unwrap().processed.mode,
        sim::MovementMode::Gliding
    );
    assert_eq!(count(&ticker, PlayerInputFlags::START_GLIDING), 0);
    assert_eq!(count(&ticker, PlayerInputFlags::STOP_GLIDING), 0);
}
