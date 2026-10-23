use super::*;
use crate::actor_store::{ActorApplyResult, ActorStore};
use protocol::{ActorEvent, ActorKind, ActorMoveEvent, ActorPositionOrigin};

mod corners;
mod precision;
mod swing;

#[test]
fn movement_step_uses_the_rounded_float_reciprocal() {
    let mut store = player(0.0);
    store.apply(
        1,
        2,
        ActorEvent::Move(ActorMoveEvent {
            dimension: 0,
            runtime_id: 1,
            position: [Some(10.0), None, Some(-10.0)],
            position_origin: ActorPositionOrigin::Feet,
            pitch: None,
            yaw: None,
            head_yaw: None,
            on_ground: Some(true),
            teleported: false,
            force_local_move: false,
            player_mode: None,
            source_tick: None,
            interpolation: Default::default(),
        }),
    );
    store.advance_interpolation_ticks(1);
    let position = store.get(1).unwrap().position;
    assert_eq!(position[0].to_bits(), 0x4055_5556);
    assert_eq!(position[2].to_bits(), 0xc055_5556);
    store.advance_interpolation_ticks(2);
    assert_eq!(store.get(1).unwrap().position, [10.0, 0.0, -10.0]);
}

/// Spawns a remote player with the supplied wire pitch.
fn player(pitch: f32) -> ActorStore {
    let mut store = ActorStore::new(1, 0);
    let ActorEvent::Spawn(mut spawn) = crate::actor_store::tests::spawn(1, 1) else {
        unreachable!();
    };
    spawn.kind = ActorKind::Player {
        uuid: [1; 16],
        username: "Remote".into(),
    };
    spawn.pitch = pitch;
    spawn.position = [0.0; 3];
    assert_eq!(
        store.apply(1, 1, ActorEvent::Spawn(spawn)),
        ActorApplyResult::Inserted
    );
    store
}

/// Admits a rotation target through the same event route as a remote movement packet.
fn move_pitch(store: &mut ActorStore, sequence: u64, pitch: f32, teleport: bool, force: bool) {
    assert_eq!(
        store.apply(
            1,
            sequence,
            ActorEvent::Move(ActorMoveEvent {
                dimension: 0,
                runtime_id: 1,
                position: [None; 3],
                position_origin: ActorPositionOrigin::Feet,
                pitch: Some(pitch),
                yaw: None,
                head_yaw: None,
                on_ground: Some(true),
                teleported: teleport,
                force_local_move: false,
                player_mode: None,
                source_tick: None,
                interpolation: ActorInterpolation {
                    ticks: 6,
                    force_completion: force,
                },
            })
        ),
        ActorApplyResult::Updated
    );
}

#[test]
fn remote_spawn_pitch_uses_the_same_signed_angle_for_every_pose_endpoint() {
    for pitch in [-15.0, 345.0, 705.0, -375.0] {
        let store = player(pitch);
        let actor = store.get(1).unwrap();
        assert_eq!(actor.pitch, -15.0);
        assert_eq!(actor.previous_pose.pitch, -15.0);
        assert_eq!(actor.received_pose.pitch, -15.0);
    }
}

#[test]
fn wrapped_remote_pitch_stays_signed_during_moves_queued_targets_and_teleports() {
    for force in [false, true] {
        let mut store = player(15.0);
        move_pitch(&mut store, 2, 345.0, false, force);
        for expected in [5.0, -5.0, -15.0] {
            store.advance_interpolation_ticks(1);
            assert_eq!(store.get(1).unwrap().pitch, expected);
        }
        move_pitch(&mut store, 3, 350.0, false, false);
        for _ in 0..8 {
            store.advance_interpolation_ticks(1);
            let actor = store.get(1).unwrap();
            assert!((-15.0..=5.0).contains(&actor.pitch));
            assert_eq!(actor.received_pose.pitch, -10.0);
            assert_eq!(actor.last_received_pose().pitch, -10.0);
        }
        assert_eq!(store.get(1).unwrap().pitch, -10.0);
        move_pitch(&mut store, 4, 705.0, true, false);
        let actor = store.get(1).unwrap();
        assert_eq!(actor.pitch, -15.0);
        assert_eq!(actor.previous_pose.pitch, -15.0);
        assert_eq!(actor.received_pose.pitch, -15.0);
    }
}

#[test]
fn signed_pitch_keeps_already_signed_values_bit_for_bit() {
    for pitch in [-180.0_f32, -89.125, -0.0, 0.0, 10.12345, 179.999] {
        assert_eq!(signed_pitch(pitch).to_bits(), pitch.to_bits());
    }
}

/// Moves actor 1 to a native origin height through the remote movement route.
fn move_native_y(store: &mut ActorStore, sequence: u64, native_y: f32, ground: bool, snap: bool) {
    let offset = store.get(1).unwrap().network_position_offset();
    store.apply(
        1,
        sequence,
        ActorEvent::Move(ActorMoveEvent {
            dimension: 0,
            runtime_id: 1,
            position: [None, Some(native_y - offset), None],
            position_origin: ActorPositionOrigin::Feet,
            pitch: None,
            yaw: None,
            head_yaw: None,
            on_ground: Some(ground),
            teleported: snap,
            force_local_move: false,
            player_mode: None,
            source_tick: None,
            interpolation: Default::default(),
        }),
    );
}

#[test]
fn grounded_interpolation_lifts_onto_a_block_surface_except_minecarts_and_airborne_actors() {
    for (kind, ground, lifted) in [
        ("minecraft:zombie", true, true),
        ("player", true, true),
        ("minecraft:chest_minecart", true, false),
        ("minecraft:zombie", false, false),
    ] {
        let mut store = ActorStore::new(1, 0);
        let ActorEvent::Spawn(mut spawn) = crate::actor_store::tests::spawn(1, 1) else {
            unreachable!();
        };
        spawn.kind = match kind {
            "player" => ActorKind::Player {
                uuid: [1; 16],
                username: "Remote".into(),
            },
            identifier => ActorKind::Entity {
                identifier: identifier.into(),
            },
        };
        store.apply(1, 1, ActorEvent::Spawn(spawn));
        move_native_y(&mut store, 2, 63.99, ground, true);
        move_native_y(&mut store, 3, 63.995, ground, false);
        store.advance_interpolation_ticks(1);
        let native = store.get(1).unwrap().native_position()[1];
        let expected = if lifted { 64.0 } else { 63.99 + 0.005 / 3.0 };
        assert!(
            (native - expected).abs() < 1.0e-4,
            "{kind} {ground}: {native}"
        );
        store.advance_interpolation_ticks(2);
        let native = store.get(1).unwrap().native_position()[1];
        assert!(
            (native - 63.995).abs() < 1.0e-4,
            "final step lands on the target: {native}"
        );
    }
}
