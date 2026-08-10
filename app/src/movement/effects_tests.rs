use protocol::{ActorEffectAction, ActorEffectEvent};
use sim::{Aabb, CollisionQuery, CollisionWorld, MovementInput, WorldQueryError};
use std::time::Duration;
use world::ChunkKey;

use super::physics::MovementEffectSource;
use super::runtime_system::physics_authority_fault_for_frame;
use super::{
    LocalMovementEffectTimeline, LocalPhysicsController, PhysicsCorrectionMode,
    PhysicsSampleContext,
};

struct EmptyWorld;

impl CollisionWorld for EmptyWorld {
    fn collision_boxes(&self, _query: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        Ok(CollisionQuery::synthetic(Vec::new()))
    }
}

struct UnavailableWorld;

impl CollisionWorld for UnavailableWorld {
    fn collision_boxes(&self, _query: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        Err(WorldQueryError::UnloadedChunk(ChunkKey::new(0, 0, 0)))
    }
}

fn event(
    action: ActorEffectAction,
    effect_id: i32,
    amplifier: i32,
    duration_ticks: i32,
    tick: u64,
) -> ActorEffectEvent {
    ActorEffectEvent {
        dimension: 0,
        actor_runtime_id: 42,
        action,
        effect_id,
        amplifier,
        particles: true,
        ambient: false,
        duration_ticks,
        tick,
    }
}

#[test]
fn finite_duration_counts_successful_local_ticks_not_packet_tick() {
    let mut timeline = LocalMovementEffectTimeline::default();
    timeline.begin_session(7);
    timeline.apply(7, 1, event(ActorEffectAction::Add, 8, 1, 2, u64::MAX));

    assert_eq!(timeline.snapshot().jump_boost, Some(1));
    assert_eq!(
        timeline.metadata_for_protocol_id(8),
        Some((1, u64::MAX, Some(2)))
    );
    timeline.commit_successful_tick();
    assert_eq!(timeline.snapshot().jump_boost, Some(1));
    timeline.commit_successful_tick();
    assert!(timeline.snapshot().is_empty());
}

#[test]
fn arrival_fifo_wins_even_when_packet_ticks_are_nonmonotonic() {
    let mut timeline = LocalMovementEffectTimeline::default();
    timeline.begin_session(3);
    timeline.apply(3, 1, event(ActorEffectAction::Add, 24, 0, -1, 200));
    timeline.apply(3, 2, event(ActorEffectAction::Update, 24, 3, -1, 10));
    assert_eq!(timeline.snapshot().levitation, Some(3));
    assert_eq!(timeline.metadata_for_protocol_id(24), Some((2, 10, None)));

    timeline.apply(3, 3, event(ActorEffectAction::Remove, 24, 0, 0, 100));
    assert!(timeline.snapshot().is_empty());
}

#[test]
fn session_replacement_clears_effects_and_rejects_stale_events() {
    let mut timeline = LocalMovementEffectTimeline::default();
    timeline.begin_session(1);
    timeline.apply(1, 5, event(ActorEffectAction::Add, 27, 0, -1, 1));
    assert!(timeline.snapshot().slow_falling);

    timeline.begin_session(2);
    timeline.apply(1, 6, event(ActorEffectAction::Add, 8, 0, -1, 2));
    assert!(timeline.snapshot().is_empty());
    assert_eq!(timeline.diagnostics().stale_or_wrong_session, 1);
}

#[test]
fn signed_amplifiers_unknown_values_zero_duration_and_stale_events_are_bounded() {
    let mut timeline = LocalMovementEffectTimeline::default();
    timeline.begin_session(4);
    timeline.apply(4, 1, event(ActorEffectAction::Add, 99, 0, 10, 1));
    timeline.apply(4, 2, event(ActorEffectAction::Add, 8, -2, 10, 1));
    assert_eq!(timeline.snapshot().jump_boost, Some(-2));
    timeline.apply(4, 3, event(ActorEffectAction::Add, 24, 1_024, 10, 1));
    assert_eq!(timeline.snapshot().levitation, Some(1_024));
    timeline.apply(4, 4, event(ActorEffectAction::Unknown(9), 27, 0, 10, 1));
    timeline.apply(4, 5, event(ActorEffectAction::Remove, 8, 0, 0, 1));
    timeline.apply(4, 6, event(ActorEffectAction::Remove, 24, 0, 0, 1));
    timeline.apply(4, 7, event(ActorEffectAction::Add, 27, 0, 0, 1));
    timeline.apply(4, 7, event(ActorEffectAction::Add, 27, 0, 10, 1));

    assert!(timeline.snapshot().is_empty());
    let diagnostics = timeline.diagnostics();
    assert_eq!(diagnostics.unknown_effect_or_action, 2);
    assert_eq!(diagnostics.stale_or_wrong_session, 1);
}

