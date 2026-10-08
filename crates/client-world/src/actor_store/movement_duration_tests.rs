use super::*;
use protocol::wire::valentine::bedrock::{
    codec::BedrockCodec,
    version::v1_26_51::{
        ActorRuntimeId, MoveActorAbsoluteData, MoveActorAbsolutePacket, MoveActorDeltaData,
        MoveActorDeltaPacket, Vec3,
    },
};

fn orb() -> ActorStore {
    let ActorEvent::Spawn(mut spawn) = tests::spawn(7, 70) else {
        unreachable!();
    };
    spawn.kind = ActorKind::Entity {
        identifier: "minecraft:xp_orb".into(),
    };
    spawn.position = [0.0; 3];
    let mut store = ActorStore::new(1, 0);
    assert_eq!(
        store.apply(1, 1, ActorEvent::Spawn(spawn)),
        ActorApplyResult::Inserted
    );
    store
}

fn delta(x: f32, ticks: u64, force_completion: bool) -> ActorEvent {
    wire_delta(MoveActorDeltaData {
        new_position_x: Some(x),
        ticks,
        force_completion,
        ..Default::default()
    })
}

fn wire_delta(mut move_data: MoveActorDeltaData) -> ActorEvent {
    move_data.actor_runtime_id = ActorRuntimeId {
        actor_runtime_id: 7,
    };
    let packet = MoveActorDeltaPacket { move_data };
    let mut bytes = Vec::new();
    packet.encode(&mut bytes).unwrap();
    let decoded = MoveActorDeltaPacket::decode(&mut bytes.as_slice(), ()).unwrap();
    let event = protocol::into_world_event(
        protocol::Packet::from_payload_with_subclients(decoded, 0, 0),
        0,
    )
    .unwrap()
    .unwrap();
    let protocol::WorldEvent::Actor(event) = event else {
        panic!("movement must survive the wire normalization route");
    };
    event
}

#[test]
fn xp_orb_wire_delta_uses_server_interpolation_duration_with_three_tick_floor() {
    for ticks in [0, 1, 2, 3, 10] {
        let mut store = orb();
        store.apply(1, 2, delta(10.0, ticks, false));
        let duration = ticks.max(u64::from(ACTOR_INTERPOLATION_TICKS));
        for tick in 1..=duration {
            store.advance_interpolation_ticks(1);
            let actor = store.get(7).unwrap();
            let expected = 10.0 * tick as f32 / duration as f32;
            assert!(
                (actor.position[0] - expected).abs() < 0.000_01,
                "wire ticks {ticks}, completed tick {tick}: position {:?}, expected X {expected}",
                actor.position
            );
        }
        store.advance_interpolation_ticks(1);
        assert_eq!(store.get(7).unwrap().position, [10.0, 0.0, 0.0]);
    }
}

#[test]
fn xp_orb_wire_force_completion_finishes_before_a_later_ordinary_target() {
    let mut store = orb();
    store.apply(1, 2, delta(9.0, u64::from(ACTOR_INTERPOLATION_TICKS), true));
    store.advance_interpolation_ticks(1);
    assert_eq!(store.get(7).unwrap().position[0], 3.0);
    store.apply(
        1,
        3,
        delta(21.0, u64::from(ACTOR_INTERPOLATION_TICKS), false),
    );
    store.advance_interpolation_ticks(1);
    assert_eq!(
        store.get(7).unwrap().position[0],
        6.0,
        "an ordinary update must wait for the completing target"
    );
    store.advance_interpolation_ticks(1);
    assert_eq!(store.get(7).unwrap().position[0], 9.0);
    store.advance_interpolation_ticks(1);
    assert_eq!(store.get(7).unwrap().position[0], 15.0);
}

#[test]
fn xp_orb_absolute_wire_completion_uses_the_same_target_ordering() {
    let packet = MoveActorAbsolutePacket {
        move_data: MoveActorAbsoluteData {
            actor_runtime_id: ActorRuntimeId {
                actor_runtime_id: 7,
            },
            header: 1 << 3,
            position: Vec3 {
                x: 9.0,
                y: 0.0,
                z: 0.0,
            },
            rotation_x: 0,
            rotation_y: 0,
            rotation_y_head: 0,
        },
    };
    let mut bytes = Vec::new();
    packet.encode(&mut bytes).unwrap();
    let decoded = MoveActorAbsolutePacket::decode(&mut bytes.as_slice(), ()).unwrap();
    let protocol::WorldEvent::Actor(movement) = protocol::into_world_event(
        protocol::Packet::from_payload_with_subclients(decoded, 0, 0),
        0,
    )
    .unwrap()
    .unwrap() else {
        panic!("absolute movement must normalize")
    };
    let mut store = orb();
    store.apply(1, 2, movement);
    store.advance_interpolation_ticks(1);
    store.apply(1, 3, delta(21.0, 0, false));
    store.advance_interpolation_ticks(1);
    assert_eq!(store.get(7).unwrap().position[0], 6.0);
}

