//! Vertical projection of the recorded Zeqa knockback and correction burst.

use std::time::Duration;

use super::{
    LocalPhysicsController, MovementSource, MovementTicker, PhysicsCorrectionOutcome,
    PhysicsSampleContext, reconcile_prediction_correction,
};
use sim::{Aabb, CollisionQuery, CollisionWorld, MovementInput, Vec3, WorldQueryError};

#[derive(serde::Deserialize)]
struct RecordedTick {
    tick: u64,
    position_y: f32,
    #[serde(default)]
    delta_y: f32,
}

#[derive(serde::Deserialize)]
struct MotionFixture {
    anchor_tick: u64,
    anchor_y: f32,
    motion_tick: u64,
    applied_tick: u64,
    motion_y: f32,
    sent: Vec<RecordedTick>,
    corrected: Vec<RecordedTick>,
}

struct RecordedFloor(f64);

impl CollisionWorld for RecordedFloor {
    /// Supplies a flat floor for the isolated vertical trajectory.
    fn collision_boxes(&self, query: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        let floor = Aabb::new(
            Vec3::new(-8.0, self.0 - 1.0, -8.0),
            Vec3::new(8.0, self.0, 8.0),
        );
        Ok(CollisionQuery::synthetic(
            floor
                .intersects(query)
                .then_some(floor)
                .into_iter()
                .collect(),
        ))
    }
}

#[test]
fn zeqa_logged_vertical_motion_and_correction_replay() {
    let fixture: MotionFixture =
        serde_json::from_str(include_str!("fixtures/zeqa_vertical_motion.json")).unwrap();
    let anchor = [0.0, fixture.anchor_y, 0.0];
    let world = RecordedFloor(f64::from(
        fixture.anchor_y - protocol::PLAYER_NETWORK_OFFSET,
    ));
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position(anchor, fixture.anchor_tick, true);
    let mut ticker = MovementTicker::default();
    ticker.reset(1, fixture.anchor_tick, anchor);
    ticker.set_source(MovementSource::Physics);
    for sent in &fixture.sent {
        if sent.tick == fixture.applied_tick {
            physics.queue_server_motion([0.0, fixture.motion_y, 0.0], fixture.motion_tick);
        }
        let frame = physics.advance_with_context(
            Duration::from_millis(50),
            MovementInput::default(),
            PhysicsSampleContext::default(),
            &world,
        );
        assert!(frame.blocked.is_none(), "{:?}", frame.blocked);
        let sample = frame.samples.into_iter().next().unwrap();
        assert_eq!(sample.tick, sent.tick);
        assert!((sample.position[1] - sent.position_y).abs() < 2.0e-5);
        assert!((sample.movement[1] - sent.delta_y).abs() < 1.0e-6);
        ticker.enqueue_completed_physics(sample).unwrap();
    }

    let corrected = &fixture.corrected[0];
    // The log omits correction velocity. Infer the next Y velocity from the
    // following authoritative positions; horizontal state is isolated here.
    let inferred_velocity = fixture.corrected[1].position_y - corrected.position_y;
    let outcome = reconcile_prediction_correction(
        &mut ticker,
        &mut physics,
        [0.0, corrected.position_y, 0.0],
        corrected.tick,
        false,
        [0.0, inferred_velocity, 0.0],
        &world,
    )
    .unwrap();
    assert_eq!(
        outcome,
        Some(PhysicsCorrectionOutcome::Replayed {
            corrected_tick: corrected.tick,
            replayed_ticks: fixture.corrected.len() - 1,
        })
    );
    for expected in &fixture.corrected {
        let sample = physics.sample_at(expected.tick).unwrap();
        assert!((sample.position[1] - expected.position_y).abs() < 3.0e-5);
        assert!(!sample.grounded_after_tick);
    }
    assert_eq!(
        physics.state().unwrap().tick,
        fixture.sent.last().unwrap().tick
    );
    assert_eq!(physics.state().unwrap().jump_delay, 0);
    assert!(!physics.state().unwrap().on_ground);
}

#[test]
fn teleport_does_not_fabricate_a_second_raw_jump_press() {
    let anchor = [0.0, 100.0 + protocol::PLAYER_NETWORK_OFFSET, 0.0];
    let world = RecordedFloor(100.0);
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position(anchor, 33572, true);
    let mut ticker = MovementTicker::default();
    ticker.reset(1, 33572, anchor);
    ticker.set_source(MovementSource::Physics);
    let input = MovementInput {
        jumping: true,
        ..MovementInput::default()
    };
    let mut context = super::PhysicsSampleContext {
        input: super::TickInput {
            jump: semantic_input::ActionPhase {
                held: true,
                pressed: true,
                released: false,
            },
            ..Default::default()
        },
        ..Default::default()
    };
    let frame = physics.advance_with_context(Duration::from_millis(300), input, context, &world);
    context.input.jump.pressed = false;
    assert_eq!(frame.samples.len(), 6);
    for sample in frame.samples {
        ticker.enqueue_completed_physics(sample).unwrap();
    }
    super::reconcile_move_player_teleport(&mut ticker, &mut physics, anchor, 0, false, &world)
        .unwrap();
    // Discard the elapsed frame that predates the teleport.
    physics.advance_with_context(Duration::from_millis(50), input, context, &world);
    let frame = physics.advance_with_context(Duration::from_millis(50), input, context, &world);
    ticker
        .enqueue_completed_physics(frame.samples[0].clone())
        .unwrap();
    let flags = ticker.pending_snapshots()[0].flags.bits();
    assert_ne!(
        flags & protocol::PlayerInputFlags::JUMP_CURRENT_RAW.bits(),
        0
    );
    assert_eq!(
        flags & protocol::PlayerInputFlags::JUMP_PRESSED_RAW.bits(),
        0
    );
}

#[test]
fn teleport_preserves_jump_cooldown_until_the_next_allowed_takeoff() {
    let anchor = [0.0, 100.0 + protocol::PLAYER_NETWORK_OFFSET, 0.0];
    let world = RecordedFloor(100.0);
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position(anchor, 33572, true);
    let mut ticker = MovementTicker::default();
    ticker.reset(1, 33572, anchor);
    ticker.set_source(MovementSource::Physics);
    let input = MovementInput {
        jumping: true,
        ..MovementInput::default()
    };
    let frame = physics.advance(Duration::from_millis(300), input, &world);
    assert_eq!(frame.samples.len(), 6);
    for sample in frame.samples {
        ticker.enqueue_completed_physics(sample).unwrap();
    }
    let cooldown = physics.state().unwrap().jump_delay;
    assert!(cooldown > 0);
    super::reconcile_move_player_teleport(&mut ticker, &mut physics, anchor, 0, true, &world)
        .unwrap();
    assert_eq!(physics.state().unwrap().jump_delay, cooldown);
    assert!(
        physics
            .advance(Duration::from_millis(50), input, &world)
            .samples
            .is_empty()
    );
    for _ in 0..cooldown {
        let frame = physics.advance(Duration::from_millis(50), input, &world);
        assert_eq!(frame.samples.len(), 1);
        assert!(!frame.samples[0].processed.jump_initiated);
        assert!(frame.samples[0].grounded_after_tick);
    }
    let frame = physics.advance(Duration::from_millis(50), input, &world);
    assert!(frame.samples[0].processed.jump_initiated);
    assert!(!frame.samples[0].grounded_after_tick);

    physics.reanchor_network_position(anchor, 0, true);
    assert_eq!(physics.state().unwrap().jump_delay, 0);
    let frame = physics.advance(Duration::from_millis(50), input, &world);
    assert!(frame.samples[0].processed.jump_initiated);
}
