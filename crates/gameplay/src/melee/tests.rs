use protocol_ids::ActorDataKey;
use simulation_clock::identity::{PositionAuthorityGeneration, SessionGeneration};
use std::{collections::HashMap, sync::Arc};

use client_world::{ActorSnapshot, WorldAuthority};
use protocol::test_support::transactions::observe_transaction;
use protocol::{ActorKind, ActorMetadataValue};

use super::*;

/// Spawns a pickable actor with optional collision dimensions.
pub(super) fn actor(
    runtime_id: u64,
    identifier: &str,
    feet: [f32; 3],
    size: Option<(f32, f32)>,
) -> ActorSnapshot {
    let metadata = size
        .map(|(width, height)| {
            HashMap::from([
                (
                    ActorDataKey::Width.wire_value(),
                    ActorMetadataValue::Float(width),
                ),
                (
                    ActorDataKey::Height.wire_value(),
                    ActorMetadataValue::Float(height),
                ),
            ])
        })
        .unwrap_or_default();
    actor_with(runtime_id, identifier, feet, metadata)
}

/// Spawns an actor with arbitrary metadata for picking tests.
fn actor_with(
    runtime_id: u64,
    identifier: &str,
    feet: [f32; 3],
    metadata: HashMap<u32, ActorMetadataValue>,
) -> ActorSnapshot {
    actor_with_base(runtime_id, identifier, Some(""), feet, metadata)
}

/// Spawns an actor after an optional advertised base has been admitted.
fn actor_with_base(
    runtime_id: u64,
    identifier: &str,
    base: Option<&str>,
    feet: [f32; 3],
    metadata: HashMap<u32, ActorMetadataValue>,
) -> ActorSnapshot {
    let mut authority = WorldAuthority::new(
        protocol::WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: 0,
            local_player_unique_id: 0,
            player_position: feet,
            world_spawn_position: [0; 3],
            air_network_id: protocol::air_network_id(false),
            block_network_ids_are_hashes: false,
        },
        Arc::new(assets::RuntimeAssets::diagnostic()),
        None,
        feet,
        None,
    );
    if let Some(base) = base {
        authority
            .apply_ordered_event(
                protocol::WorldEvent::Actor(protocol::ActorEvent::Identifiers(
                    protocol::ActorIdentifierRegistry {
                        entries: Arc::from([protocol::ActorIdentifier {
                            identifier: identifier.into(),
                            base_identifier: base.into(),
                        }]),
                        skipped: 0,
                    },
                )),
                Some(1),
            )
            .unwrap();
    }
    let spawn = protocol::ActorSpawnEvent {
        dimension: 0,
        unique_id: runtime_id as i64 * 10,
        runtime_id,
        kind: ActorKind::Entity {
            identifier: identifier.into(),
        },
        position: feet,
        velocity: [0.0; 3],
        pitch: 0.0,
        yaw: 0.0,
        head_yaw: 0.0,
        body_yaw: 0.0,
        held_item: Default::default(),
        metadata: metadata
            .into_iter()
            .map(|(key, value)| protocol::ActorMetadata { key, value })
            .collect(),
        attributes: Arc::from([]),
        properties: Arc::from([]),
        links: Arc::from([]),
    };
    authority
        .apply_ordered_event(
            protocol::WorldEvent::Actor(protocol::ActorEvent::Spawn(spawn)),
            Some(2),
        )
        .unwrap();
    let mut actor = authority
        .actor(runtime_id)
        .expect("spawn committed")
        .clone();
    actor.spawn_revision = 0;
    actor
}

const EYE: [f32; 3] = [0.0, 1.62, 0.0];
const NORTH: [f32; 3] = [0.0, 0.0, -1.0];

#[test]
fn a_new_swing_waits_for_half_the_current_one() {
    let mut swings = SwingTracker::default();
    assert_eq!(swings.take_started(), None);
    assert!(swings.try_swing(10, 6));
    assert_eq!(
        swings.take_started(),
        Some(6),
        "an accepted swing is handed to the local rig once, with its duration"
    );
    assert_eq!(swings.take_started(), None);
    assert!(!swings.try_swing(10, 6));
    assert!(!swings.try_swing(12, 6));
    assert!(!swings.try_swing(13, 6));
    assert!(swings.try_swing(14, 6));
    // Reanchored tick numbers never lock swinging out.
    assert!(swings.try_swing(2, 6));
}

