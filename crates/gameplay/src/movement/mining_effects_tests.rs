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
                    14,
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
    pair.remaining(Some(14));
    pair.anchor(100);
    assert_eq!(pair.advance(Duration::ZERO, &EmptyWorld).completed_ticks, 0);
    pair.remaining(Some(14));
    let blocked = pair.advance(Duration::from_millis(50), &UnavailableWorld);
    assert_eq!(blocked.completed_ticks, 0);
    assert!(blocked.blocked.is_some());
    pair.remaining(Some(14));
    assert_eq!(
        pair.advance(Duration::from_millis(50), &EmptyWorld)
            .completed_ticks,
        1
    );
    pair.remaining(Some(13));
    let catchup = pair.advance(Duration::from_secs(1), &EmptyWorld);
    assert_eq!(
        catchup.completed_ticks,
        crate::movement::MAX_LOCAL_PHYSICS_TICKS_PER_FRAME
    );
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
            replayed_ticks: 9,
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

/// Effect expiry within a catch-up frame uses that tick's denominator rather than the final one.
#[test]
fn batched_swing_progress_preserves_pre_and_post_expiry_effects() {
    use crate::melee::{SwingTracker, java_swing_duration};
    let mut effects = LocalMovementEffectTimeline::default();
    effects.begin_session(1);
    effects.apply(
        1,
        1,
        protocol::ActorEffectEvent {
            dimension: 0,
            actor_runtime_id: 1,
            action: protocol::ActorEffectAction::Add,
            effect_id: 3,
            amplifier: 1,
            particles: false,
            ambient: false,
            duration_ticks: 6,
            tick: 0,
        },
    );
    effects.begin_frame();
    for _ in 1..=8 {
        effects.commit_successful_tick();
    }
    let mut swings = SwingTracker::default();
    swings.sync_ticks((1, 1), 8, &effects);
    assert!(swings.try_swing(1, 4));
    assert_eq!(swings.published_progress(8).java, [0.0; 2]);
    assert!(
        swings.try_swing(9, 6),
        "the swing completed before Haste expired"
    );
    let (before, after) = effects.mining_tick(6, 8);
    assert_eq!(java_swing_duration(before), 4);
    assert_eq!(java_swing_duration(after), 6);
}

/// Conduit modifies Bedrock wire admission while Java receives independently guarded attempts.
#[test]
fn conduit_swing_attempts_keep_java_and_bedrock_counters_independent() {
    use crate::melee::SwingTracker;
    let mut effects = LocalMovementEffectTimeline::default();
    effects.begin_session(1);
    effects.apply(
        1,
        1,
        protocol::ActorEffectEvent {
            dimension: 0,
            actor_runtime_id: 1,
            action: protocol::ActorEffectAction::Add,
            effect_id: 26,
            amplifier: 1,
            particles: false,
            ambient: false,
            duration_ticks: -1,
            tick: 0,
        },
    );
    effects.begin_frame();
    for _ in 1..=8 {
        effects.commit_successful_tick();
    }
    let mut swings = SwingTracker::default();
    swings.sync_ticks((1, 1), 8, &effects);
    let accepted = (1..=8)
        .filter(|tick| swings.try_swing(*tick, 4))
        .collect::<Vec<_>>();
    assert_eq!(accepted, [1, 4, 7]);
    let progress = swings.published_progress(8);
    assert_eq!(progress.bedrock, [0.0, 0.25]);
    assert_eq!(progress.java, [2.0 / 6.0, 0.5]);
}

/// Publication keeps the expired tick's denominator even after a zero-tick frame clears history.
#[test]
fn a_backpressured_expiry_tick_retains_its_post_expiry_denominator() {
    use crate::melee::{SwingTracker, swing_duration};
    let mut effects = crate::movement::LocalMovementEffectTimeline::default();
    effects.begin_session(1);
    for (sequence, effect_id, amplifier, duration_ticks) in [(1, 3, 1, 1), (2, 4, 0, -1)] {
        effects.apply(
            1,
            sequence,
            protocol::ActorEffectEvent {
                dimension: 0,
                actor_runtime_id: 1,
                action: protocol::ActorEffectAction::Add,
                effect_id,
                amplifier,
                particles: false,
                ambient: false,
                duration_ticks,
                tick: 0,
            },
        );
    }
    effects.begin_frame();
    effects.commit_successful_tick();
    let (before, after) = effects.mining_tick(100, 100);
    let before_duration = swing_duration(before);
    let after_duration = swing_duration(after);
    assert_eq!((before_duration, after_duration), (4, 8));
    let mut swings = SwingTracker::default();
    swings.sync_ticks((1, 1), 100, &effects);
    let mut candidate = swings.clone();
    assert!(candidate.try_swing(100, before_duration));
    swings.defer_unadmitted_attempt(&candidate);
    let idle = swings.published_progress(100);
    assert_eq!(idle.java, [0.0; 2]);
    let mut rejected_again = swings.clone();
    assert!(rejected_again.try_swing(100, before_duration));
    swings.defer_unadmitted_attempt(&rejected_again);
    assert_eq!(swings.published_progress(100), idle);
    effects.begin_frame();
    swings.sync_ticks((1, 1), 100, &effects);
    assert!(swings.try_swing(100, before_duration));
    assert_eq!(swings.take_started(), Some(before_duration));
    let admitted = swings.published_progress(100);
    assert_eq!(swings.published_progress(100), admitted);
    let next = swings.published_progress(101);
    let expected = [0.0, 1.0 / after_duration as f32];
    assert_eq!(next.bedrock, expected);
    assert_eq!(next.java, expected);
}

