use bedrock_protocol_1_26_51::{packets, types, wire};
use bytes::Bytes;

/// Both absolute decoders must retain local authority independently of interpolation.
#[test]
fn absolute_actor_moves_preserve_every_authority_flag_combination() {
    for (flags, teleport, force_local, completion) in [
        (0, false, false, false),
        (1, false, false, false),
        (2, true, false, false),
        (3, true, false, false),
        (4, false, true, false),
        (5, false, true, false),
        (6, true, true, false),
        (7, true, true, false),
        (8, false, false, true),
        (9, false, false, true),
        (10, true, false, true),
        (11, true, false, true),
        (12, false, true, true),
        (13, false, true, true),
        (14, true, true, true),
        (15, true, true, true),
    ] {
        let packet = packets::MoveActorAbsolute {
            move_data: types::MoveActorAbsoluteData {
                actor_runtime_id: types::ActorRuntimeID(42),
                header: wire::U8(flags),
                position: [1.25, 64.5, -8.0].into(),
                ..Default::default()
            },
        };
        let mut bytes = Vec::new();
        let mut writer = wire::Writer::new(&mut bytes);
        wire::Encode::encode(&packet, &mut writer).unwrap();
        let body = Bytes::from(bytes);
        let direct = crate::actor::normalize_move_entity_body(&body, 2).unwrap();
        let structured = crate::actor::normalize_move_entity(packet, 2).unwrap();
        assert_eq!(direct, structured);
        let crate::ActorEvent::Move(movement) = direct else {
            panic!("move")
        };
        assert_eq!(movement.teleported, teleport);
        assert_eq!(movement.force_local_move, force_local);
        assert_eq!(movement.interpolation.force_completion, completion);
        assert_eq!(movement.controls_local_actor(), teleport || force_local);
    }
}

#[test]
fn delta_actor_moves_keep_local_authority_separate_from_forced_completion() {
    for (teleport, force_local, completion) in [
        (false, false, false),
        (true, false, false),
        (false, true, false),
        (true, true, false),
        (false, false, true),
        (true, false, true),
        (false, true, true),
        (true, true, true),
    ] {
        let crate::ActorEvent::Move(movement) = crate::actor::normalize_move_entity_delta(
            packets::MoveActorDelta {
                move_data: types::MoveActorDeltaData {
                    actor_runtime_id: types::ActorRuntimeID(42),
                    new_position_y: Some(wire::F32LE(64.5)),
                    force_move: teleport,
                    force_move_local_entity: force_local,
                    force_completion: completion,
                    ..Default::default()
                },
            },
            2,
        )
        .unwrap() else {
            panic!("move")
        };
        assert_eq!(movement.position, [None, Some(64.5), None]);
        assert_eq!(movement.teleported, teleport);
        assert_eq!(movement.force_local_move, force_local);
        assert_eq!(movement.interpolation.force_completion, completion);
        assert_eq!(movement.controls_local_actor(), teleport || force_local);
    }
}
