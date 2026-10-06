use super::*;
use protocol::Packet;

fn transfer(id: Option<u32>) -> DimensionTransfer {
    let mut transfer = DimensionTransfer::default();
    transfer.begin(
        7,
        11,
        ChangeDimensionEvent {
            dimension: 1,
            position: [8.0, 64.0, 8.0],
            loading_screen_id: id,
            ..Default::default()
        },
        42,
        Duration::ZERO,
    );
    transfer
}

fn collect(transfer: &mut DimensionTransfer) -> Vec<Packet> {
    collect_at(transfer, Duration::ZERO, true)
}

fn collect_at(
    transfer: &mut DimensionTransfer,
    now: Duration,
    destination_ready: bool,
) -> Vec<Packet> {
    let mut packets = Vec::new();
    transfer
        .send_before_presentation(now, destination_ready, |packet| {
            packets.push(packet);
            Ok(())
        })
        .unwrap();
    packets
}

#[test]
fn dimension_done_waits_for_start_server_and_terrain_then_keeps_loading_until_presentation_end() {
    let mut transfer = transfer(Some(0));
    assert!(transfer.take_presentation_reset());
    assert!(!transfer.take_presentation_reset());
    assert!(
        !transfer
            .complete_with(|_| panic!("End before switch"))
            .unwrap()
    );
    assert_eq!(
        collect(&mut transfer),
        [protocol::loading_screen_packet(
            LoadingScreenPhase::Start,
            Some(0)
        )]
    );
    assert!(!transfer.active.as_ref().unwrap().server_acknowledged);
    assert!(collect(&mut transfer).is_empty());
    transfer.acknowledge(11);
    assert!(collect_at(&mut transfer, Duration::ZERO, false).is_empty());
    assert!(transfer.waiting_for_switch());
    assert_eq!(
        collect(&mut transfer),
        [protocol::dimension_change_done_packet(42)]
    );
    assert!(transfer.active(), "loading remains up after dimension-done");
    assert!(!transfer.waiting_for_switch());
    assert!(
        collect(&mut transfer).is_empty(),
        "dimension-done is sent once"
    );
    let mut end = None;
    assert!(
        transfer
            .complete_with(|packet| {
                end = Some(packet);
                Ok(())
            })
            .unwrap()
    );
    assert_eq!(
        end,
        Some(protocol::loading_screen_packet(
            LoadingScreenPhase::End,
            Some(0)
        ))
    );
    assert!(!transfer.active());
}

#[test]
fn server_acknowledgement_is_epoch_bounded_and_does_not_repeat_control_packets() {
    let mut transfer = transfer(None);
    assert_eq!(
        collect(&mut transfer),
        [protocol::loading_screen_packet(
            LoadingScreenPhase::Start,
            None
        )]
    );
    transfer.acknowledge(10);
    assert!(!transfer.active.as_ref().unwrap().server_acknowledged);
    assert!(collect(&mut transfer).is_empty());
    transfer.acknowledge(11);
    transfer.acknowledge(11);
    assert!(transfer.active.as_ref().unwrap().server_acknowledged);
    assert_eq!(
        collect(&mut transfer),
        [protocol::dimension_change_done_packet(42)]
    );
    assert!(collect(&mut transfer).is_empty());
    assert!(
        transfer
            .complete_with(|packet| {
                assert_eq!(
                    packet,
                    protocol::loading_screen_packet(LoadingScreenPhase::End, None)
                );
                Ok(())
            })
            .unwrap()
    );
    transfer.acknowledge(11);
    assert!(!transfer.active(), "late server ack cannot restart loading");
    assert!(collect(&mut transfer).is_empty());
}

#[test]
fn outbound_backpressure_before_start_keeps_both_steps_pending() {
    let mut transfer = transfer(Some(99));
    assert!(matches!(
        transfer.send_before_presentation(Duration::ZERO, true, |packet| Err(
            PacketSendError::Full(packet)
        )),
        Err(PacketSendError::Full(_))
    ));
    assert!(transfer.waiting_for_switch());
    assert!(
        !transfer
            .complete_with(|_| panic!("End before accepted Start and Switch"))
            .unwrap()
    );
    assert_eq!(
        collect(&mut transfer),
        [protocol::loading_screen_packet(
            LoadingScreenPhase::Start,
            Some(99)
        )]
    );
    transfer.acknowledge(11);
    assert_eq!(
        collect(&mut transfer),
        [protocol::dimension_change_done_packet(42)]
    );
}

#[test]
fn outbound_backpressure_retries_each_unsent_step_without_duplicating_accepted_packets() {
    let mut transfer = transfer(Some(99));
    transfer.acknowledge(11);
    let mut packets = collect(&mut transfer);
    assert!(
        transfer.waiting_for_switch(),
        "Start cannot queue Switch in the same update"
    );
    let result = transfer.send_before_presentation(Duration::ZERO, true, |packet| {
        Err(PacketSendError::Full(packet))
    });
    assert!(matches!(result, Err(PacketSendError::Full(_))));
    assert!(transfer.waiting_for_switch());
    packets.extend(collect(&mut transfer));
    assert_eq!(
        packets,
        [
            protocol::loading_screen_packet(LoadingScreenPhase::Start, Some(99)),
            protocol::dimension_change_done_packet(42),
        ]
    );
    assert!(matches!(
        transfer.complete_with(|packet| Err(PacketSendError::Full(packet))),
        Err(PacketSendError::Full(_))
    ));
    assert!(
        transfer.active(),
        "queue saturation cannot clear loading-screen identity"
    );
    assert!(
        transfer
            .complete_with(|packet| {
                packets.push(packet);
                Ok(())
            })
            .unwrap()
    );
    assert_eq!(
        packets.last(),
        Some(&protocol::loading_screen_packet(
            LoadingScreenPhase::End,
            Some(99)
        ))
    );
    assert!(!transfer.active());
}

