use std::time::Duration;

use sim::{Aabb, CollisionQuery, CollisionWorld, MovementInput, WorldQueryError};
use world::ChunkKey;

use super::*;
use crate::movement::{
    LocalPhysicsController, LocalPhysicsFrame, PhysicsCorrectionMode, PhysicsCorrectionOutcome,
    PhysicsMovementSample, PhysicsSampleContext,
};

// Protocol-2193 identities: gophertunnel b725d82563e9,
// minecraft/protocol/packet/mob_effect.go. These tests do not apply mining rates.
const MINING_IDS: [i32; 3] = [3, 4, 26];

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
    id: i32,
    amplifier: i32,
    duration: i32,
    tick: u64,
) -> ActorEffectEvent {
    ActorEffectEvent {
        dimension: 0,
        actor_runtime_id: 42,
        action,
        effect_id: id,
        amplifier,
        particles: true,
        ambient: false,
        duration_ticks: duration,
        tick,
    }
}

fn assert_retained(
    timeline: &LocalMovementEffectTimeline,
    id: i32,
    amplifier: i32,
    metadata: (u64, u64, Option<u32>),
) {
    assert_eq!(
        timeline.metadata_for_protocol_id(id),
        Some(metadata),
        "effect {id}"
    );
    let retained = timeline
        .active
        .iter()
        .flatten()
        .find(|entry| entry.sequence == metadata.0);
    assert_eq!(
        retained.map(|entry| entry.amplifier),
        Some(amplifier),
        "effect {id}"
    );
}

#[test]
fn passive_effects_retain_signed_amplifiers_and_update_remove_independently() {
    for id in MINING_IDS {
        for amplifier in [i32::MIN, -17, 0, i32::MAX] {
            let mut timeline = LocalMovementEffectTimeline::default();
            timeline.begin_session(7);
            timeline.apply(
                7,
                1,
                event(ActorEffectAction::Add, id, amplifier, -1, u64::MAX),
            );
            assert_retained(&timeline, id, amplifier, (1, u64::MAX, None));
            assert_eq!(timeline.snapshot(), MovementEffects::default());
            timeline.apply(7, 2, event(ActorEffectAction::Update, id, -2, 2, 0));
            assert_retained(&timeline, id, -2, (2, 0, Some(2)));
            timeline.apply(
                7,
                3,
                event(ActorEffectAction::Remove, id, i32::MAX, -1, 100),
            );
            assert_eq!(timeline.metadata_for_protocol_id(id), None);
            assert_eq!(timeline.diagnostics().unsupported_amplifier, 0);
            assert_eq!(timeline.diagnostics().unknown_effect_or_action, 0);
        }
    }
}

#[test]
fn mixed_effects_share_fifo_but_not_slots_or_vertical_safety_limits() {
    let mut timeline = LocalMovementEffectTimeline::default();
    timeline.begin_session(7);
    timeline.apply(7, 1, event(ActorEffectAction::Add, 8, -2, 10, 300));
    timeline.apply(7, 2, event(ActorEffectAction::Add, 3, i32::MIN, 2, 200));
    timeline.apply(7, 3, event(ActorEffectAction::Add, 4, i32::MAX, -1, 100));
    timeline.apply(7, 4, event(ActorEffectAction::Add, 26, -3, 1, 0));
    let vertical = timeline.snapshot();
    assert_retained(&timeline, 3, i32::MIN, (2, 200, Some(2)));
    assert_retained(&timeline, 4, i32::MAX, (3, 100, None));
    assert_retained(&timeline, 26, -3, (4, 0, Some(1)));

    timeline.apply(6, 5, event(ActorEffectAction::Remove, 3, 0, 0, 0));
    timeline.apply(7, 4, event(ActorEffectAction::Remove, 4, 0, 0, 0));
    timeline.apply(7, 5, event(ActorEffectAction::Update, 8, 1_025, 2, 0));
    timeline.apply(7, 6, event(ActorEffectAction::Unknown(9), 26, 0, 0, 0));
    timeline.apply(7, 7, event(ActorEffectAction::Add, 99, 0, 2, 0));
    // An unknown event still consumes the one shared arrival sequence.
    timeline.apply(7, 7, event(ActorEffectAction::Remove, 26, 0, 0, 0));
    assert_eq!(timeline.snapshot(), vertical);
    assert_retained(&timeline, 26, -3, (4, 0, Some(1)));
    assert_eq!(timeline.diagnostics().stale_or_wrong_session, 3);
    assert_eq!(timeline.diagnostics().unsupported_amplifier, 1);
    assert_eq!(timeline.diagnostics().unknown_effect_or_action, 2);

    timeline.apply(7, 8, event(ActorEffectAction::Remove, 3, 0, 0, 0));
    assert_eq!(timeline.metadata_for_protocol_id(3), None);
    assert_retained(&timeline, 4, i32::MAX, (3, 100, None));
    timeline.apply(7, 9, event(ActorEffectAction::Update, 4, -1, 0, 0));
    assert_eq!(timeline.metadata_for_protocol_id(4), None);
    timeline.begin_session(8);
    assert!(timeline.active.iter().all(Option::is_none));
    timeline.apply(7, 10, event(ActorEffectAction::Add, 26, 0, -1, 0));
    assert_eq!(timeline.diagnostics().stale_or_wrong_session, 1);
    timeline.apply(8, 1, event(ActorEffectAction::Add, 26, -1, -1, 0));
    assert_retained(&timeline, 26, -1, (1, 0, None));
}

