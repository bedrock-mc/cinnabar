use super::*;

#[test]
fn flush_refuses_a_stale_queue_without_physics_authority() {
    let mut ticker = MovementTicker::default();
    ticker.reset(1, 10, [0.0; 3]);
    ticker.set_source(MovementSource::Physics);
    ticker
        .enqueue_completed_physics(PhysicsMovementSample {
            tick: 11,
            position: [1.0, 2.0, 3.0],
            velocity: [0.1, 0.2, 0.3],
            move_vector: [0.0; 2],
            raw_move_vector: [0.0; 2],
            analogue_move_vector: [0.0; 2],
            pitch: 0.0,
            yaw: 0.0,
            head_yaw: 0.0,
            camera_orientation: [0.0, 0.0, 1.0],
            jumping: false,
            sneaking: false,
            sprinting: false,
            input_mode: PlayerInputMode::Mouse,
            grounded_before_tick: false,
            grounded_after_tick: false,
            horizontal_collision: false,
            vertical_collision: false,
            jump_repeated: false,
            world_identity: WorldCollisionIdentity::new(
                sim::CollisionRegistryIdentity {
                    protocol: 1001,
                    id_space: sim::CollisionIdSpace::Sequential,
                    preg_sha256: [1; 32],
                },
                [],
            )
            .unwrap(),
        })
        .unwrap();
    assert_eq!(ticker.outbox.len(), 1);

    // Simulate stale state surviving a future refactor so the flush guard
    // is verified independently from set_source's transition cleanup.
    ticker.source = MovementSource::FreeCamera;
    let mut sent_packets = 0;
    let flushed = flush_player_auth_inputs(&mut ticker, 8, None, |_identity, _packet| {
        sent_packets += 1;
        Ok::<_, ()>(())
    })
    .unwrap();

    assert_eq!(flushed, 0);
    assert_eq!(sent_packets, 0);
    assert_eq!(ticker.sent_free_camera_packet_count(), 0);
    assert_eq!(ticker.outbox.len(), 1);
}
