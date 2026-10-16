//! Assisted trident release shares the charge boundary and ordered movement fence.

use super::*;
use client_interaction::movement::{LocalPhysicsController, MovementTicker};

#[path = "assisted/release_edge.rs"]
mod release_edge;

/// Uses the same verified stack while varying only the logical tick and physical hold.
fn trident(tick: u64, held: bool) -> UseFrame {
    UseFrame {
        held,
        riptide: Some(gameplay::item_use::RiptideUse {
            level: 3,
            wet: true,
            mounted: false,
            usable: true,
        }),
        air_use: classify("minecraft:trident", false, 0, None),
        ..use_frame(tick)
    }
}

/// Hands inputs to the real session queue and acknowledges their socket receipts in order.
pub(super) fn flush(
    ticker: &mut MovementTicker,
    network: &NetworkHandle<PackApplication>,
) -> usize {
    let mut written = Vec::new();
    gameplay::movement::flush_player_auth_inputs_guarded(
        ticker,
        gameplay::movement::OUTBOX_CAPACITY,
        Some(evidence()),
        |identity, packet, guard, _| {
            written.push(identity);
            network.send_physics_packet(identity, packet, guard)
        },
    )
    .unwrap();
    for identity in &written {
        assert!(ticker.acknowledge_physics_send(*identity));
    }
    written.len()
}

#[test]
fn assisted_riptide_release_keeps_its_charge_and_first_launch_input() {
    for charge in [
        gameplay::item_use::RIPTIDE_CHARGE_TICKS - 1,
        gameplay::item_use::RIPTIDE_CHARGE_TICKS,
    ] {
        let (network, mut captured) = NetworkHandle::<PackApplication>::stub_capturing_packets();
        let origin = [0.0, 65.620_01, 0.0];
        let mut physics = LocalPhysicsController::default();
        physics.reanchor_network_position(origin, 100, false);
        let mut ticker = MovementTicker::with_epoch_publisher(network.physics_epoch_publisher());
        ticker.reset(7, 100, origin);
        ticker.set_source(gameplay::movement::MovementSource::Physics);
        let mut runtime = ItemUseRuntime::default();
        let mut swings = SwingTracker::default();
        let mut view = client_presentation::local_player::LocalViewPose::default();
        let end = 100 + charge;
        for tick in 101..=end {
            let frame = physics.advance(
                simulation_clock::TICK_DURATION,
                sim::MovementInput::default(),
                &RiptideWorld,
            );
            ticker
                .enqueue_completed_physics(frame.samples[0].clone())
                .unwrap();
            if tick == 101 {
                runtime.observe_press(true);
                super::super::admit_with_action_aim(
                    &mut runtime,
                    &mut swings,
                    &mut ticker,
                    &mut view,
                    &trident(tick, true),
                    42,
                    6,
                    None,
                    &network,
                );
            }
            if tick < end {
                flush(&mut ticker, &network);
            }
        }
        captured.drain();
        let rotation = bevy::prelude::Quat::from_rotation_y(90_f32.to_radians());
        super::super::admit_with_action_aim(
            &mut runtime,
            &mut swings,
            &mut ticker,
            &mut view,
            &trident(end, false),
            42,
            6,
            Some(rotation),
            &network,
        );
        let level = runtime.take_riptide_launch();
        let charged = charge >= gameplay::item_use::RIPTIDE_CHARGE_TICKS;
        assert_eq!(level.map(|launch| launch.level), charged.then_some(3));
        assert_eq!(runtime.take_riptide_launch(), None);
        if let Some(launch) = level {
            ticker.request_riptide(&mut physics, launch.level, launch.rotation);
        }
        view.set_rotation(bevy::prelude::Quat::IDENTITY);
        assert!(captured.drain().is_empty());
        flush(&mut ticker, &network);
        let facing = captured.drain();
        assert_eq!(facing.len(), 1);
        assert!(
            (protocol::player_auth_input_trace_sample(&facing[0])
                .unwrap()
                .yaw
                - 90.0)
                .abs()
                < 1e-3
        );
        ticker.send_held_release(|_| Err(gameplay::BatchSendError::Full));
        let input = gameplay::movement::physics_movement_input(
            [0.0, 1.0],
            0.0,
            true,
            false,
            false,
            false,
            None,
        );
        let mut ordinary = physics.clone();
        let ordinary = ordinary
            .advance(simulation_clock::TICK_DURATION, input, &RiptideWorld)
            .samples
            .remove(0);
        let launch = physics
            .advance_with_context(
                simulation_clock::TICK_DURATION,
                input,
                gameplay::movement::PhysicsSampleContext {
                    release_use_modifier: ticker.release_use_modifier(),
                    ..Default::default()
                },
                &RiptideWorld,
            )
            .samples
            .remove(0);
        assert_eq!(
            launch.move_vector, ordinary.move_vector,
            "queue pressure after facing must not prolong slowdown"
        );
        assert_eq!(launch.processed.spin_started, charged);
        assert_eq!(launch.movement[0] < -1.0, charged);
        ticker.enqueue_completed_physics(launch).unwrap();
        assert_eq!(
            flush(&mut ticker, &network),
            0,
            "no launch input may overtake the release"
        );
        assert!(captured.drain().is_empty());
        ticker.send_held_release(|packets| network.send_inventory_packets(packets));
        assert_eq!(flush(&mut ticker, &network), 1);
        let packets = captured.drain();
        assert_eq!(packets.len(), 2);
        assert!(protocol::is_aim_assist_rotation_action(&packets[0]));
        assert_eq!(
            protocol::player_auth_input_trace_sample(&packets[1])
                .unwrap()
                .flag_names
                .contains(&"StartSpinAttack"),
            charged
        );
        ticker.send_held_release(|_| panic!("the release must be sent only once"));
        let next = physics.advance(
            simulation_clock::TICK_DURATION,
            sim::MovementInput::default(),
            &RiptideWorld,
        );
        assert!(
            !next.samples[0].processed.spin_started,
            "the release must launch only once"
        );
    }
}
