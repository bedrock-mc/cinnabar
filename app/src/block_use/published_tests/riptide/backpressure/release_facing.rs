//! Turning during movement backpressure cannot replace an observed release's action facing.

use super::*;

#[test]
fn ordinary_riptide_first_facing_survives_fifo_wait_without_replacing_later_look() {
    for charge in [
        gameplay::item_use::RIPTIDE_CHARGE_TICKS - 1,
        gameplay::item_use::RIPTIDE_CHARGE_TICKS,
    ] {
        for wet in [true, false] {
            for blocked in [false, true] {
                let (mut world, mut captured) = release_edge(charge, wet, blocked);
                world
                    .resource_mut::<LocalViewPose>()
                    .set_rotation(Quat::IDENTITY);
                if blocked {
                    for _ in 0..3 {
                        assert!(!advance(&mut world, true).processed.spin_started);
                        refuse_fifo(&mut world);
                        produce(&mut world);
                        assert!(captured.drain().is_empty());
                    }
                }
                let expected = wet && charge == gameplay::item_use::RIPTIDE_CHARGE_TICKS;
                assert_eq!(finish(&mut world, &mut captured, true), expected);
                assert_eq!(world.resource::<LocalViewPose>().rotation(), Quat::IDENTITY);
                let launch = world
                    .resource::<LocalPhysicsController>()
                    .sample_at(world.resource::<MovementTicker>().completed_tick())
                    .unwrap();
                assert_eq!(launch.movement[0] < -1.0, expected);
                // Only the action fence overrides facing; subsequent physical input keeps the new look.
                let sample = world
                    .resource_mut::<LocalPhysicsController>()
                    .advance_with_context(
                        simulation_clock::TICK_DURATION,
                        sim::MovementInput {
                            yaw_degrees: 180.0,
                            ..Default::default()
                        },
                        gameplay::movement::PhysicsSampleContext {
                            pitch: 20.0,
                            ..Default::default()
                        },
                        &Water,
                    )
                    .samples
                    .remove(0);
                world
                    .resource_mut::<MovementTicker>()
                    .enqueue_completed_physics(sample.clone())
                    .unwrap();
                flush_and_acknowledge(&mut world);
                let packets = captured.drain();
                let input = packets
                    .iter()
                    .filter_map(protocol::player_auth_input_trace_sample)
                    .find(|input| input.tick == sample.tick)
                    .unwrap();
                assert_eq!((input.pitch, input.yaw), (20.0, 180.0));
                assert!(
                    !sample.processed.spin_started,
                    "the admitted launch is consumed only once"
                );
            }
        }
    }
}
