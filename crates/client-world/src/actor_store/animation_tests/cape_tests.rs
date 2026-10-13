//! Remote cape inputs after movement, hurt, teleport and interpolation-reset packets.
use protocol::{ActorEvent, ActorKind, MovePlayerEvent, MovePlayerMode};
use render_model::java_animation::{JavaCapeInput, java_cape_bone};

use crate::actor_store::ActorStore;

const REMOTE_RUNTIME_ID: u64 = 7;

/// Spawns a remote player with an original, deterministic animation fixture.
fn store() -> ActorStore {
    let assets = actor_animation::test_support::counting_random_assets_for("minecraft:player");
    let mut store = ActorStore::new_with_entity_assets(1, 0, assets);
    let ActorEvent::Spawn(mut spawn) = crate::actor_store::tests::spawn(REMOTE_RUNTIME_ID, 70)
    else {
        unreachable!();
    };
    spawn.kind = ActorKind::Player {
        uuid: [REMOTE_RUNTIME_ID as u8; 16],
        username: "cape-fixture".into(),
    };
    spawn.position = [0.0; 3];
    store.apply(1, 1, ActorEvent::Spawn(spawn));
    store.advance_interpolation_ticks(1);
    store
}

/// Sends a player position in network coordinates through the ordinary packet handler.
fn movement(store: &mut ActorStore, sequence: u64, position: [f32; 3], mode: MovePlayerMode) {
    store.apply_player_move(
        1,
        sequence,
        0,
        MovePlayerEvent {
            runtime_id: REMOTE_RUNTIME_ID,
            position: [
                position[0],
                position[1] + protocol::PLAYER_NETWORK_OFFSET,
                position[2],
            ],
            pitch: 0.0,
            yaw: 0.0,
            head_yaw: 0.0,
            mode,
            on_ground: true,
            teleported: mode.is_teleport(),
            source_tick: sequence,
            riding_runtime_id: 0,
            teleport_cause: None,
        },
    );
}

/// Reads the cape query from the same retained tick inputs as authored actor animation.
fn flap(store: &ActorStore) -> f32 {
    store
        .actor_rig(REMOTE_RUNTIME_ID)
        .unwrap()
        .particle_queries(store.get(REMOTE_RUNTIME_ID).unwrap(), 1.0, 0)
        .query("cape_flap_amount")
}

/// Checks Java cape direction throughout the frame and the Bedrock flap's hanging range.
fn assert_hanging(store: &ActorStore) {
    let motion = store.actor_rig(REMOTE_RUNTIME_ID).unwrap().java;
    for alpha in [0.0, 0.25, 0.5, 0.75, 1.0] {
        let lerp = |[from, to]: [f32; 2]| from + (to - from) * alpha;
        let chase: [f32; 3] =
            std::array::from_fn(|axis| lerp([motion.cape[0][axis], motion.cape[1][axis]]));
        let input = JavaCapeInput {
            chase: chase.into(),
            body_yaw: motion.body_yaw_at(alpha),
            bob: lerp(motion.bob),
            walked: lerp(motion.walked),
            sneaking: false,
        };
        let (rotation, _) = java_cape_bone(&input, [0.0, 1.5, 0.1875].into());
        let down = rotation.mul_vec3([0.0, -1.0, 0.0].into());
        assert!(
            down.y <= 1e-6 && down.z >= -1e-6,
            "alpha {alpha}: {input:?}"
        );
    }
    // The authored cape pitch stays below horizontal for these ordinary movement inputs.
    assert!((0.0..=2.0 / 3.0).contains(&flap(store)));
}

#[test]
fn remote_knockback_and_repeated_sprint_hits_do_not_max_out_cape_flap() {
    for step in [[0.0, 0.3, 0.3], [0.0, 0.3, -0.3], [0.3, 0.3, 0.0]] {
        let mut store = store();
        for hit in 0..3 {
            let sequence = 2 + hit * 3;
            store.apply_motion(
                sequence,
                protocol::ActorMotionEvent {
                    actor_runtime_id: REMOTE_RUNTIME_ID,
                    motion: [step[0], 0.4, step[2]],
                    tick: sequence,
                },
            );
            let target = step.map(|value| value * (hit + 1) as f32);
            movement(&mut store, sequence + 1, target, MovePlayerMode::Normal);
            store.apply(
                1,
                sequence + 2,
                ActorEvent::Status(protocol::ActorStatusEvent {
                    runtime_id: REMOTE_RUNTIME_ID,
                    kind: protocol::ActorStatusKind::Hurt,
                    data: 0,
                }),
            );
            for _ in 0..3 {
                store.advance_interpolation_ticks(1);
                assert!((flap(&store) - 0.4).abs() < 1e-5);
                assert_hanging(&store);
            }
            store.advance_interpolation_ticks(1);
            assert_eq!(
                flap(&store),
                0.0,
                "retained packet velocity must not keep flapping"
            );
            assert_hanging(&store);
        }
    }
}

#[test]
fn remote_teleports_and_interpolation_resets_discard_both_cape_endpoints() {
    for mode in [MovePlayerMode::Teleport, MovePlayerMode::Reset] {
        for target in [[0.0, 3.0, 5.0], [5.0, -3.0, -5.0], [0.0, 0.0, 30.0]] {
            let mut store = store();
            movement(&mut store, 2, [0.0, 0.0, 0.6], MovePlayerMode::Normal);
            store.advance_interpolation_ticks(3);
            assert_ne!(
                store.actor_rig(REMOTE_RUNTIME_ID).unwrap().java.cape[1],
                [0.0; 3]
            );
            movement(&mut store, 3, target, mode);
            assert_eq!(
                store.actor_rig(REMOTE_RUNTIME_ID).unwrap().java.cape,
                [[0.0; 3]; 2]
            );
            for _ in 0..4 {
                store.advance_interpolation_ticks(1);
                assert_eq!(
                    store.actor_rig(REMOTE_RUNTIME_ID).unwrap().java.cape,
                    [[0.0; 3]; 2]
                );
                assert_eq!(flap(&store), 0.0);
                assert_hanging(&store);
            }
            movement(
                &mut store,
                4,
                [target[0], target[1], target[2] + 0.3],
                MovePlayerMode::Normal,
            );
            store.advance_interpolation_ticks(1);
            assert!((flap(&store) - 0.4).abs() < 1e-4);
            assert_ne!(
                store.actor_rig(REMOTE_RUNTIME_ID).unwrap().java.cape[1],
                [0.0; 3]
            );
            assert_hanging(&store);
        }
    }
}

#[test]
fn unflagged_large_remote_position_jump_does_not_become_cape_motion() {
    let mut store = store();
    movement(&mut store, 2, [0.0, 0.0, 60.0], MovePlayerMode::Normal);
    for _ in 0..4 {
        store.advance_interpolation_ticks(1);
        assert_eq!(flap(&store), 0.0);
        assert_eq!(
            store.actor_rig(REMOTE_RUNTIME_ID).unwrap().java.cape,
            [[0.0; 3]; 2]
        );
        assert_hanging(&store);
    }
}
