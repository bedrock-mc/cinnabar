use std::sync::Arc;

use protocol::{ItemUseTrigger, NetworkItemStack, PlayerGameMode, VerifiedNetworkItemStack};
use sha2::{Digest, Sha256};

use super::{
    BlockUseRuntime, LocalUse, RepeatClock, UseSurroundings, placement_cell,
    repeat_interval_millis, toggled_states, use_packets,
};
use client_world::game_mode_capabilities::GameModeCapabilities;

mod held_placement;
mod respawn_anchor;

fn network_item(network_id: i32, block_runtime_id: i32) -> NetworkItemStack {
    let extra_data: Arc<[u8]> = Arc::from([]);
    NetworkItemStack {
        network_id,
        metadata: 0,
        stack_network_id: 41,
        count: 1,
        nbt_digest: Sha256::digest(&extra_data).into(),
        block_runtime_id,
        extra_data,
    }
}

fn verified(stack: NetworkItemStack) -> VerifiedNetworkItemStack {
    VerifiedNetworkItemStack::try_new(stack.clone(), stack.nbt_digest).unwrap()
}

#[test]
fn held_repeats_follow_stance_speed_and_the_survival_floor() {
    assert_eq!(repeat_interval_millis(true, false, 5.0, true), 300);
    assert_eq!(repeat_interval_millis(false, true, 5.0, true), 300);
    assert_eq!(repeat_interval_millis(false, false, 0.0, true), 200);
    assert_eq!(repeat_interval_millis(false, false, f32::NAN, false), 200);
    // Any nonzero speed uses the moving formula.
    assert_eq!(repeat_interval_millis(false, false, 0.001, true), 180);
    assert_eq!(repeat_interval_millis(false, false, 10.0, false), 90);
    assert_eq!(repeat_interval_millis(false, false, 10.0, true), 100);
}

fn clock(now_millis: u64, speed: f32) -> RepeatClock {
    RepeatClock {
        now_millis,
        sneaking: false,
        speed,
        survival: true,
    }
}

#[test]
fn press_fires_at_once_and_held_repeats_keep_a_bounded_schedule() {
    let mut runtime = BlockUseRuntime::default();
    assert_eq!(runtime.due(true, 1, clock(0, 0.0)), None);
    runtime.latched_press = true;
    let (trigger, due) = runtime.due(false, 1, clock(1_000, 0.0)).unwrap();
    assert_eq!(trigger, ItemUseTrigger::PlayerInput);
    runtime.intention.record(
        false,
        [0, 63, 0],
        LocalUse::Place,
        true,
        false,
        [0.5, 64.0, 0.5],
    );
    runtime.record(trigger, due, 1, LocalUse::Place, clock(1_000, 0.0));
    // A fresh placement repeats at the slow interval, then the line is established.
    assert_eq!(runtime.due(true, 2, clock(1_300, 0.0)), None);
    let (trigger, due) = runtime.due(true, 3, clock(1_301, 0.0)).unwrap();
    assert_eq!((trigger, due), (ItemUseTrigger::SimulationTick, 1_300));
    runtime.intention.record(
        false,
        [0, 63, 0],
        LocalUse::Place,
        true,
        false,
        [0.5, 64.0, 0.5],
    );
    runtime.intention.record(
        true,
        [1, 63, 0],
        LocalUse::Place,
        true,
        false,
        [1.5, 64.0, 0.5],
    );
    runtime.record(trigger, due, 3, LocalUse::Place, clock(1_301, 0.0));
    // Still: anchored to now. Moving: to the due time, lagging at most 180 ms.
    assert_eq!(runtime.due(true, 4, clock(1_501, 0.0)), None);
    let (trigger, due) = runtime.due(true, 4, clock(1_502, 5.0)).unwrap();
    assert_eq!(due, 1_481);
    runtime.record(trigger, due, 4, LocalUse::Place, clock(1_502, 5.0));
    assert_eq!(runtime.last_use_millis, Some(1_481));
    runtime.record(
        ItemUseTrigger::SimulationTick,
        1_600,
        5,
        LocalUse::Place,
        clock(2_000, 5.0),
    );
    assert_eq!(runtime.last_use_millis, Some(1_820));
    // One attempt per tick; a failure keeps the schedule and retries next tick.
    assert_eq!(runtime.due(true, 5, clock(5_000, 5.0)), None);
    let (trigger, due) = runtime.due(true, 6, clock(5_000, 5.0)).unwrap();
    runtime.record(trigger, due, 6, LocalUse::Nothing, clock(5_000, 5.0));
    assert!(runtime.due(true, 7, clock(5_001, 5.0)).is_some());
}

