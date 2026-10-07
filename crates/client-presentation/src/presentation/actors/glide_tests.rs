//! Gliding bodies tilt and bank as vanilla's renderer turns them, about the feet.

use super::*;
use client_world::WorldAuthority;
use protocol::{ActorEvent, ActorMetadataValue, ActorSpawnEvent, WorldBootstrap, WorldEvent};

const GLIDING: u64 = 1 << 32;

/// A player admitted through the public event path, moving at `velocity` with `flags`.
fn player(velocity: [f32; 3], flags: u64) -> ActorSnapshot {
    let mut world = WorldAuthority::new(
        WorldBootstrap {
            local_player_unique_id: 1,
            local_player_runtime_id: 1,
            dimension: 0,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: 0,
            block_network_ids_are_hashes: false,
        },
        Arc::new(assets::RuntimeAssets::diagnostic()),
        None,
        [0.0; 3],
        None,
    );
    world
        .apply_ordered_event(
            WorldEvent::Actor(ActorEvent::Spawn(ActorSpawnEvent {
                dimension: 0,
                unique_id: 2,
                runtime_id: 2,
                kind: ActorKind::Player {
                    uuid: [2; 16],
                    username: "glider".into(),
                },
                position: [0.0; 3],
                velocity,
                pitch: 0.0,
                yaw: 0.0,
                head_yaw: 0.0,
                body_yaw: 0.0,
                held_item: Default::default(),
                metadata: Arc::from([]),
                attributes: Arc::from([]),
                properties: Arc::from([]),
                links: Arc::from([]),
            })),
            Some(1),
        )
        .unwrap();
    let mut actor = world.actor(2).unwrap().clone();
    actor.metadata.insert(0, ActorMetadataValue::Flags(flags));
    actor
}

fn glider(ticks: u32, pitch: f32, velocity: [f32; 3]) -> ActorSnapshot {
    let mut actor = player(velocity, GLIDING);
    actor.status.fall_fly_ticks = ticks;
    actor.pitch = pitch;
    actor.previous_pose.pitch = pitch;
    actor
}

fn close(actual: f32, expected: f32) -> bool {
    (actual - expected).abs() < 1e-3
}

fn column(rows: &[[f32; 4]; 3], axis: usize) -> [f32; 3] {
    [rows[0][axis], rows[1][axis], rows[2][axis]]
}

#[test]
fn only_a_gliding_actor_tilts() {
    assert_eq!(glide_rotation(&player([0.0; 3], 0), 0.5), None);
    let rows = rig_world_from_actor([1.0, 2.0, 3.0], 30.0, 1.0);
    assert_eq!(glide_tilted(rows, None), rows);
}

/// The local glide eases from upright to prone, reaching it once ticks plus alpha reach ten.
#[test]
fn pitch_tilt_eases_in_over_the_first_ten_gliding_ticks() {
    let [pitch, _] = glide_rotation(&glider(4, 0.0, [0.0; 3]), 0.5).unwrap();
    assert!(close(pitch, -90.0 * 4.5 * 4.5 / 100.0), "{pitch}");
    let [pitch, _] = glide_rotation(&glider(10, 0.0, [0.0; 3]), 0.0).unwrap();
    assert!(close(pitch, -90.0), "{pitch}");
    let [pitch, _] = glide_rotation(&glider(40, 30.0, [0.0; 3]), 0.7).unwrap();
    assert!(
        close(pitch, -120.0),
        "diving tips the head further down: {pitch}"
    );
    let [pitch, _] = glide_rotation(&glider(40, -90.0, [0.0; 3]), 0.7).unwrap();
    assert!(
        close(pitch, 0.0),
        "climbing straight up stays upright: {pitch}"
    );
}

/// Remote gliders never accumulate gliding ticks, so only the frame fraction eases them.
#[test]
fn a_glider_without_ticks_stays_nearly_upright() {
    let [pitch, _] = glide_rotation(&glider(0, 0.0, [0.0; 3]), 1.0).unwrap();
    assert!(close(pitch, -0.9), "{pitch}");
}

/// The body banks by the angle from the view to the horizontal motion, measured against the
/// unnormalised horizontal view, and keeps level inside the cross-product dead zone.
#[test]
fn turn_banks_toward_the_horizontal_motion() {
    let turn = |pitch: f32, velocity: [f32; 3]| {
        glide_rotation(&glider(20, pitch, velocity), 0.0).unwrap()[1]
    };
    assert!(close(turn(0.0, [0.5, 0.0, 0.5]), 45.0));
    assert!(close(turn(0.0, [-0.5, 0.0, 0.5]), -45.0));
    assert!(close(turn(0.0, [1.0, 0.0, 0.0]), 90.0));
    let lifted = turn(-60.0, [1.0, 0.0, 1.0]);
    // Native lookup-table trig quantizes the lifted view; the analytic angle is approximate.
    let analytic = (0.5 / 2.0_f32.sqrt()).acos().to_degrees();
    assert!((lifted - analytic).abs() < 0.01, "{lifted}");
    assert_eq!(turn(0.0, [0.05, 0.0, 1.0]), 0.0);
    assert_eq!(turn(0.0, [0.0; 3]), 0.0);
}

/// Prone at yaw 0, the head points along the facing (+Z) and the chest faces the ground.
#[test]
fn prone_tilt_points_the_head_forward_about_the_feet() {
    let feet = [3.0, 70.0, -2.0];
    let rows = glide_tilted(rig_world_from_actor(feet, 0.0, 1.0), Some([-90.0, 0.0]));
    let up = column(&rows, 1);
    let front = column(&rows, 2).map(|value| -value);
    assert!(
        close(up[0], 0.0) && close(up[1], 0.0) && close(up[2], 1.0),
        "{up:?}"
    );
    assert!(close(front[1], -1.0), "{front:?}");
    assert_eq!([rows[0][3], rows[1][3], rows[2][3]], feet);
}

/// Banking rolls the prone body about its own head-to-feet axis.
#[test]
fn bank_rolls_about_the_body_axis() {
    let rows = glide_tilted(
        rig_world_from_actor([0.0; 3], 0.0, 1.0),
        Some([-90.0, 90.0]),
    );
    let up = column(&rows, 1);
    assert!(close(up[2], 1.0), "the head still leads: {up:?}");
    let right = column(&rows, 0);
    assert!(
        close(right[1].abs(), 1.0),
        "the shoulders stand vertical: {right:?}"
    );
}
