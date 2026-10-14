use super::*;
use crate::test_support::survival_mining::{completed, evidence, ticker_with_ticks};

struct EmptyWorld;

impl sim::CollisionWorld for EmptyWorld {
    /// Supplies empty space so the test measures only input slowdown.
    fn collision_boxes(
        &self,
        _: sim::Aabb,
    ) -> Result<sim::CollisionQuery<Vec<sim::Aabb>>, sim::WorldQueryError> {
        Ok(sim::CollisionQuery::synthetic(Vec::new()))
    }
}

#[test]
fn a_frame_use_waits_for_every_completed_input_and_starts_on_the_next_tick() {
    let mut movement = ticker_with_ticks(2);
    let mut runtime = ItemUseRuntime::default();
    runtime.observe_press(true);
    assert!(runtime.frame_sample(&movement, true, true).is_none());
    let mut packets = Vec::new();
    crate::movement::flush_player_auth_inputs(&mut movement, 1, Some(evidence()), |_, packet| {
        packets.push(packet);
        Ok::<_, ()>(())
    })
    .unwrap();
    assert!(runtime.frame_sample(&movement, true, true).is_none());
    crate::movement::flush_player_auth_inputs(&mut movement, 8, Some(evidence()), |_, packet| {
        packets.push(packet);
        Ok::<_, ()>(())
    })
    .unwrap();
    let sample = runtime.frame_sample(&movement, true, true).unwrap();
    assert_eq!(sample.tick, 103);
    let mut swings = SwingTracker::default();
    let use_frame = UseFrame {
        position: sample.position,
        ..item_frame(sample.tick, true, stack(0, BOW, 1), "minecraft:bow")
    };
    admit_on_tick(
        &mut runtime,
        &mut swings,
        &mut movement,
        &use_frame,
        42,
        6,
        |batch| {
            packets.extend(batch);
            Ok(())
        },
    );
    assert!(runtime.is_using());
    assert!(runtime.frame_sample(&movement, true, true).is_none());
    let mut physics = crate::movement::LocalPhysicsController::default();
    physics.reanchor_network_position(sample.position, movement.completed_tick(), false);
    let mut plain = physics.clone();
    let controls = sim::MovementInput {
        forward: 1.0,
        ..Default::default()
    };
    let baseline = plain
        .advance(std::time::Duration::from_millis(50), controls, &EmptyWorld)
        .samples
        .remove(0);
    let next = physics
        .advance(
            std::time::Duration::from_millis(50),
            sim::MovementInput {
                item_use_movement_modifier: runtime.movement_modifier(),
                ..controls
            },
            &EmptyWorld,
        )
        .samples
        .remove(0);
    assert!(next.movement[2].abs() < baseline.movement[2].abs());
    assert!(next.move_vector[1].abs() < baseline.move_vector[1].abs());
    movement.enqueue_completed_physics(next).unwrap();
    crate::movement::flush_player_auth_inputs(&mut movement, 8, Some(evidence()), |_, packet| {
        packets.push(packet);
        Ok::<_, ()>(())
    })
    .unwrap();
    assert_eq!(packets.len(), 4);
    assert!(observe_transaction(&packets[2]).is_some());
    for (packet, tick, started) in [
        (&packets[0], 101, false),
        (&packets[1], 102, false),
        (&packets[3], 103, true),
    ] {
        let input = protocol::player_auth_input_trace_sample(packet).unwrap();
        assert_eq!(input.tick, tick);
        assert_eq!(input.flag_names.contains(&"StartUsingItem"), started);
    }
    assert_eq!(
        runtime.frame_sample(&movement, true, true).unwrap().tick,
        104
    );
}

#[test]
fn action_facing_can_be_reserved_after_completed_inputs_are_admitted() {
    let mut movement = ticker_with_ticks(2);
    crate::movement::flush_player_auth_inputs(&mut movement, 8, Some(evidence()), |_, _| {
        Ok::<_, ()>(())
    })
    .unwrap();
    let sample = movement.between_ticks_sample().unwrap();
    assert!(movement.override_action_rotation(sample.tick, 15.0, 30.0));
    let before = completed(sample.tick);
    movement.enqueue_completed_physics(before.clone()).unwrap();
    let after = movement.pending_snapshots()[0];
    assert_eq!((after.pitch, after.yaw), (15.0, 30.0));
    assert_eq!(after.head_yaw, before.head_yaw);
    assert_eq!(after.camera_orientation, before.camera_orientation);
    movement
        .enqueue_completed_physics(completed(sample.tick + 1))
        .unwrap();
    assert_eq!(movement.pending_snapshots()[1].yaw, before.yaw);
}