#[test]
fn haste_shortens_and_fatigue_lengthens_the_swing() {
    let effects = |haste, fatigue| MiningEffects {
        haste,
        mining_fatigue: fatigue,
        conduit_power: None,
    };
    assert_eq!(swing_duration(effects(None, None)), 6);
    assert_eq!(swing_duration(effects(Some(0), None)), 5);
    assert_eq!(swing_duration(effects(None, Some(1))), 10);
    assert_eq!(swing_duration(effects(Some(1), Some(1))), 4);
    assert_eq!(swing_duration(effects(Some(40), None)), 1);
}

/// An extreme server amplifier must not overflow the swing arithmetic.
#[test]
fn maximal_effect_amplifiers_saturate() {
    let effects = |haste, fatigue| MiningEffects {
        haste,
        mining_fatigue: fatigue,
        conduit_power: None,
    };
    assert_eq!(swing_duration(effects(Some(i32::MAX), None)), 1);
    assert_eq!(swing_duration(effects(None, Some(i32::MAX))), i32::MAX);
}

/// Supplies an admitted empty-hand attack with a stable tick and player position.
pub(super) fn press(input_mode: PlayerInputMode) -> PressContext {
    let stack = protocol::NetworkItemStack::empty();
    PressContext {
        refused_destroy: None,
        tick: 101,
        player_position: [0.5, 2.620_01, 0.5],
        input_mode,
        local_runtime_id: 42,
        selection: Some(crate::mining::FrozenMiningSelection {
            slot: 3,
            item: protocol::VerifiedNetworkItemStack::try_new(stack.clone(), stack.nbt_digest)
                .unwrap(),
        }),
        swing_duration: 6,
        item_attack: None,
        now_millis: 1_000,
    }
}

fn kinds(packets: &[protocol::Packet]) -> Vec<String> {
    packets
        .iter()
        .map(|packet| format!("{:?}", packet.kind()))
        .collect()
}

const ZOMBIE: Crosshair = Crosshair::Actor(ActorHit {
    runtime_id: 9,
    distance: 2.0,
    point: [0.0, 1.5, -2.0],
});

#[test]
fn one_press_attacks_once_before_its_swing_and_held_frames_do_nothing() {
    let mut runtime = MeleeRuntime::default();
    let mut swings = SwingTracker::default();
    assert!(runtime.observe_input(true, true));
    let outcome = runtime.resolve(ZOMBIE, &press(PlayerInputMode::Mouse), &mut swings);
    assert_eq!(
        kinds(&outcome.packets),
        ["InventoryTransactionPacket", "AnimatePacket"]
    );
    assert!(!outcome.missed_swing);
    assert!(runtime.actor_in_front());
    assert!(runtime.blocks_use_at(1_199) && !runtime.blocks_use_at(1_200));
    for _ in 0..5 {
        assert!(runtime.observe_input(false, true));
        let held = runtime.resolve(ZOMBIE, &press(PlayerInputMode::Mouse), &mut swings);
        assert!(held.packets.is_empty() && !held.missed_swing);
    }
}

#[test]
fn misses_flag_the_tick_and_only_non_touch_misses_swing() {
    for (mode, swing) in [
        (PlayerInputMode::Mouse, true),
        (PlayerInputMode::Touch, false),
    ] {
        let mut runtime = MeleeRuntime::default();
        runtime.observe_input(true, false);
        let outcome = runtime.resolve(Crosshair::Miss, &press(mode), &mut SwingTracker::default());
        assert!(outcome.missed_swing, "{mode:?}");
        assert_eq!(!outcome.packets.is_empty(), swing, "{mode:?}");
    }
    let mut runtime = MeleeRuntime::default();
    runtime.observe_input(true, false);
    let block = runtime.resolve(
        Crosshair::Block,
        &press(PlayerInputMode::Mouse),
        &mut SwingTracker::default(),
    );
    assert_eq!(kinds(&block.packets), ["AnimatePacket"]);
    assert!(!block.missed_swing && !runtime.actor_in_front());
}