#[test]
fn unsafe_extreme_amplifiers_never_reach_or_poison_the_controller() {
    let mut timeline = LocalMovementEffectTimeline::default();
    timeline.begin_session(1);
    timeline.apply(1, 1, event(ActorEffectAction::Add, 8, i32::MAX, 20, 10));
    timeline.apply(1, 2, event(ActorEffectAction::Add, 24, i32::MIN, 20, 11));
    assert!(timeline.snapshot().is_empty());
    assert_eq!(timeline.diagnostics().unsupported_amplifier, 2);

    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position([0.0, 5.0, 0.0], 9, false);
    let frame = physics.advance_with_context_and_effects(
        Duration::from_millis(50),
        MovementInput {
            jumping: true,
            jump_pressed: true,
            ..MovementInput::default()
        },
        PhysicsSampleContext::default(),
        &EmptyWorld,
        &mut timeline,
    );

    assert_eq!(frame.completed_ticks, 1);
    assert!(frame.blocked.is_none());
    assert_eq!(physics_authority_fault_for_frame(&frame), None);
    assert!(physics.is_active());
    assert!(physics.state().unwrap().velocity.is_finite());
    assert_eq!(physics.history_len(), 1);

    let next = physics.advance_with_context_and_effects(
        Duration::from_millis(50),
        MovementInput::default(),
        PhysicsSampleContext::default(),
        &EmptyWorld,
        &mut timeline,
    );
    assert_eq!(next.completed_ticks, 1);
    assert!(next.blocked.is_none());
    assert_eq!(physics_authority_fault_for_frame(&next), None);
    assert!(physics.is_active());
    assert!(timeline.snapshot().is_empty());
}

#[test]
fn each_due_tick_snapshots_then_transactionally_consumes_effects() {
    let mut timeline = LocalMovementEffectTimeline::default();
    timeline.begin_session(1);
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position([0.0, 5.0, 0.0], 9, false);
    let baseline = physics.advance(
        Duration::from_millis(50),
        MovementInput::default(),
        &EmptyWorld,
    );
    assert_eq!(baseline.completed_ticks, 1);
    timeline.apply(1, 1, event(ActorEffectAction::Add, 27, 0, 1, 10));

    let frame = physics.advance_with_context_and_effects(
        Duration::from_millis(100),
        MovementInput::default(),
        PhysicsSampleContext::default(),
        &EmptyWorld,
        &mut timeline,
    );

    assert_eq!(frame.completed_ticks, 2);
    assert!(timeline.snapshot().is_empty());
    let velocity = physics.state().unwrap().velocity.y;
    assert!((velocity - -0.163_299_36).abs() < 1.0e-9, "{velocity}");
}

#[test]
fn failed_paused_and_reanchored_frames_do_not_consume_duration() {
    let mut timeline = LocalMovementEffectTimeline::default();
    timeline.begin_session(1);
    timeline.apply(1, 1, event(ActorEffectAction::Add, 8, 0, 2, 50));
    let mut physics = LocalPhysicsController::default();

    let inactive = physics.advance_with_context_and_effects(
        Duration::from_millis(50),
        MovementInput::default(),
        PhysicsSampleContext::default(),
        &EmptyWorld,
        &mut timeline,
    );
    assert_eq!(inactive.completed_ticks, 0);
    assert_eq!(timeline.metadata_for_protocol_id(8).unwrap().2, Some(2));

    physics.reanchor_network_position([0.0, 5.0, 0.0], 9, false);

    let paused = physics.advance_with_context_and_effects(
        Duration::ZERO,
        MovementInput::default(),
        PhysicsSampleContext::default(),
        &EmptyWorld,
        &mut timeline,
    );
    assert_eq!(paused.completed_ticks, 0);
    assert_eq!(timeline.metadata_for_protocol_id(8).unwrap().2, Some(2));

    let blocked = physics.advance_with_context_and_effects(
        Duration::from_millis(50),
        MovementInput::default(),
        PhysicsSampleContext::default(),
        &UnavailableWorld,
        &mut timeline,
    );
    assert_eq!(blocked.completed_ticks, 0);
    assert_eq!(timeline.metadata_for_protocol_id(8).unwrap().2, Some(2));

    physics.reanchor_network_position_before_advance([0.0, 5.0, 0.0], 20, false);
    let discarded = physics.advance_with_context_and_effects(
        Duration::from_millis(50),
        MovementInput::default(),
        PhysicsSampleContext::default(),
        &EmptyWorld,
        &mut timeline,
    );
    assert_eq!(discarded.completed_ticks, 0);
    assert_eq!(timeline.metadata_for_protocol_id(8).unwrap().2, Some(2));
}

#[test]
fn replay_and_hard_reanchor_do_not_rewind_authoritative_countdown() {
    let mut timeline = LocalMovementEffectTimeline::default();
    timeline.begin_session(1);
    timeline.apply(1, 1, event(ActorEffectAction::Add, 27, 0, 3, 99));
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position([0.0, 5.0, 0.0], 9, false);

    let frame = physics.advance_with_context_and_effects(
        Duration::from_millis(100),
        MovementInput::default(),
        PhysicsSampleContext::default(),
        &EmptyWorld,
        &mut timeline,
    );
    assert_eq!(timeline.metadata_for_protocol_id(27).unwrap().2, Some(1));

    let corrected = frame.samples[0].clone();
    physics
        .apply_correction(
            corrected.position,
            corrected.tick,
            false,
            PhysicsCorrectionMode::ReplayIfRetained,
            None,
            &EmptyWorld,
        )
        .unwrap();
    assert_eq!(timeline.metadata_for_protocol_id(27).unwrap().2, Some(1));

    physics.reanchor_network_position([0.0, 8.0, 0.0], 100, false);
    assert_eq!(timeline.metadata_for_protocol_id(27).unwrap().2, Some(1));
    let completed = physics.advance_with_context_and_effects(
        Duration::from_millis(50),
        MovementInput::default(),
        PhysicsSampleContext::default(),
        &EmptyWorld,
        &mut timeline,
    );
    assert_eq!(completed.completed_ticks, 1);
    assert!(timeline.snapshot().is_empty());
}
