use std::collections::HashMap;

use client_world::{ActorPose, ActorSnapshot};
use protocol::{ActorKind, ActorMetadataValue};

use super::*;

fn actor(
    runtime_id: u64,
    identifier: &str,
    feet: [f32; 3],
    size: Option<(f32, f32)>,
) -> ActorSnapshot {
    let pose = ActorPose {
        position: feet,
        pitch: 0.0,
        yaw: 0.0,
        head_yaw: 0.0,
    };
    let metadata = size
        .map(|(width, height)| {
            HashMap::from([
                (53, ActorMetadataValue::Float(width)),
                (54, ActorMetadataValue::Float(height)),
            ])
        })
        .unwrap_or_default();
    ActorSnapshot {
        unique_id: runtime_id as i64 * 10,
        runtime_id,
        spawn_revision: 0,
        movement_revision: 0,
        kind: ActorKind::Entity {
            identifier: identifier.into(),
        },
        position: feet,
        velocity: [0.0; 3],
        pitch: 0.0,
        yaw: 0.0,
        head_yaw: 0.0,
        previous_pose: pose,
        received_pose: pose,
        interpolation_ticks_remaining: 0,
        body_yaw: 0.0,
        on_ground: None,
        teleported: false,
        player_mode: None,
        source_tick: None,
        metadata,
        attributes: HashMap::new(),
        int_properties: HashMap::new(),
        float_properties: HashMap::new(),
        status: Default::default(),
    }
}

const EYE: [f32; 3] = [0.0, 1.62, 0.0];
const NORTH: [f32; 3] = [0.0, 0.0, -1.0];

#[test]
fn nearest_pickable_actor_wins_and_reports_the_inflated_entry_point() {
    let near = actor(1, "minecraft:zombie", [0.0, 0.0, -2.0], Some((0.6, 1.95)));
    let far = actor(2, "minecraft:zombie", [0.0, 0.0, -4.0], Some((0.6, 1.95)));
    let drop = actor(3, "minecraft:item", [0.0, 0.0, -1.0], Some((0.25, 0.25)));
    let sizeless = actor(4, "minecraft:cow", [0.0, 0.0, -1.5], None);
    let actors = [far, drop, sizeless, near];
    let hit = pick_actor(actors.iter(), None, EYE, NORTH, 5.7).unwrap();
    assert_eq!(hit.runtime_id, 1);
    // Front face at z = -2 + 0.3, grown by the pick radius.
    assert!((hit.distance - 1.6).abs() < 1e-6, "{}", hit.distance);
    assert!((hit.point[2] + 1.6).abs() < 1e-6);
    // The ridden vehicle is never picked.
    let hit = pick_actor(actors.iter(), Some(10), EYE, NORTH, 5.7).unwrap();
    assert_eq!(hit.runtime_id, 2);
    assert_eq!(pick_actor(actors[..1].iter(), None, EYE, NORTH, 3.0), None);
}

#[test]
fn an_eye_inside_the_box_hits_at_zero_distance() {
    let around = actor(5, "minecraft:slime", [0.0, 0.0, 0.0], Some((2.0, 2.0)));
    let hit = pick_actor([around].iter(), None, EYE, [1.0, 0.0, 0.0], 3.0).unwrap();
    assert_eq!(hit.distance, 0.0);
    assert_eq!(pick_actor([].iter(), None, EYE, [0.0; 3], 3.0), None);
}

#[test]
fn survival_reach_and_block_occlusion_decide_the_press() {
    let hit = |distance| ActorHit {
        runtime_id: 7,
        distance,
        point: [0.0; 3],
    };
    assert_eq!(
        classify(Some(hit(2.0)), Some(4.0), 3.0),
        Crosshair::Actor(hit(2.0))
    );
    // In front of the block but beyond melee reach: nothing is targeted.
    assert_eq!(classify(Some(hit(3.5)), Some(5.0), 3.0), Crosshair::Miss);
    assert_eq!(classify(Some(hit(3.5)), None, 3.0), Crosshair::Miss);
    // The actor must beat the block by the pick radius.
    assert_eq!(classify(Some(hit(1.95)), Some(2.0), 3.0), Crosshair::Block);
    assert_eq!(classify(None, Some(2.0), 3.0), Crosshair::Block);
    assert_eq!(classify(None, None, 3.0), Crosshair::Miss);
    assert_eq!(
        classify(Some(hit(5.0)), None, 7.0),
        Crosshair::Actor(hit(5.0))
    );
}

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
    assert!(swings.try_swing(13, 6));
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

fn press(input_mode: PlayerInputMode) -> PressContext {
    let stack = protocol::NetworkItemStack::empty();
    PressContext {
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
        now_millis: 1_000,
    }
}

fn kinds(packets: &[protocol::Packet]) -> Vec<String> {
    packets
        .iter()
        .map(|packet| format!("{:?}", packet.header.id))
        .collect()
}

const ZOMBIE: Crosshair = Crosshair::Actor(ActorHit {
    runtime_id: 9,
    distance: 2.0,
    point: [0.0, 1.5, -2.0],
});

#[test]
fn one_press_attacks_once_with_the_swing_first_and_held_frames_do_nothing() {
    let mut runtime = MeleeRuntime::default();
    let mut swings = SwingTracker::default();
    assert!(runtime.observe_input(true, true));
    let outcome = runtime.resolve(ZOMBIE, &press(PlayerInputMode::Mouse), &mut swings);
    assert_eq!(
        kinds(&outcome.packets),
        ["AnimatePacket", "InventoryTransactionPacket"]
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
    runtime.synchronize((7, 0));
    runtime.observe_input(true, false);
    runtime.synchronize((8, 0));
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
    runtime.defer(10 + MAX_PENDING_INTERACTION_FRAMES);
    let outcome = runtime.resolve(
        ZOMBIE,
        &press(PlayerInputMode::Mouse),
        &mut SwingTracker::default(),
    );
    assert_eq!(outcome.packets.len(), 2, "the deferred press still attacks");

    runtime.observe_input(true, true);
    runtime.defer(50);
    runtime.defer(51 + MAX_PENDING_INTERACTION_FRAMES);
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