#[test]
fn a_position_authority_change_drops_a_latched_press() {
    let mut runtime = MeleeRuntime::default();
    runtime.synchronize((
        SessionGeneration::new(7),
        PositionAuthorityGeneration::new(0),
    ));
    runtime.observe_input(true, false);
    runtime.synchronize((
        SessionGeneration::new(8),
        PositionAuthorityGeneration::new(0),
    ));
    let outcome = runtime.resolve(
        ZOMBIE,
        &press(PlayerInputMode::Mouse),
        &mut SwingTracker::default(),
    );
    assert!(outcome.packets.is_empty());
}

/// Players need no size metadata to be picked.
#[test]
fn a_sizeless_player_is_picked() {
    let mut player = actor(6, "", [0.0, 0.0, -2.0], None);
    player.kind = ActorKind::Player {
        uuid: [6; 16],
        username: "p".into(),
    };
    let hit = pick_actor([player].iter(), None, EYE, NORTH, 5.7).unwrap();
    assert_eq!(hit.runtime_id, 6);
}

/// A press waiting on unavailable block evidence survives briefly, then expires.
#[test]
fn a_press_deferred_on_unavailable_block_evidence_is_bounded() {
    let mut runtime = MeleeRuntime::default();
    runtime.observe_input(true, true);
    runtime.defer(10);
    runtime.defer(10 + MAX_PENDING_INTERACTION_MILLIS);
    let outcome = runtime.resolve(
        ZOMBIE,
        &press(PlayerInputMode::Mouse),
        &mut SwingTracker::default(),
    );
    assert_eq!(outcome.packets.len(), 2, "the deferred press still attacks");

    runtime.observe_input(true, true);
    runtime.defer(50);
    runtime.defer(51 + MAX_PENDING_INTERACTION_MILLIS);
    let outcome = runtime.resolve(
        ZOMBIE,
        &press(PlayerInputMode::Mouse),
        &mut SwingTracker::default(),
    );
    assert!(
        outcome.packets.is_empty(),
        "a press stale past the bound is dropped"
    );
}

/// Only an open screen or spectator mode stops the attack button; an unknown or not yet
/// received game mode still swings (Zeqa's StartGame carries survival, mode 0).
#[test]
fn only_a_screen_or_spectator_stops_the_attack_button() {
    use protocol::PlayerGameMode::{Adventure, Creative, Spectator, Survival, Unknown};
    for mode in [
        None,
        Some(Survival),
        Some(Creative),
        Some(Adventure),
        Some(Unknown),
    ] {
        assert_eq!(press_admission(true, mode), Ok(()), "{mode:?}");
    }
    assert_eq!(press_admission(true, Some(Spectator)), Err("spectator"));
    assert_eq!(press_admission(false, Some(Survival)), Err("screen_open"));
}

/// A press into the air swings and reports MissedSwing on its tick.
#[test]
fn an_air_press_swings_and_reports_a_missed_swing() {
    let mut runtime = MeleeRuntime::default();
    let mut swings = SwingTracker::default();
    runtime.observe_input(true, true);
    let outcome = runtime.resolve(Crosshair::Miss, &press(PlayerInputMode::Mouse), &mut swings);
    assert_eq!(kinds(&outcome.packets), ["AnimatePacket"]);
    assert!(outcome.missed_swing);
    assert!(swings.take_started().is_some());
}

/// Java 1.7 has no Conduit Power swing modifier, even when fatigue is active.
#[test]
fn java_swing_ignores_conduit_power() {
    assert_eq!(
        java_swing_duration(MiningEffects {
            conduit_power: Some(1),
            ..Default::default()
        }),
        6
    );
    assert_eq!(
        java_swing_duration(MiningEffects {
            conduit_power: Some(1),
            mining_fatigue: Some(0),
            ..Default::default()
        }),
        8
    );
}