#[test]
fn zero_finite_and_infinite_durations_do_not_use_packet_ticks() {
    for id in MINING_IDS {
        let mut timeline = LocalMovementEffectTimeline::default();
        timeline.begin_session(1);
        timeline.apply(1, 1, event(ActorEffectAction::Add, id, 0, 0, u64::MAX));
        assert_eq!(timeline.metadata_for_protocol_id(id), None);
        timeline.apply(1, 2, event(ActorEffectAction::Update, id, -1, 1, 0));
        assert_retained(&timeline, id, -1, (2, 0, Some(1)));
        timeline.commit_successful_tick();
        assert_eq!(timeline.metadata_for_protocol_id(id), None);
        timeline.apply(
            1,
            3,
            event(ActorEffectAction::Add, id, i32::MAX, i32::MIN, u64::MAX),
        );
        for _ in 0..20 {
            timeline.commit_successful_tick();
        }
        assert_retained(&timeline, id, i32::MAX, (3, u64::MAX, None));
    }
}

fn sample_bits(sample: &PhysicsMovementSample) -> Vec<u32> {
    sample
        .position
        .into_iter()
        .chain(sample.velocity)
        .chain(sample.move_vector)
        .chain(sample.raw_move_vector)
        .chain(sample.analogue_move_vector)
        .chain([sample.pitch, sample.yaw, sample.head_yaw])
        .chain(sample.camera_orientation)
        .map(f32::to_bits)
        .collect()
}

fn assert_frames_equal(left: &LocalPhysicsFrame, right: &LocalPhysicsFrame) {
    assert_eq!(left.due_ticks, right.due_ticks);
    assert_eq!(left.completed_ticks, right.completed_ticks);
    assert_eq!(left.dropped_ticks, right.dropped_ticks);
    assert_eq!(left.blocked_tick_index, right.blocked_tick_index);
    assert_eq!(left.blocked, right.blocked);
    assert_eq!(left.samples, right.samples);
    for (left, right) in left.samples.iter().zip(&right.samples) {
        assert_eq!(sample_bits(left), sample_bits(right));
    }
}

struct PairedPrediction {
    baseline: LocalPhysicsController,
    mixed: LocalPhysicsController,
    vertical: LocalMovementEffectTimeline,
    passive: LocalMovementEffectTimeline,
}

impl PairedPrediction {
    fn new() -> Self {
        let mut vertical = LocalMovementEffectTimeline::default();
        let mut passive = LocalMovementEffectTimeline::default();
        vertical.begin_session(1);
        passive.begin_session(1);
        for (sequence, id, amplifier) in [(1, 8, 1), (5, 24, -2), (9, 27, 0)] {
            let value = event(ActorEffectAction::Add, id, amplifier, 50, 9);
            vertical.apply(1, sequence, value);
            passive.apply(1, sequence, value);
            let (mining_id, mining_amplifier) = match id {
                8 => (3, i32::MIN),
                24 => (4, i32::MAX),
                _ => (26, -2),
            };
            passive.apply(
                1,
                sequence + 1,
                event(
                    ActorEffectAction::Add,
                    mining_id,
                    mining_amplifier,
                    12,
                    u64::MAX,
                ),
            );
        }
        Self {
            baseline: LocalPhysicsController::default(),
            mixed: LocalPhysicsController::default(),
            vertical,
            passive,
        }
    }

    fn anchor(&mut self, tick: u64) {
        self.baseline
            .reanchor_network_position([0.0, 5.0, 0.0], tick, false);
        self.mixed
            .reanchor_network_position([0.0, 5.0, 0.0], tick, false);
    }