/// Retried Conduit admission keeps Java's independent guard after the effect has expired.
#[test]
fn a_backpressured_conduit_restart_preserves_each_modes_admission() {
    use crate::melee::SwingTracker;
    let mut effects = crate::movement::LocalMovementEffectTimeline::default();
    effects.begin_session(1);
    effects.apply(
        1,
        1,
        protocol::ActorEffectEvent {
            dimension: 0,
            actor_runtime_id: 1,
            action: protocol::ActorEffectAction::Add,
            effect_id: 26,
            amplifier: 1,
            particles: false,
            ambient: false,
            duration_ticks: 4,
            tick: 0,
        },
    );
    effects.begin_frame();
    effects.commit_successful_tick();
    let mut swings = SwingTracker::default();
    swings.sync_ticks((1, 1), 100, &effects);
    assert!(swings.try_swing(100, 4));
    swings.published_progress(100);
    effects.begin_frame();
    for _ in 101..=103 {
        effects.commit_successful_tick();
    }
    swings.sync_ticks((1, 1), 103, &effects);
    swings.published_progress(102);
    let mut candidate = swings.clone();
    assert!(candidate.try_swing(103, 4));
    swings.defer_unadmitted_attempt(&candidate);
    swings.published_progress(103);
    let mut rejected_again = swings.clone();
    assert!(rejected_again.try_swing(103, 4));
    swings.defer_unadmitted_attempt(&rejected_again);
    effects.begin_frame();
    swings.sync_ticks((1, 1), 103, &effects);
    assert!(swings.try_swing(103, 4));
    assert_eq!(swings.take_started(), Some(4));
    let recovered = swings.published_progress(103);
    assert_eq!(recovered.bedrock, [0.5, 0.0]);
    assert_eq!(
        recovered.java,
        [2.0 / 6.0, 0.5],
        "Java did not admit this native restart"
    );
    assert_eq!(swings.published_progress(103), recovered);
    let next = swings.published_progress(104);
    assert_eq!(next.bedrock, [0.0, 1.0 / 6.0]);
    assert_eq!(next.java, [0.5, 4.0 / 6.0]);
}

/// Initial idle publication retains every committed tick before the first attack or after reset.
#[test]
fn committed_swing_samples_preserve_initial_idle_catchup_after_authority_reset() {
    use crate::melee::SwingTracker;
    let mut effects = crate::movement::LocalMovementEffectTimeline::default();
    effects.begin_session(1);
    let mut swings = SwingTracker::default();
    for (authority, first) in [(1, 101), (2, 201)] {
        effects.begin_frame();
        for _ in 0..3 {
            effects.commit_successful_tick();
        }
        swings.sync_ticks((1, authority), first + 2, &effects);
        swings.published_progress(first + 2);
        let samples = swings.committed_samples().collect::<Vec<_>>();
        assert_eq!(
            samples.iter().map(|(tick, _)| *tick).collect::<Vec<_>>(),
            vec![first, first + 1, first + 2]
        );
        assert!(
            samples
                .iter()
                .all(|(_, progress)| *progress == client_world::LocalSwingProgress::default())
        );
    }
}

/// The largest retained tick still increments its admitted swing exactly once.
#[test]
fn committed_swing_samples_advance_the_maximum_tick_once() {
    use crate::melee::SwingTracker;
    let mut effects = crate::movement::LocalMovementEffectTimeline::default();
    effects.begin_session(1);
    effects.begin_frame();
    effects.commit_successful_tick();
    let mut swings = SwingTracker::default();
    swings.sync_ticks((1, 1), u64::MAX, &effects);
    assert!(swings.try_swing(u64::MAX, 6));
    let progress = swings.published_progress(u64::MAX);
    assert_eq!(progress.bedrock, [0.0; 2]);
    assert_eq!(progress.java, [0.0; 2]);
    assert_eq!(
        swings.committed_samples().last(),
        Some((u64::MAX, progress))
    );
    assert_eq!(swings.published_progress(u64::MAX), progress);
}