/// Zero-based amplifiers produce the reference tick counts in both animation modes.
#[test]
fn swing_modes_match_no_effects_haste_ii_and_fatigue_i() {
    for (effects, duration) in [
        (MiningEffects::default(), 6),
        (
            MiningEffects {
                haste: Some(1),
                ..Default::default()
            },
            4,
        ),
        (
            MiningEffects {
                mining_fatigue: Some(0),
                ..Default::default()
            },
            8,
        ),
    ] {
        assert_eq!(swing_duration(effects), duration);
        assert_eq!(java_swing_duration(effects), duration);
    }
    assert_eq!(
        swing_duration(MiningEffects {
            conduit_power: Some(1),
            ..Default::default()
        }),
        4
    );
}

/// Held attempts arrive before the tick advances the counter from its initial -1.
#[test]
fn held_swing_admission_matches_the_native_counter_phase() {
    for (duration, interval) in [(6, 4), (4, 3), (8, 5)] {
        let mut swings = SwingTracker::default();
        let started = (100..120)
            .filter(|tick| swings.try_swing(*tick, duration))
            .collect::<Vec<_>>();
        let expected = (100..120).step_by(interval).collect::<Vec<_>>();
        assert_eq!(started, expected, "duration {duration}");
    }
}

/// Published progress follows each accepted held attempt even when all ticks arrive in one frame.
#[test]
fn local_swing_progress_is_exact_for_batched_held_ticks() {
    for (duration, interval) in [(6, 4), (4, 3), (8, 5)] {
        let mut batched = SwingTracker::default();
        let mut sequential = SwingTracker::default();
        let mut expected = actor_animation::LocalSwingProgress::default();
        for tick in 1..=8 {
            batched.try_swing(tick, duration);
            sequential.try_swing(tick, duration);
            expected = sequential.published_progress(tick);
        }
        assert_eq!(
            batched.published_progress(8),
            expected,
            "duration {duration}, interval {interval}"
        );
        assert_eq!(
            batched.published_progress(8),
            expected,
            "unchanged tick retains samples"
        );
    }
}

/// A completed Haste swing cannot become active again when its duration grows.
#[test]
fn a_completed_short_swing_admits_after_effect_expiry() {
    let mut swings = SwingTracker::default();
    assert!(swings.try_swing(1, 4));
    assert_eq!(swings.published_progress(5).java[1], 0.0);
    assert!(swings.try_swing(6, 6));
    assert_eq!(swings.published_progress(6).java, [0.0; 2]);
}

/// A correction changes the local clock identity, even if its tick number advances.
#[test]
fn movement_authority_change_resets_local_swing_samples() {
    let effects = crate::movement::LocalMovementEffectTimeline::default();
    let mut swings = SwingTracker::default();
    swings.sync_ticks(
        (
            SessionGeneration::new(1),
            PositionAuthorityGeneration::new(1),
        ),
        10,
        &effects,
    );
    assert!(swings.try_swing(10, 6));
    swings.published_progress(12);
    swings.sync_ticks(
        (
            SessionGeneration::new(1),
            PositionAuthorityGeneration::new(2),
        ),
        2,
        &effects,
    );
    assert!(swings.try_swing(2, 6));
    assert_eq!(swings.published_progress(2).java, [0.0; 2]);
    swings.sync_ticks(
        (
            SessionGeneration::new(1),
            PositionAuthorityGeneration::new(3),
        ),
        20,
        &effects,
    );
    assert!(swings.try_swing(20, 6));
    assert_eq!(swings.published_progress(20).bedrock, [0.0; 2]);
}

/// A block press starts on the first catch-up tick before held attempts continue in order.
#[test]
fn pressed_block_swing_keeps_held_catchup_ticks() {
    let movement = crate::test_support::survival_mining::ticker_with_ticks(5);
    let first = movement.first_unsent_sample_in_frame(5).unwrap();
    let mut runtime = MeleeRuntime::default();
    let mut swings = SwingTracker::default();
    swings.sync_ticks(
        movement.interaction_authority_identity(),
        movement.completed_tick(),
        &crate::movement::LocalMovementEffectTimeline::default(),
    );
    runtime.observe_input(true, true);
    runtime.resolve(
        Crosshair::Block,
        &PressContext {
            tick: first.tick,
            ..press(PlayerInputMode::Mouse)
        },
        &mut swings,
    );
    for tick in 101..=105 {
        swings.try_swing(tick, 6);
    }
    assert_eq!(swings.published_progress(105).java, [0.5, 0.0]);
    assert_eq!(first.tick, 101);
    assert!(movement.first_unsent_sample_in_frame(0).is_none());
    assert_eq!(movement.first_unsent_sample_in_frame(2).unwrap().tick, 104);
    assert_eq!(movement.newest_unsent_sample().unwrap().tick, 105);
}