#[test]
fn xp_orb_a_new_completing_target_can_replace_the_current_target() {
    let mut store = orb();
    store.apply(1, 2, delta(9.0, u64::from(ACTOR_INTERPOLATION_TICKS), true));
    store.advance_interpolation_ticks(1);
    store.apply(
        1,
        3,
        delta(18.0, u64::from(ACTOR_INTERPOLATION_TICKS), true),
    );
    store.advance_interpolation_ticks(1);
    assert_eq!(store.get(7).unwrap().position[0], 8.0);
}

#[test]
fn xp_orb_long_wire_durations_preserve_counter_width() {
    for ticks in [300, (1_u64 << u32::BITS) + 300] {
        let mut store = orb();
        store.apply(1, 2, delta(300.0, ticks, false));
        store.advance_interpolation_ticks(1);
        assert_eq!(store.get(7).unwrap().position[0], 1.0);
        assert_eq!(store.get(7).unwrap().interpolation_ticks_remaining, 299);
    }
}

#[test]
fn xp_orb_unsupported_wire_durations_are_skipped_without_poisoning_later_movement() {
    for ticks in [1_u64 << u32::BITS, i32::MAX as u64 + 1, u64::MAX] {
        let mut store = orb();
        assert_eq!(
            store.apply(1, 2, delta(10.0, ticks, false)),
            ActorApplyResult::Updated
        );
        store.advance_interpolation_ticks(1);
        assert_eq!(store.get(7).unwrap().position, [0.0; 3]);
        assert_eq!(store.ignored_movement_components, 1);
        store.apply(1, 3, delta(3.0, 0, false));
        store.advance_interpolation_ticks(ACTOR_INTERPOLATION_TICKS);
        assert_eq!(store.get(7).unwrap().position[0], 3.0);
    }
}

#[test]
fn xp_orb_partial_deltas_merge_into_the_latest_wire_pose_while_completion_waits() {
    let mut store = orb();
    store.apply(1, 2, delta(9.0, 0, true));
    store.advance_interpolation_ticks(1);
    store.apply(
        1,
        3,
        wire_delta(MoveActorDeltaData {
            new_position_x: Some(21.0),
            new_position_y: Some(6.0),
            ..Default::default()
        }),
    );
    store.apply(
        1,
        4,
        wire_delta(MoveActorDeltaData {
            new_position_z: Some(9.0),
            ..Default::default()
        }),
    );
    store.advance_interpolation_ticks(4);
    assert_eq!(store.get(7).unwrap().position, [21.0, 6.0, 9.0]);
}

#[test]
fn xp_orb_rotation_keeps_three_ticks_when_position_has_a_longer_duration() {
    let mut store = orb();
    store.apply(
        1,
        2,
        wire_delta(MoveActorDeltaData {
            new_position_x: Some(10.0),
            rotation_x: Some(64),
            rotation_y: Some(64),
            rotation_y_head: Some(64),
            ticks: 10,
            ..Default::default()
        }),
    );
    store.advance_interpolation_ticks(ACTOR_INTERPOLATION_TICKS);
    let actor = store.get(7).unwrap();
    assert_eq!(actor.position[0], 3.0);
    assert_eq!([actor.pitch, actor.yaw, actor.head_yaw], [90.0; 3]);
}

#[test]
fn xp_orb_teleport_and_lifetime_replacement_discard_a_queued_target() {
    for replace in [false, true] {
        let mut store = orb();
        store.apply(1, 2, delta(9.0, 0, true));
        store.advance_interpolation_ticks(1);
        store.apply(1, 3, delta(21.0, 0, false));
        if replace {
            store.apply(
                1,
                4,
                ActorEvent::Remove(protocol::ActorRemoveEvent {
                    dimension: 0,
                    unique_id: 70,
                }),
            );
            let ActorEvent::Spawn(mut spawn) = tests::spawn(7, 71) else {
                unreachable!()
            };
            spawn.position = [40.0, 0.0, 0.0];
            store.apply(1, 5, ActorEvent::Spawn(spawn));
        } else {
            let ActorEvent::Move(mut movement) = delta(40.0, 0, false) else {
                unreachable!()
            };
            movement.teleported = true;
            store.apply(1, 4, ActorEvent::Move(movement));
        }
        store.advance_interpolation_ticks(10);
        assert_eq!(store.get(7).unwrap().position, [40.0, 0.0, 0.0]);
    }
}