#[test]
fn session_replacement_and_superseding_transfer_drop_obsolete_loading_identity() {
    let mut transfer = transfer(Some(77));
    collect(&mut transfer);
    transfer.synchronize_session(Some(8));
    assert!(!transfer.active());
    assert!(collect(&mut transfer).is_empty());
    assert!(
        transfer
            .complete_with(|_| panic!("End for replaced session"))
            .unwrap()
    );
    transfer.begin(
        8,
        20,
        ChangeDimensionEvent {
            loading_screen_id: Some(78),
            ..Default::default()
        },
        55,
        Duration::from_secs(19),
    );
    collect(&mut transfer);
    transfer.begin(
        8,
        21,
        ChangeDimensionEvent {
            loading_screen_id: Some(88),
            ..Default::default()
        },
        56,
        Duration::from_secs(20),
    );
    transfer.acknowledge(11);
    transfer.acknowledge(20);
    assert!(!transfer.active.as_ref().unwrap().server_acknowledged);
    assert_eq!(
        collect(&mut transfer),
        [protocol::loading_screen_packet(
            LoadingScreenPhase::Start,
            Some(88)
        )]
    );
    assert!(transfer.waiting_for_switch());
    assert!(collect(&mut transfer).is_empty());
    transfer.acknowledge(21);
    assert_eq!(
        collect(&mut transfer),
        [protocol::dimension_change_done_packet(56)]
    );
    assert!(collect(&mut transfer).is_empty());
    assert!(
        transfer
            .complete_with(|packet| {
                assert_eq!(
                    packet,
                    protocol::loading_screen_packet(LoadingScreenPhase::End, Some(88))
                );
                Ok(())
            })
            .unwrap()
    );
}

#[test]
fn dimension_start_progresses_without_a_window_but_done_waits_for_server_and_terrain() {
    use crate::environment::WorldClock;
    use crate::runtime::world::ClientWorld;
    use bevy::prelude::{App, Update};
    let clock = WorldClock::default();
    let session = clock.session_generation();
    let mut world = ClientWorld {
        stream: Some(chunk_pipeline::WorldStream::new(protocol::WorldBootstrap {
            dimension: 1,
            local_player_runtime_id: 42,
            local_player_unique_id: 42,
            player_position: [8.0, 64.0, 8.0],
            world_spawn_position: [8, 64, 8],
            air_network_id: protocol::SEQUENTIAL_AIR_NETWORK_ID,
            block_network_ids_are_hashes: false,
        })),
        ..ClientWorld::default()
    };
    world.dimension_transfer.begin(
        session,
        11,
        ChangeDimensionEvent {
            dimension: 1,
            loading_screen_id: Some(91),
            ..Default::default()
        },
        42,
        Duration::ZERO,
    );
    let position = world
        .stream
        .as_ref()
        .unwrap()
        .resolved_server_position()
        .position;
    assert!(
        !world
            .stream
            .as_ref()
            .unwrap()
            .dimension_transfer_ready(position)
    );
    assert!(
        !world
            .stream
            .as_ref()
            .unwrap()
            .dimension_transfer_presentable(position)
    );
    let (network, _queue_guard) = NetworkHandle::with_command_capacity(2);
    let mut app = App::new();
    app.insert_resource(world)
        .insert_resource(network)
        .insert_resource(clock)
        .init_resource::<bevy::prelude::Time<bevy::time::Real>>()
        .add_systems(Update, advance_dimension_transfer);
    app.update();
    let transfer = &app.world().resource::<ClientWorld>().dimension_transfer;
    assert!(transfer.active.as_ref().unwrap().start_queued);
    assert!(transfer.waiting_for_switch());
    assert!(!transfer.active.as_ref().unwrap().server_acknowledged);
    assert!(transfer.active(), "presentation still holds local movement");
    assert!(transfer.active.as_ref().unwrap().presentation_pending);
    assert_eq!(
        app.world()
            .resource::<NetworkHandle>()
            .pending_command_count(),
        1
    );
    app.world_mut()
        .resource_mut::<ClientWorld>()
        .dimension_transfer
        .acknowledge(11);
    app.update();
    assert_eq!(
        app.world()
            .resource::<NetworkHandle>()
            .pending_command_count(),
        1
    );
}

#[test]
fn server_wait_timeout_defers_one_update_and_still_requires_destination_terrain() {
    let mut transfer = transfer(Some(5));
    assert_eq!(collect(&mut transfer).len(), 1);
    assert!(collect_at(&mut transfer, SERVER_ACK_TIMEOUT, true).is_empty());
    assert!(!transfer.active.as_ref().unwrap().server_acknowledged);
    let expired = SERVER_ACK_TIMEOUT + Duration::from_nanos(1);
    assert!(collect_at(&mut transfer, expired, true).is_empty());
    assert!(transfer.active.as_ref().unwrap().server_acknowledged);
    assert!(collect_at(&mut transfer, expired, false).is_empty());
    assert_eq!(
        collect_at(&mut transfer, expired, true),
        [protocol::dimension_change_done_packet(42)]
    );
    assert!(
        transfer.active(),
        "presentation is a separate readiness gate"
    );
}