/// Each published simulation sample follows the native counter for the three reported cases.
#[test]
fn local_published_swing_matches_native_tick_samples() {
    for (duration, samples) in [
        (
            6,
            vec![
                0.0,
                1.0 / 6.0,
                2.0 / 6.0,
                0.5,
                4.0 / 6.0,
                5.0 / 6.0,
                0.0,
                0.0,
            ],
        ),
        (4, vec![0.0, 0.25, 0.5, 0.75, 0.0, 0.0]),
        (
            8,
            vec![0.0, 0.125, 0.25, 0.375, 0.5, 0.625, 0.75, 0.875, 0.0, 0.0],
        ),
    ] {
        let mut swings = SwingTracker::default();
        assert!(swings.try_swing(100, duration));
        let mut previous = 0.0;
        for (offset, expected) in samples.into_iter().enumerate() {
            let progress = swings.published_progress(100 + offset as u64);
            assert_eq!(
                progress.bedrock,
                [previous, expected],
                "duration {duration}, offset {offset}"
            );
            assert_eq!(
                progress.java,
                [previous, expected],
                "duration {duration}, offset {offset}"
            );
            previous = expected;
        }
    }
}

/// A failed batch retries against its original tick after that tick was published.
fn assert_published_press_retry(crosshair: Crosshair, duration: i32) {
    let effects = crate::movement::LocalMovementEffectTimeline::default();
    let mut runtime = MeleeRuntime::default();
    let mut swings = SwingTracker::default();
    let mut press = press(PlayerInputMode::Mouse);
    press.swing_duration = duration;
    runtime.observe_input(true, true);
    swings.sync_ticks(
        (
            SessionGeneration::new(1),
            PositionAuthorityGeneration::new(1),
        ),
        press.tick,
        &effects,
    );
    let mut initial_packets = Vec::new();
    assert!(!resolve_and_send(
        &mut runtime,
        &mut swings,
        crosshair,
        &press,
        1,
        |packets| {
            initial_packets = kinds(&packets);
            Err(BatchSendError::Full)
        },
    ));
    assert_eq!(swings.take_started(), None);
    assert_eq!(
        runtime.admitted_attack(),
        None,
        "a refused FIFO admits no combat evidence"
    );
    assert_eq!(swings.published_progress(press.tick).bedrock, [0.0; 2]);
    swings.sync_ticks(
        (
            SessionGeneration::new(1),
            PositionAuthorityGeneration::new(1),
        ),
        press.tick,
        &effects,
    );
    let mut admitted_packets = Vec::new();
    let missed = resolve_and_send(&mut runtime, &mut swings, crosshair, &press, 2, |packets| {
        admitted_packets = kinds(&packets);
        assert_eq!(
            admitted_packets, initial_packets,
            "a recovered send retains its original packet order"
        );
        if let Crosshair::Actor(hit) = crosshair {
            let observed =
                observe_transaction(&packets[0]).expect("the actor transaction precedes its swing");
            let transaction = observed
                .actor_use()
                .expect("the original actor attack survives backpressure");
            assert_eq!(transaction.runtime_id, hit.runtime_id);
            assert_eq!(transaction.from_position, press.player_position);
        }
        Ok(())
    });
    assert_eq!(
        admitted_packets, initial_packets,
        "the retry keeps its admitted swing and packet order"
    );
    if let Crosshair::Actor(hit) = crosshair {
        assert_eq!(
            runtime.admitted_attack().map(|attack| attack.hit),
            Some(hit)
        );
    } else {
        assert_eq!(runtime.admitted_attack(), None);
    }
    assert_eq!(missed, crosshair == Crosshair::Miss);
    assert_eq!(swings.take_started(), Some(press.swing_duration));
    assert!(
        !swings.try_swing(press.tick, press.swing_duration),
        "one tick cannot admit twice"
    );
    assert_eq!(swings.published_progress(press.tick).java, [0.0; 2]);
    let next = swings.published_progress(press.tick + 1);
    let expected = [0.0, 1.0 / press.swing_duration as f32];
    assert_eq!(
        next.bedrock, expected,
        "the original tick starts the recovered swing"
    );
    assert_eq!(next.java, expected);
    let held = runtime.resolve(crosshair, &press, &mut swings);
    assert!(
        held.packets.is_empty(),
        "the recovered press is consumed exactly once"
    );
}

