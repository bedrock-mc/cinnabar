//! An assisted release with no unsent input retains its physical charge boundary.

use super::*;

#[test]
fn assisted_riptide_empty_outbox_keeps_edge_charge_during_catchup() {
    for charge in [
        gameplay::item_use::RIPTIDE_CHARGE_TICKS - 1,
        gameplay::item_use::RIPTIDE_CHARGE_TICKS,
    ] {
        for catchup in [1, 3] {
            let (network, mut captured) =
                NetworkHandle::<PackApplication>::stub_capturing_packets();
            let origin = [0.0, 65.620_01, 0.0];
            let mut physics = LocalPhysicsController::default();
            physics.reanchor_network_position(origin, 100, false);
            let mut ticker =
                MovementTicker::with_epoch_publisher(network.physics_epoch_publisher());
            ticker.reset(7, 100, origin);
            ticker.set_source(gameplay::movement::MovementSource::Physics);
            let mut runtime = ItemUseRuntime::default();
            let mut swings = SwingTracker::default();
            let mut view = client_presentation::local_player::LocalViewPose::default();
            // The accepted use starts between inputs, at tick102 after completed tick101.
            let sample = physics
                .advance(
                    simulation_clock::TICK_DURATION,
                    sim::MovementInput::default(),
                    &RiptideWorld,
                )
                .samples
                .remove(0);
            ticker.enqueue_completed_physics(sample).unwrap();
            flush(&mut ticker, &network);
            runtime.observe_press(true);
            super::super::super::admit_with_action_aim(
                &mut runtime,
                &mut swings,
                &mut ticker,
                &mut view,
                &trident(102, true),
                42,
                6,
                None,
                &network,
            );
            assert!(runtime.is_using());
            for _ in 0..charge {
                let sample = physics
                    .advance(
                        simulation_clock::TICK_DURATION,
                        sim::MovementInput::default(),
                        &RiptideWorld,
                    )
                    .samples
                    .remove(0);
                ticker.enqueue_completed_physics(sample).unwrap();
                flush(&mut ticker, &network);
            }
            captured.drain();
            let edge = ticker.between_ticks_sample().unwrap();
            assert_eq!(edge.tick - 102, charge);
            let rotation = bevy::prelude::Quat::from_rotation_y(90_f32.to_radians());
            super::super::super::admit_with_action_aim(
                &mut runtime,
                &mut swings,
                &mut ticker,
                &mut view,
                &trident(edge.tick, false),
                42,
                6,
                Some(rotation),
                &network,
            );
            assert!(runtime.is_using());
            assert!(!ticker.has_held_release());
            assert_eq!(runtime.take_riptide_launch(), None);
            assert!(captured.drain().is_empty());
            let frame = physics.advance(
                simulation_clock::TICK_DURATION * catchup,
                sim::MovementInput::default(),
                &RiptideWorld,
            );
            assert_eq!(frame.samples.len(), catchup as usize);
            for sample in frame.samples {
                ticker.enqueue_completed_physics(sample).unwrap();
            }
            view.set_rotation(bevy::prelude::Quat::IDENTITY);
            let later_aim = bevy::prelude::Quat::from_rotation_y(-90_f32.to_radians());
            let facing = ticker.newest_unsent_sample().unwrap().tick;
            super::super::super::admit_with_action_aim(
                &mut runtime,
                &mut swings,
                &mut ticker,
                &mut view,
                &trident(facing, false),
                42,
                6,
                Some(later_aim),
                &network,
            );
            assert!(!runtime.is_using());
            assert_eq!(view.rotation(), bevy::prelude::Quat::IDENTITY);
            let level = runtime.take_riptide_launch();
            assert_eq!(
                level.map(|launch| launch.level),
                (charge == gameplay::item_use::RIPTIDE_CHARGE_TICKS).then_some(3)
            );
            assert!(ticker.has_held_release());
            assert!(captured.drain().is_empty());
            if let Some(launch) = level {
                ticker.request_riptide(&mut physics, launch.level, launch.rotation);
            }
            assert_eq!(flush(&mut ticker, &network), catchup as usize);
            let packets = captured.drain();
            assert_eq!(packets.len(), catchup as usize);
            let input = protocol::player_auth_input_trace_sample(packets.last().unwrap()).unwrap();
            assert_eq!(input.tick, facing);
            assert!((input.yaw - 90.0).abs() < 1e-3);
            ticker.send_held_release(|packets| network.send_inventory_packets(packets));
            let packets = captured.drain();
            assert_eq!(packets.len(), 1);
            assert!(protocol::is_aim_assist_rotation_action(&packets[0]));
            let next = physics.advance(
                simulation_clock::TICK_DURATION,
                sim::MovementInput::default(),
                &RiptideWorld,
            );
            assert_eq!(next.samples[0].processed.spin_started, level.is_some());
            assert_eq!(next.samples[0].movement[0] < -1.0, level.is_some());
            assert_eq!(runtime.take_riptide_launch(), None);
        }
    }
}

#[test]
fn an_ordinary_riptide_edge_does_not_switch_to_a_later_assisted_target() {
    let (network, mut captured) = NetworkHandle::<PackApplication>::stub_capturing_packets();
    let mut ticker = MovementTicker::with_epoch_publisher(network.physics_epoch_publisher());
    ticker.reset(7, 100, completed(101).position);
    ticker.set_source(gameplay::movement::MovementSource::Physics);
    let mut runtime = ItemUseRuntime::default();
    let mut swings = SwingTracker::default();
    let mut view = client_presentation::local_player::LocalViewPose::default();
    runtime.observe_press(true);
    admit_on_tick(
        &mut runtime,
        &mut swings,
        &mut ticker,
        &trident(101, true),
        42,
        6,
        |_| Ok(()),
    );
    for tick in 101..=101 + gameplay::item_use::RIPTIDE_CHARGE_TICKS {
        ticker.enqueue_completed_physics(completed(tick)).unwrap();
        flush(&mut ticker, &network);
    }
    captured.drain();
    let edge = trident(ticker.next_tick(), false);
    let first = gameplay::item_use::RiptideFacing {
        rotation: [0.0, 90.0],
        assisted: false,
    };
    assert!(runtime.observe_riptide_release(
        edge.tick,
        edge.held,
        edge.riptide,
        edge.selection.as_ref(),
        first
    ));
    super::super::super::admit_with_action_aim(
        &mut runtime,
        &mut swings,
        &mut ticker,
        &mut view,
        &edge,
        42,
        6,
        Some(bevy::prelude::Quat::from_rotation_y(-90_f32.to_radians())),
        &network,
    );
    assert!(
        ticker.has_held_release(),
        "an ordinary edge retains its future-facing input convention"
    );
    let launch = runtime.take_riptide_launch().unwrap();
    assert_eq!(launch.rotation, first.rotation);
    assert_eq!(view.rotation(), bevy::prelude::Quat::IDENTITY);
    assert!(captured.drain().is_empty());
    ticker
        .enqueue_completed_physics(completed(edge.tick))
        .unwrap();
    flush(&mut ticker, &network);
    let packets = captured.drain();
    assert_eq!(packets.len(), 1);
    assert_eq!(
        protocol::player_auth_input_trace_sample(&packets[0])
            .unwrap()
            .yaw,
        90.0
    );
    assert_eq!(runtime.take_riptide_launch(), None);
}