#[test]
fn placement_targets_the_clicked_face_neighbor() {
    let clicked = [4, 64, -2];
    let cells = (0..6)
        .map(|face| placement_cell(clicked, face))
        .collect::<Vec<_>>();
    assert_eq!(
        cells,
        [
            [4, 63, -2],
            [4, 65, -2],
            [4, 64, -3],
            [4, 64, -1],
            [3, 64, -2],
            [5, 64, -2]
        ]
    );
}

fn surroundings(clicked: &str, neighbor: &str) -> UseSurroundings {
    UseSurroundings {
        clicked_identifier: Some(clicked.to_owned()),
        clicked_canonical_state: None,
        held_block_identifier: None,
        neighbor_identifier: Some(neighbor.to_owned()),
        player_box: ([0.2, 64.0, 0.2], [0.8, 65.8, 0.8]),
        actor_boxes: Vec::new(),
        sneaking: false,
        placed_boxes: None,
    }
}

#[test]
fn local_use_decides_interaction_placement_or_nothing() {
    let block = verified(network_item(2, 77));
    let stick = verified(network_item(3, 0));
    let empty = verified(NetworkItemStack::empty());
    let survival = GameModeCapabilities::for_mode(PlayerGameMode::Survival);
    let place = |item, clicked, face, around: &UseSurroundings| {
        LocalUse::resolve(item, clicked, face, around, &survival)
    };
    let stone = surroundings("minecraft:stone", "minecraft:air");
    assert_eq!(place(&block, [2, 63, 0], 1, &stone), LocalUse::Place);
    assert_eq!(place(&stick, [2, 63, 0], 1, &stone), LocalUse::Nothing);
    assert_eq!(
        place(
            &block,
            [2, 63, 0],
            1,
            &surroundings("minecraft:stone", "minecraft:dirt")
        ),
        LocalUse::Nothing
    );
    // The player's own column cannot receive a block, nor can an occupied cell.
    assert_eq!(place(&block, [0, 64, 0], 1, &stone), LocalUse::Nothing);
    let occupied = UseSurroundings {
        actor_boxes: vec![([1.7, 64.0, -0.3], [2.3, 65.9, 0.3])],
        ..stone.clone()
    };
    assert_eq!(place(&block, [2, 63, 0], 1, &occupied), LocalUse::Nothing);
    // A replaceable clicked block is replaced in place, whatever the face.
    let grass = surroundings("minecraft:short_grass", "minecraft:stone");
    assert_eq!(place(&block, [2, 64, 0], 4, &grass), LocalUse::Place);
    assert_eq!(place(&block, [0, 64, 0], 4, &grass), LocalUse::Nothing);
    // Interactive blocks succeed unless sneaking with an item.
    let chest = surroundings("minecraft:chest", "minecraft:air");
    assert_eq!(place(&empty, [2, 63, 0], 1, &chest), LocalUse::Interact);
    assert_eq!(place(&block, [2, 63, 0], 1, &chest), LocalUse::Interact);
    let sneaking = UseSurroundings {
        sneaking: true,
        ..chest
    };
    assert_eq!(place(&block, [2, 63, 0], 1, &sneaking), LocalUse::Place);
    assert_eq!(place(&empty, [2, 63, 0], 1, &sneaking), LocalUse::Interact);
    let iron = surroundings("minecraft:iron_door", "minecraft:air");
    assert_eq!(place(&empty, [2, 63, 0], 1, &iron), LocalUse::Nothing);
}