#[test]
fn a_backpressured_miss_retries_after_its_tick_was_published() {
    for duration in [6, 4, 8] {
        assert_published_press_retry(Crosshair::Miss, duration);
    }
}

#[test]
fn a_backpressured_actor_press_retries_after_its_tick_was_published() {
    for duration in [6, 4, 8] {
        assert_published_press_retry(ZOMBIE, duration);
    }
}

/// A recovered restart replaces only the final tick, preserving its interpolation predecessor.
#[test]
fn a_backpressured_restart_keeps_the_published_tick_and_previous_sample() {
    let mut swings = SwingTracker::default();
    assert!(swings.try_swing(100, 6));
    assert_eq!(swings.published_progress(103).java, [2.0 / 6.0, 0.5]);
    let mut candidate = swings.clone();
    assert!(candidate.try_swing(104, 6));
    swings.defer_unadmitted_attempt(&candidate);
    assert_eq!(swings.published_progress(104).java, [0.5, 4.0 / 6.0]);
    assert!(swings.try_swing(104, 6));
    let recovered = swings.published_progress(104);
    assert_eq!(recovered.bedrock, [0.5, 0.0]);
    assert_eq!(recovered.java, [0.5, 0.0]);
    assert!(!swings.try_swing(104, 6));
    assert_eq!(swings.published_progress(104), recovered);
    assert_eq!(swings.published_progress(105).java, [0.0, 1.0 / 6.0]);
}

/// Retry permission never admits a duplicate, an older tick, or a different movement authority.
#[test]
fn only_the_current_unadmitted_tick_can_replay() {
    let effects = crate::movement::LocalMovementEffectTimeline::default();
    let mut swings = SwingTracker::default();
    swings.sync_ticks(
        (
            SessionGeneration::new(1),
            PositionAuthorityGeneration::new(1),
        ),
        100,
        &effects,
    );
    let mut candidate = swings.clone();
    assert!(candidate.try_swing(100, 6));
    swings.defer_unadmitted_attempt(&candidate);
    swings.published_progress(101);
    assert!(
        !swings.try_swing(100, 6),
        "an older tick cannot rewrite published history"
    );
    assert!(
        !swings.try_swing(101, 6),
        "permission belongs only to the rejected attempt"
    );
    assert!(
        swings.try_swing(102, 6),
        "the caller can use its newest unsent tick"
    );
    swings.defer_unadmitted_attempt(&swings.clone());
    swings.published_progress(102);
    assert!(
        !swings.try_swing(102, 6),
        "an admitted attempt cannot acquire retry permission"
    );

    let mut other = SwingTracker::default();
    other.sync_ticks(
        (
            SessionGeneration::new(1),
            PositionAuthorityGeneration::new(2),
        ),
        103,
        &effects,
    );
    assert!(other.try_swing(103, 6));
    swings.defer_unadmitted_attempt(&other);
    swings.published_progress(103);
    assert!(
        !swings.try_swing(103, 6),
        "a different authority cannot reopen a tick"
    );
    swings.sync_ticks(
        (
            SessionGeneration::new(1),
            PositionAuthorityGeneration::new(2),
        ),
        103,
        &effects,
    );
    assert!(
        swings.try_swing(103, 6),
        "new authority resets both counters and permission"
    );
}

