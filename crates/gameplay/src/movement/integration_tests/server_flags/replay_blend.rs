use super::*;

/// Starts grounded with one retained sample before any server pose writes.
fn grounded() -> (LocalPhysicsController, MovementTicker) {
    let position = [0.0, 1.0 + protocol::PLAYER_NETWORK_OFFSET, 0.0];
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position(position, 100, true);
    let mut ticker = MovementTicker::default();
    ticker.reset(7, 100, position);
    ticker.set_source(MovementSource::Physics);
    tick(&mut physics, &mut ticker, elytra(false));
    (physics, ticker)
}

/// Checks the complete final simulation state and each replayed jump decision.
fn assert_replay_matches_live(physics: &LocalPhysicsController, ticker: &MovementTicker) {
    let end = physics.state().unwrap().tick;
    for start in 101..=end {
        let mut replayed = physics.clone();
        let mut outbox = ticker.clone();
        reconcile_timeline_rewind(&mut outbox, &mut replayed, start, &VersionedFloor(1)).unwrap();
        assert_eq!(replayed.state(), physics.state(), "replay {start}..{end}");
        assert_eq!(replayed.pose_flags(), physics.pose_flags());
        for tick in start + 1..=end {
            let actual = replayed.sample_at(tick).unwrap();
            let expected = physics.sample_at(tick).unwrap();
            assert_eq!(
                actual.processed.jump_initiated, expected.processed.jump_initiated,
                "jump admission at {tick}, replay {start}..{end}",
            );
            assert_eq!(actual.velocity, expected.velocity, "velocity at {tick}");
            assert_eq!(actual.position, expected.position, "position at {tick}");
            assert_eq!(actual.processed.pose_flags, expected.processed.pose_flags);
        }
    }
}

#[test]
fn immobile_pose_exit_preserves_the_pending_swim_blend_during_replay() {
    for swimming in [false, true] {
        for unchanged in [false, true] {
            let (mut physics, mut ticker) = grounded();
            physics.apply_server_movement_flags(
                101,
                flags(|flags| {
                    if swimming {
                        flags.swimming = Some(true);
                    } else {
                        flags.crawling = Some(true);
                    }
                }),
            );
            tick_with(
                &mut physics,
                &mut ticker,
                MovementInput {
                    immobile: true,
                    ..Default::default()
                },
                elytra(false),
            );
            assert!(!physics.pose_flags().swim_pose());
            assert!(physics.state().unwrap().swim_pose_active);
            assert_eq!(physics.state().unwrap().swim_amount, 0.0);
            if unchanged {
                assert_eq!(
                    physics.apply_server_movement_flags(
                        102,
                        flags(|flags| {
                            flags.swimming = Some(false);
                            flags.crawling = Some(false);
                        }),
                    ),
                    None,
                );
            }
            let sample = tick_with(
                &mut physics,
                &mut ticker,
                MovementInput {
                    jumping: true,
                    ..Default::default()
                },
                elytra(false),
            );
            assert!(!sample.processed.jump_initiated);
            assert_eq!(physics.state().unwrap().swim_amount, 0.1);
            assert_replay_matches_live(&physics, &ticker);
        }
    }
}

#[test]
fn unrelated_flag_corrections_preserve_the_pending_swim_blend() {
    for spin in [false, true] {
        let (mut physics, mut ticker) = grounded();
        physics.apply_server_movement_flags(101, flags(|flags| flags.swimming = Some(true)));
        tick_with(
            &mut physics,
            &mut ticker,
            MovementInput {
                immobile: true,
                ..Default::default()
            },
            elytra(false),
        );
        physics.apply_server_movement_flags(
            102,
            flags(|flags| {
                if spin {
                    flags.spinning = Some(true);
                } else {
                    flags.gliding = Some(true);
                }
            }),
        );
        assert!(physics.state().unwrap().swim_pose_active);
        let sample = tick_with(
            &mut physics,
            &mut ticker,
            MovementInput {
                jumping: true,
                ..Default::default()
            },
            elytra(false),
        );
        assert!(!sample.processed.jump_initiated);
        assert_replay_matches_live(&physics, &ticker);
    }
}

/// Produces a reproducible stream without clock or external entropy dependencies.
fn next_random(seed: &mut u64) -> u64 {
    *seed ^= *seed << 13;
    *seed ^= *seed >> 7;
    *seed ^= *seed << 17;
    *seed
}

#[test]
fn seeded_pose_blend_sequences_replay_from_every_retained_start() {
    for seed in 1..=16 {
        let mut random = seed;
        let (mut physics, mut ticker) = grounded();
        for _ in 0..24 {
            let choices = next_random(&mut random);
            let poses = physics.pose_flags();
            let spinning = physics.state().unwrap().spin_attack_ticks.is_some();
            let write = flags(|flags| match choices % 8 {
                0 => {}
                1 => flags.swimming = Some(!poses.swimming),
                2 => flags.crawling = Some(!poses.crawling),
                3 => flags.gliding = Some(!poses.gliding),
                4 => flags.spinning = Some(!spinning),
                5 => {
                    flags.swimming = Some(poses.swimming);
                    flags.crawling = Some(poses.crawling);
                    flags.gliding = Some(poses.gliding);
                    flags.spinning = Some(spinning);
                }
                6 => flags.swimming = Some(choices & 8 != 0),
                _ => flags.crawling = Some(choices & 8 != 0),
            });
            physics.apply_server_movement_flags(physics.state().unwrap().tick, write);
            tick_with(
                &mut physics,
                &mut ticker,
                MovementInput {
                    immobile: choices & 16 != 0,
                    jumping: choices & 32 != 0,
                    forward: if choices & 64 != 0 { 1.0 } else { 0.0 },
                    ..Default::default()
                },
                elytra(choices & 128 != 0),
            );
            assert_replay_matches_live(&physics, &ticker);
        }
    }
}