/// Obstruction tests the placed block's own shape, and a replaced block is the destination.
#[test]
fn placement_obstruction_uses_the_placed_shape_and_resolved_cell() {
    let survival = GameModeCapabilities::for_mode(PlayerGameMode::Survival);
    let block = verified(network_item(2, 77));
    // A sneaking player (1.5 tall) standing at y 64 reaches 65.5.
    let around = |placed_boxes| UseSurroundings {
        player_box: ([0.2, 64.0, 0.2], [0.8, 65.5, 0.8]),
        placed_boxes,
        sneaking: true,
        ..surroundings("minecraft:stone", "minecraft:air")
    };
    let place =
        |around: &UseSurroundings| LocalUse::resolve(&block, [0, 66, 0], 0, around, &survival);
    assert_eq!(
        place(&around(None)),
        LocalUse::Nothing,
        "a full cell overlaps"
    );
    let top_slab = vec![([0.0, 0.5, 0.0], [1.0, 1.0, 1.0])];
    assert_eq!(place(&around(Some(top_slab))), LocalUse::Place);
    let bottom_slab = vec![([0.0, 0.0, 0.0], [1.0, 0.5, 1.0])];
    assert_eq!(place(&around(Some(bottom_slab))), LocalUse::Nothing);
    assert_eq!(
        place(&around(Some(Vec::new()))),
        LocalUse::Place,
        "no collision"
    );
    let grass = surroundings("minecraft:short_grass", "minecraft:stone");
    assert_eq!(grass.destination([2, 64, 0], 4), ([2, 64, 0], true));
    let stone = surroundings("minecraft:stone", "minecraft:air");
    assert_eq!(stone.destination([2, 64, 0], 4), ([1, 64, 0], true));
}

/// A full cube does not merge into the clicked cube; its placement is just as certain.
#[test]
fn a_stateless_cube_predicts_when_the_clicked_block_is_the_same_kind() {
    let cobblestone = "minecraft:cobblestone";
    let around = surroundings(cobblestone, "minecraft:air");
    let block = verified(network_item(2, 77));
    let caps = GameModeCapabilities::for_mode(PlayerGameMode::Survival);
    let clicked = [2, 63, 0];
    assert_eq!(
        LocalUse::resolve(&block, clicked, 1, &around, &caps),
        LocalUse::Place
    );
    assert_eq!(around.destination(clicked, 1), ([2, 64, 0], true));
}