/// Catch-up publication retains the progress each body-motion tick must consume.
#[test]
fn committed_swing_samples_preserve_each_catchup_tick_for_both_modes() {
    let last = crate::movement::MAX_LOCAL_PHYSICS_TICKS_PER_FRAME as u64;
    for duration in [6, 4, 8] {
        let mut batched = SwingTracker::default();
        let mut sequential = SwingTracker::default();
        let mut expected = Vec::new();
        for tick in 1..=last {
            batched.try_swing(tick, duration);
            sequential.try_swing(tick, duration);
            expected.push((tick, sequential.published_progress(tick)));
        }
        batched.published_progress(last);
        assert_eq!(batched.committed_samples().collect::<Vec<_>>(), expected);
    }
}

/// A same-tick retry replaces that tick's motion sample instead of appending a duplicate.
#[test]
fn committed_swing_samples_replace_a_recovered_published_restart() {
    let mut swings = SwingTracker::default();
    assert!(swings.try_swing(100, 6));
    swings.published_progress(103);
    let mut candidate = swings.clone();
    assert!(candidate.try_swing(104, 6));
    swings.defer_unadmitted_attempt(&candidate);
    swings.published_progress(104);
    assert!(swings.try_swing(104, 6));
    let recovered = swings.published_progress(104);
    let samples = swings.committed_samples().collect::<Vec<_>>();
    assert_eq!(samples.last(), Some(&(104, recovered)));
    assert_eq!(samples.iter().filter(|(tick, _)| *tick == 104).count(), 1);
    assert_eq!(
        samples
            .iter()
            .find(|(tick, _)| *tick == 103)
            .unwrap()
            .1
            .java[1],
        0.5
    );
}

/// Old progress is bounded and cannot survive a movement authority reset.
#[test]
fn committed_swing_samples_are_bounded_across_large_tick_jumps_and_authority_resets() {
    use crate::movement::MAX_LOCAL_PHYSICS_TICKS_PER_FRAME;
    use actor_animation::LocalSwingProgress;
    let mut effects = crate::movement::LocalMovementEffectTimeline::default();
    effects.begin_session(1);
    let mut swings = SwingTracker::default();
    swings.sync_ticks(
        (
            SessionGeneration::new(1),
            PositionAuthorityGeneration::new(1),
        ),
        1,
        &effects,
    );
    assert!(swings.try_swing(1, 6));
    swings.published_progress(1);
    swings.published_progress(u64::MAX);
    let samples = swings.committed_samples().collect::<Vec<_>>();
    assert_eq!(samples.len(), MAX_LOCAL_PHYSICS_TICKS_PER_FRAME);
    assert_eq!(samples.last().unwrap().0, u64::MAX);
    assert!(
        samples
            .iter()
            .all(|(_, sample)| *sample == LocalSwingProgress::default())
    );
    swings.published_progress(u64::MAX);
    assert_eq!(swings.committed_samples().collect::<Vec<_>>(), samples);
    effects.begin_session(1);
    swings.sync_ticks(
        (
            SessionGeneration::new(1),
            PositionAuthorityGeneration::new(2),
        ),
        2,
        &effects,
    );
    assert_eq!(swings.committed_samples().count(), 0);
    swings.published_progress(2);
    assert_eq!(
        swings.committed_samples().collect::<Vec<_>>(),
        vec![(2, LocalSwingProgress::default())]
    );
}

/// A new press waits for a new simulation tick even when another owner holds retry permission.
fn assert_fresh_published_press_waits(crosshair: Crosshair, foreign_retry: bool) {
    let mut runtime = MeleeRuntime::default();
    let mut swings = SwingTracker::default();
    let mut press = press(PlayerInputMode::Mouse);
    if foreign_retry {
        let mut candidate = swings.clone();
        assert!(candidate.try_swing(press.tick, press.swing_duration));
        swings.defer_unadmitted_attempt(&candidate);
    }
    swings.published_progress(press.tick);
    runtime.observe_input(true, false);
    let mut sends = 0;
    assert!(!resolve_and_send(
        &mut runtime,
        &mut swings,
        crosshair,
        &press,
        1,
        |_| {
            sends += 1;
            Ok(())
        },
    ));
    assert_eq!(
        sends, 0,
        "a fresh press must not consume an already-published tick"
    );
    assert!(
        runtime.observe_input(false, false),
        "the press stays latched"
    );
    assert_eq!(swings.take_started(), None);
    press.tick += 1;
    let missed = resolve_and_send(&mut runtime, &mut swings, crosshair, &press, 2, |packets| {
        assert_eq!(
            kinds(&packets),
            if crosshair == Crosshair::Miss {
                vec!["AnimatePacket"]
            } else {
                vec!["InventoryTransactionPacket", "AnimatePacket"]
            }
        );
        Ok(())
    });
    assert_eq!(missed, crosshair == Crosshair::Miss);
    assert!(!runtime.observe_input(false, false));
    assert_eq!(swings.take_started(), Some(press.swing_duration));
    swings.published_progress(press.tick);
    assert_eq!(
        swings.published_progress(press.tick + 1).java,
        [0.0, 1.0 / 6.0]
    );
}

