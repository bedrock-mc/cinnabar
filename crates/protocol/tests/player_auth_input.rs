use protocol::{PlayerAuthInputSnapshot, PlayerInputFlags, PlayerInputMode, player_auth_input};
use valentine::bedrock::version::v1_26_30::{
    InputFlag, McpePacketData, McpePacketName, PlayerAuthInputPacketInputMode,
    PlayerAuthInputPacketInteractionModel, PlayerAuthInputPacketPlayMode,
};

fn snapshot() -> PlayerAuthInputSnapshot {
    PlayerAuthInputSnapshot {
        tick: 1_234,
        position: [1.25, 64.0, -2.5],
        delta: [0.25, 0.0, -0.5],
        move_vector: [-1.0, 1.0],
        analogue_move_vector: [-0.75, 0.75],
        raw_move_vector: [-1.0, 1.0],
        pitch: 10.5,
        yaw: 20.25,
        head_yaw: 30.75,
        camera_orientation: [0.25, -0.5, -0.75],
        flags: PlayerInputFlags::UP
            | PlayerInputFlags::LEFT
            | PlayerInputFlags::JUMPING
            | PlayerInputFlags::SPRINTING,
        input_mode: PlayerInputMode::Mouse,
    }
}

#[test]
fn vendor_neutral_snapshot_maps_to_protocol_1001_player_auth_input() {
    let packet = player_auth_input(snapshot()).expect("valid player input");
    assert_eq!(packet.header.id, McpePacketName::PacketPlayerAuthInput);
    assert_eq!(
        (packet.header.from_subclient, packet.header.to_subclient),
        (0, 0)
    );

    let McpePacketData::PacketPlayerAuthInput(input) = packet.data else {
        panic!("expected PlayerAuthInput payload");
    };
    assert_eq!(input.tick, 1_234);
    assert_eq!(
        (input.position.x, input.position.y, input.position.z),
        (1.25, 64.0, -2.5)
    );
    assert_eq!(
        (input.delta.x, input.delta.y, input.delta.z),
        (0.25, 0.0, -0.5)
    );
    assert_eq!((input.move_vector.x, input.move_vector.z), (-1.0, 1.0));
    assert_eq!(
        (input.analogue_move_vector.x, input.analogue_move_vector.z),
        (-0.75, 0.75)
    );
    assert_eq!(
        (input.raw_move_vector.x, input.raw_move_vector.z),
        (-1.0, 1.0)
    );
    assert_eq!(
        (input.pitch, input.yaw, input.head_yaw),
        (10.5, 20.25, 30.75)
    );
    assert_eq!(
        (
            input.camera_orientation.x,
            input.camera_orientation.y,
            input.camera_orientation.z
        ),
        (0.25, -0.5, -0.75)
    );
    assert_eq!(input.interact_rotation.x, input.pitch);
    assert_eq!(input.interact_rotation.z, input.yaw);
    assert_eq!(input.input_mode, PlayerAuthInputPacketInputMode::Mouse);
    assert_eq!(input.play_mode, PlayerAuthInputPacketPlayMode::Normal);
    assert_eq!(
        input.interaction_model,
        PlayerAuthInputPacketInteractionModel::Unknown(-1)
    );
    assert_eq!(
        input.input_data,
        InputFlag::UP | InputFlag::LEFT | InputFlag::JUMPING | InputFlag::SPRINTING
    );
    assert!(input.transaction.is_none());
    assert!(input.item_stack_request.is_none());
    assert!(input.content.is_none());
    assert!(input.block_action.is_none());
}

#[test]
fn player_auth_input_rejects_non_finite_state_and_ticks_outside_wire_range() {
    let mut invalid_position = snapshot();
    invalid_position.position[1] = f32::NAN;
    assert!(player_auth_input(invalid_position).is_err());

    let mut invalid_rotation = snapshot();
    invalid_rotation.yaw = f32::INFINITY;
    assert!(player_auth_input(invalid_rotation).is_err());

    let mut invalid_tick = snapshot();
    invalid_tick.tick = i64::MAX as u64 + 1;
    assert!(player_auth_input(invalid_tick).is_err());
}