/// Trapdoors and levers flip, buttons press once, and two-part switches are left to the server.
#[test]
fn switch_uses_predict_their_toggled_state() {
    let flipped = |identifier, state: &str| {
        toggled_states(identifier, state)
            .map(|states| serde_json::Value::Object(states).to_string())
    };
    assert_eq!(
        flipped(
            "minecraft:spruce_trapdoor",
            r#"{"direction":2,"open_bit":0,"upside_down_bit":1}"#
        ),
        Some(r#"{"direction":2,"open_bit":1,"upside_down_bit":1}"#.to_owned())
    );
    assert_eq!(
        flipped(
            "minecraft:lever",
            r#"{"open_bit":{"type":"byte","value":1}}"#
        ),
        Some(r#"{"open_bit":{"type":"byte","value":0}}"#.to_owned())
    );
    assert_eq!(
        flipped("minecraft:stone_button", r#"{"button_pressed_bit":false}"#),
        Some(r#"{"button_pressed_bit":true}"#.to_owned())
    );
    assert_eq!(
        flipped("minecraft:stone_button", r#"{"button_pressed_bit":true}"#),
        None
    );
    assert_eq!(flipped("minecraft:oak_door", r#"{"open_bit":0}"#), None);
    assert_eq!(
        flipped("minecraft:iron_trapdoor", r#"{"open_bit":0}"#),
        None
    );
}

/// Adventure uses doors and containers but cannot place; each ability gates only its own use.
#[test]
fn interaction_and_placement_follow_their_own_abilities() {
    let block = verified(network_item(2, 77));
    let adventure = GameModeCapabilities::for_mode(PlayerGameMode::Adventure);
    let resolve = |clicked: &str, caps: &GameModeCapabilities| {
        LocalUse::resolve(
            &block,
            [2, 63, 0],
            1,
            &surroundings(clicked, "minecraft:air"),
            caps,
        )
    };
    assert_eq!(
        resolve("minecraft:oak_door", &adventure),
        LocalUse::Interact
    );
    assert_eq!(resolve("minecraft:chest", &adventure), LocalUse::Interact);
    assert_eq!(resolve("minecraft:stone", &adventure), LocalUse::Nothing);
    assert!(adventure.can_use_blocks());
    let no_switches = GameModeCapabilities {
        can_use_switches: false,
        ..GameModeCapabilities::for_mode(PlayerGameMode::Survival)
    };
    assert_eq!(
        resolve("minecraft:stone_button", &no_switches),
        LocalUse::Place
    );
    assert_eq!(
        resolve("minecraft:barrel", &no_switches),
        LocalUse::Interact
    );
    let mine_only = GameModeCapabilities {
        can_build: false,
        ..adventure
    };
    assert_eq!(resolve("minecraft:stone", &mine_only), LocalUse::Nothing);
}

#[test]
fn successful_uses_swing_before_their_always_sent_transaction() {
    let observed = crate::interaction_authority::FrozenBlockObservation::fixture(
        [2, 63, 0],
        1,
        verified(network_item(2, 77)),
    );
    let kinds = |local_use| {
        use_packets(
            (&observed, observed.target.runtime_id),
            [0.5, 65.62, 0.5],
            ItemUseTrigger::PlayerInput,
            local_use,
            None,
            None,
            42,
            |_| true,
            101,
        )
        .iter()
        .map(|packet| format!("{:?}", packet.header.id))
        .collect::<Vec<_>>()
    };
    assert_eq!(
        kinds(LocalUse::Place),
        ["AnimatePacket", "InventoryTransactionPacket"]
    );
    assert_eq!(
        kinds(LocalUse::Interact),
        ["AnimatePacket", "InventoryTransactionPacket"]
    );
    assert_eq!(kinds(LocalUse::Nothing), ["InventoryTransactionPacket"]);
    let guarded = use_packets(
        (&observed, observed.target.runtime_id),
        [0.5, 65.62, 0.5],
        ItemUseTrigger::SimulationTick,
        LocalUse::Place,
        None,
        None,
        42,
        |_| false,
        101,
    );
    assert_eq!(
        guarded.len(),
        1,
        "the half-swing guard suppresses the animation only"
    );
}

#[test]
fn a_position_authority_change_revokes_the_press_but_preserves_repeat_timing() {
    let mut runtime = BlockUseRuntime::default();
    runtime.synchronize((7, 0));
    runtime.latched_press = true;
    runtime.last_use_millis = Some(900);
    runtime.synchronize((7, 1));
    assert_eq!(runtime.due(false, 1, clock(1_000, 0.0)), None);
    assert_eq!(runtime.last_use_millis, Some(900));
    assert_eq!(runtime.due(true, 1, clock(1_000, 0.0)), None);
}

#[test]
fn review_quick_use_press_survives_release_before_the_next_tick() {
    let mut runtime = BlockUseRuntime::default();
    assert!(runtime.observe_use(true, true, false, true));
    assert!(runtime.observe_use(false, false, false, true));
    assert!(runtime.due(false, 1, clock(1000, 0.0)).is_some());
}

#[test]
fn review_refused_use_preserves_the_press_and_success_schedule() {
    let mut runtime = BlockUseRuntime::default();
    runtime.observe_use(true, true, false, true);
    assert!(!runtime.admit(
        ItemUseTrigger::PlayerInput,
        1000,
        1,
        LocalUse::Interact,
        clock(1000, 0.0),
        false
    ));
    assert!(runtime.due(false, 2, clock(1001, 0.0)).is_some());
    assert_eq!(runtime.last_use_millis, None);
    assert!(!runtime.interacted_at(1));
}

/// Successful repeats alone do not establish an adjacent placement line.
#[test]
fn unlined_repeats_keep_the_initial_delay() {
    let mut runtime = BlockUseRuntime::default();
    runtime.intention.record(
        false,
        [0, 63, 0],
        LocalUse::Place,
        true,
        false,
        [0.5, 64.0, 0.5],
    );
    runtime.record(
        ItemUseTrigger::PlayerInput,
        1000,
        1,
        LocalUse::Place,
        clock(1000, 0.0),
    );
    runtime.intention.record(
        true,
        [5, 63, 0],
        LocalUse::Place,
        true,
        false,
        [5.5, 64.0, 0.5],
    );
    runtime.record(
        ItemUseTrigger::SimulationTick,
        1300,
        8,
        LocalUse::Place,
        clock(1350, 0.0),
    );
    assert_eq!(runtime.due(true, 13, clock(1551, 0.0)), None);
}

/// A successful hold starts once, before its swing and click-block transaction.
#[test]
fn first_success_starts_before_swing_and_repeat_keeps_only_transaction() {
    let observed = crate::interaction_authority::FrozenBlockObservation::fixture(
        [2, 63, 0],
        3,
        verified(network_item(2, 77)),
    );
    let kinds = |start, trigger| {
        use_packets(
            (&observed, 9),
            [2.5, 65.62, 0.5],
            trigger,
            LocalUse::Place,
            start,
            None,
            42,
            |_| true,
            1,
        )
        .iter()
        .map(|packet| format!("{:?}", packet.header.id))
        .collect::<Vec<_>>()
    };
    assert_eq!(
        kinds(Some([2, 63, 1]), ItemUseTrigger::PlayerInput),
        [
            "PlayerActionPacket",
            "AnimatePacket",
            "InventoryTransactionPacket"
        ]
    );
    assert_eq!(
        kinds(None, ItemUseTrigger::SimulationTick),
        ["AnimatePacket", "InventoryTransactionPacket"]
    );
    let failed = use_packets(
        (&observed, 9),
        [2.5, 65.62, 0.5],
        ItemUseTrigger::SimulationTick,
        LocalUse::Nothing,
        Some([2, 63, 1]),
        None,
        42,
        |_| true,
        2,
    );
    assert_eq!(failed.len(), 1);
    assert_eq!(
        format!("{:?}", failed[0].header.id),
        "InventoryTransactionPacket"
    );
}

/// Orientation caching applies to placement-direction states, not pillar axes.
#[test]
fn orientation_sensitive_states_preserve_the_original_world_hit() {
    assert!(super::orientation_sensitive(Some(
        r#"{"minecraft:cardinal_direction":"north"}"#
    )));
    assert!(super::orientation_sensitive(Some(
        r#"{"upside_down_bit":false}"#
    )));
    assert!(!super::orientation_sensitive(Some(
        r#"{"pillar_axis":"y"}"#
    )));
    let mut intention = super::BuildIntention::default();
    intention.record(
        false,
        [0, 63, 0],
        LocalUse::Place,
        true,
        false,
        [0.75, 64.0, 0.25],
    );
    intention.record(
        true,
        [1, 63, 0],
        LocalUse::Place,
        true,
        false,
        [1.5, 64.0, 0.5],
    );
    assert_eq!(intention.first_world_hit(), Some([0.75, 64.0, 0.25]));
}

#[test]
fn adventure_held_repeats_keep_the_noncreative_floor() {
    let mut runtime = BlockUseRuntime::default();
    let mut clock = RepeatClock::for_game_mode(1_000, false, 20.0, Some(PlayerGameMode::Adventure));
    runtime.record(
        ItemUseTrigger::PlayerInput,
        1_000,
        1,
        LocalUse::Place,
        clock,
    );
    clock.now_millis = 1_050;
    assert_eq!(runtime.due(true, 2, clock), None);
    clock.now_millis = 1_100;
    assert_eq!(runtime.due(true, 3, clock), None);
    clock.now_millis = 1_150;
    assert_eq!(
        runtime.due(true, 4, clock),
        Some((ItemUseTrigger::SimulationTick, 1_100))
    );
}