#[test]
fn a_fresh_miss_waits_for_an_unpublished_tick() {
    assert_fresh_published_press_waits(Crosshair::Miss, false);
}

#[test]
fn a_fresh_actor_press_waits_for_an_unpublished_tick() {
    assert_fresh_published_press_waits(ZOMBIE, false);
}

#[test]
fn a_fresh_press_cannot_borrow_another_owners_swing_retry() {
    for crosshair in [Crosshair::Miss, ZOMBIE] {
        assert_fresh_published_press_waits(crosshair, true);
    }
}

#[test]
fn fresh_published_presses_expire_without_sending() {
    let mut runtime = MeleeRuntime::default();
    let mut swings = SwingTracker::default();
    let press = press(PlayerInputMode::Mouse);
    swings.published_progress(press.tick);
    runtime.observe_input(true, false);
    for frame in [1, 2 + MAX_PENDING_INTERACTION_MILLIS] {
        assert!(!resolve_and_send(
            &mut runtime,
            &mut swings,
            ZOMBIE,
            &press,
            frame,
            |_| {
                panic!("a published tick cannot submit a fresh attack");
            }
        ));
    }
    assert!(!runtime.observe_input(false, false));
}

#[test]
fn melee_retry_permission_clears_on_cancel_authority_change_and_expiry() {
    for reset in 0..3 {
        let mut runtime = MeleeRuntime::default();
        runtime.synchronize((
            SessionGeneration::new(1),
            PositionAuthorityGeneration::new(1),
        ));
        let mut swings = SwingTracker::default();
        let press = press(PlayerInputMode::Mouse);
        runtime.observe_input(true, false);
        resolve_and_send(
            &mut runtime,
            &mut swings,
            Crosshair::Miss,
            &press,
            1,
            |_| Err(BatchSendError::Full),
        );
        swings.published_progress(press.tick);
        match reset {
            0 => runtime.cancel(),
            1 => runtime.synchronize((
                SessionGeneration::new(1),
                PositionAuthorityGeneration::new(2),
            )),
            _ => runtime.defer(2 + MAX_PENDING_INTERACTION_MILLIS),
        }
        runtime.observe_input(true, false);
        assert!(!resolve_and_send(
            &mut runtime,
            &mut swings,
            Crosshair::Miss,
            &press,
            3,
            |_| {
                panic!("a new press cannot reuse a cleared retry lease");
            }
        ));
        assert!(runtime.observe_input(false, false));
    }
}

#[test]
fn a_fresh_unpublished_press_still_obeys_the_half_duration_guard() {
    for crosshair in [Crosshair::Miss, ZOMBIE] {
        let mut runtime = MeleeRuntime::default();
        let mut swings = SwingTracker::default();
        let mut press = press(PlayerInputMode::Mouse);
        assert!(swings.try_swing(press.tick, press.swing_duration));
        swings.take_started();
        swings.published_progress(press.tick);
        press.tick += 1;
        runtime.observe_input(true, false);
        let missed = resolve_and_send(&mut runtime, &mut swings, crosshair, &press, 1, |packets| {
            assert_eq!(
                kinds(&packets),
                if crosshair == Crosshair::Miss {
                    vec![]
                } else {
                    vec!["InventoryTransactionPacket"]
                }
            );
            Ok(())
        });
        assert_eq!(missed, crosshair == Crosshair::Miss);
        assert!(!runtime.observe_input(false, false));
        assert_eq!(swings.take_started(), None);
    }
}

mod picking;
