use std::{collections::HashMap, sync::Arc};

use client_world::{
    ActorSnapshot, BOUNDING_BOX_HEIGHT_METADATA_KEY, BOUNDING_BOX_WIDTH_METADATA_KEY,
    SCALE_METADATA_KEY, WorldAuthority,
};
use protocol::wire::valentine::bedrock::version::v1_26_51::{
    EnumsItemUseOnActorInventoryTransactionActionType, InventoryTransactionPacketTransaction,
    McpePacketData,
};
use protocol::{ActorKind, ActorMetadataValue};

use super::*;

fn actor(
    runtime_id: u64,
    identifier: &str,
    feet: [f32; 3],
    size: Option<(f32, f32)>,
) -> ActorSnapshot {
    let metadata = size
        .map(|(width, height)| {
            HashMap::from([
                (
                    BOUNDING_BOX_WIDTH_METADATA_KEY,
                    ActorMetadataValue::Float(width),
                ),
                (
                    BOUNDING_BOX_HEIGHT_METADATA_KEY,
                    ActorMetadataValue::Float(height),
                ),
            ])
        })
        .unwrap_or_default();
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
            Some(1),
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
fn nearest_pickable_actor_wins_and_reports_the_inflated_entry_point() {
    let near = actor(1, "minecraft:zombie", [0.0, 0.0, -2.0], Some((0.6, 1.95)));
    let far = actor(2, "minecraft:zombie", [0.0, 0.0, -4.0], Some((0.6, 1.95)));
    let drop = actor(3, "minecraft:item", [0.0, 0.0, -1.0], Some((0.25, 0.25)));
    let invalid = actor(4, "minecraft:cow", [0.0, 0.0, -1.5], Some((f32::NAN, 1.95)));
    let actors = [far, drop, invalid, near];
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
fn custom_npc_without_size_metadata_sends_attack_for_picked_world_point() {
    let npc = actor(70, "example:game_selection_npc", [10.0, 20.0, 28.0], None);
    let origin = [10.0, 21.0, 30.0];
    let hit = pick_actor([npc].iter(), None, origin, NORTH, 3.0)
        .expect("a custom NPC retains its default collision box before size metadata arrives");
    assert_eq!(hit.runtime_id, 70);
    assert_eq!([hit.point[0], hit.point[1]], [origin[0], origin[1]]);
    assert!(hit.point[2] > 28.0 && hit.point[2] < origin[2]);

    let mut runtime = MeleeRuntime::default();
    runtime.observe_input(true, false);
    let mut press = press(PlayerInputMode::Mouse);
    press.player_position = origin;
    let outcome = runtime.resolve(
        classify(Some(hit), None, 3.0),
        &press,
        &mut SwingTracker::default(),
    );
    assert_eq!(
        kinds(&outcome.packets),
        ["AnimatePacket", "InventoryTransactionPacket"]
    );
    assert!(!outcome.missed_swing);
    let McpePacketData::InventoryTransactionPacket(packet) = &outcome.packets[1].data else {
        panic!("the picked NPC receives an attack transaction");
    };
    let InventoryTransactionPacketTransaction::ItemUseOnActorInventoryTransaction(transaction) =
        &packet.transaction
    else {
        panic!("the transaction targets an actor");
    };
    assert_eq!(
        transaction.action_type,
        EnumsItemUseOnActorInventoryTransactionActionType::Attack
    );
    assert_eq!(transaction.runtime_id.actor_runtime_id, hit.runtime_id);
    assert_eq!(
        [
            transaction.hit_position.x,
            transaction.hit_position.y,
            transaction.hit_position.z,
        ],
        hit.point
    );
    assert_eq!(
        [
            transaction.from_position.x,
            transaction.from_position.y,
            transaction.from_position.z,
        ],
        press.player_position
    );
}

#[test]
fn a_positive_actor_scale_expands_the_pick_box() {
    let mut npc = actor(
        71,
        "example:game_selection_npc",
        [0.0, 0.0, -2.0],
        Some((0.6, 1.0)),
    );
    assert_eq!(pick_actor([&npc].into_iter(), None, EYE, NORTH, 3.0), None);
    npc.metadata
        .insert(SCALE_METADATA_KEY, ActorMetadataValue::Float(2.0));
    let hit = pick_actor([npc].iter(), None, EYE, NORTH, 3.0)
        .expect("the ray intersects the scaled actor above its unscaled height");
    assert_eq!(hit.runtime_id, 71);
    assert_eq!(hit.point[1], EYE[1]);
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