    fn advance(&mut self, duration: Duration, world: &impl CollisionWorld) -> LocalPhysicsFrame {
        let input = MovementInput {
            forward: 0.7,
            strafe: -0.45,
            sneaking: true,
            move_vector_is_raw: true,
            ..MovementInput::default()
        };
        let left = self.baseline.advance_with_context_and_effects(
            duration,
            input,
            PhysicsSampleContext::default(),
            world,
            &mut self.vertical,
        );
        let right = self.mixed.advance_with_context_and_effects(
            duration,
            input,
            PhysicsSampleContext::default(),
            world,
            &mut self.passive,
        );
        assert_frames_equal(&left, &right);
        assert_eq!(self.baseline.state(), self.mixed.state());
        assert_eq!(self.baseline.history_len(), self.mixed.history_len());
        assert_eq!(
            self.baseline.dropped_tick_count(),
            self.mixed.dropped_tick_count()
        );
        assert_eq!(
            self.baseline
                .render_eye_position()
                .map(|v| v.map(f32::to_bits)),
            self.mixed
                .render_eye_position()
                .map(|v| v.map(f32::to_bits))
        );
        assert_eq!(self.vertical.snapshot(), self.passive.snapshot());
        right
    }

    fn remaining(&self, ticks: Option<u32>) {
        for id in MINING_IDS {
            assert_eq!(
                self.passive
                    .metadata_for_protocol_id(id)
                    .map(|value| value.2),
                ticks.map(Some),
                "effect {id}"
            );
        }
    }
}

#[test]
fn passive_retention_preserves_frames_at_success_failure_drop_replay_and_reanchor_boundaries() {
    let mut pair = PairedPrediction::new();
    assert_eq!(
        pair.advance(Duration::from_millis(50), &EmptyWorld)
            .completed_ticks,
        0
    );
    pair.remaining(Some(12));
    pair.anchor(100);
    assert_eq!(pair.advance(Duration::ZERO, &EmptyWorld).completed_ticks, 0);
    pair.remaining(Some(12));
    let blocked = pair.advance(Duration::from_millis(50), &UnavailableWorld);
    assert_eq!(blocked.completed_ticks, 0);
    assert!(blocked.blocked.is_some());
    pair.remaining(Some(12));
    assert_eq!(
        pair.advance(Duration::from_millis(50), &EmptyWorld)
            .completed_ticks,
        1
    );
    pair.remaining(Some(11));
    let catchup = pair.advance(Duration::from_secs(1), &EmptyWorld);
    assert_eq!(catchup.completed_ticks, 8);
    assert!(catchup.dropped_ticks > 0);
    pair.remaining(Some(3));

    let corrected = &catchup.samples[0];
    let left = pair
        .baseline
        .apply_correction(
            crate::movement::PhysicsAnchor {
                network_position: corrected.position,
                tick: corrected.tick,
                on_ground: false,
                velocity: None,
            },
            PhysicsCorrectionMode::ReplayIfRetained,
            None,
            &EmptyWorld,
        )
        .unwrap();
    let right = pair
        .mixed
        .apply_correction(
            crate::movement::PhysicsAnchor {
                network_position: corrected.position,
                tick: corrected.tick,
                on_ground: false,
                velocity: None,
            },
            PhysicsCorrectionMode::ReplayIfRetained,
            None,
            &EmptyWorld,
        )
        .unwrap();
    assert_eq!(left.outcome, right.outcome);
    assert_eq!(left.corrected_tick, right.corrected_tick);
    assert_eq!(left.final_tick, right.final_tick);
    assert_eq!(
        left.final_position.map(f32::to_bits),
        right.final_position.map(f32::to_bits)
    );
    assert_eq!(left.replayed_samples, right.replayed_samples);
    for (left, right) in left.replayed_samples.iter().zip(&right.replayed_samples) {
        assert_eq!(sample_bits(left), sample_bits(right));
    }
    assert!(matches!(
        right.outcome,
        PhysicsCorrectionOutcome::Replayed {
            replayed_ticks: 7,
            ..
        }
    ));
    assert_eq!(pair.baseline.state(), pair.mixed.state());
    pair.remaining(Some(3));

    pair.anchor(300);
    pair.remaining(Some(3));
    pair.baseline
        .reanchor_network_position_before_advance([0.0, 5.0, 0.0], 400, false);
    pair.mixed
        .reanchor_network_position_before_advance([0.0, 5.0, 0.0], 400, false);
    assert_eq!(
        pair.advance(Duration::from_millis(50), &EmptyWorld)
            .completed_ticks,
        0
    );
    pair.remaining(Some(3));
    assert_eq!(
        pair.advance(Duration::from_millis(100), &EmptyWorld)
            .completed_ticks,
        2
    );
    pair.remaining(Some(1));
    assert_eq!(
        pair.advance(Duration::from_millis(50), &EmptyWorld)
            .completed_ticks,
        1
    );
    pair.remaining(None);
}
