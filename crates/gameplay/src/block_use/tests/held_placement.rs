use super::*;
use crate::block_use::{BuildIntention, PlacementTarget};
use protocol::wire::valentine::bedrock::version::v1_26_51::{
    EnumsItemUseInventoryTransactionTriggerType, InventoryTransactionPacketTransaction,
    McpePacketData,
};

/// Advances one deterministic use tick against the known support and actor box.
fn step(
    runtime: &mut BlockUseRuntime,
    tick: u64,
    hit: Option<PlacementTarget>,
    eye: [f32; 3],
    velocity: [f32; 3],
    sneaking: bool,
) -> Option<([i32; 3], Vec<protocol::Packet>)> {
    let clock = RepeatClock {
        now_millis: tick * 50,
        speed: velocity
            .iter()
            .map(|n| (n * 20.0).powi(2))
            .sum::<f32>()
            .sqrt(),
        sneaking,
        survival: true,
    };
    let (trigger, due) = runtime.due(true, tick, clock)?;
    let endpoint = [eye[0], eye[1] - 5.7, eye[2]];
    let Some(target) = runtime
        .intention
        .target(hit, eye, endpoint, velocity, sneaking)
    else {
        runtime.record(trigger, due, tick, LocalUse::Nothing, clock);
        return None;
    };
    let mut around = surroundings("minecraft:stone", "minecraft:air");
    around.player_box = (
        [
            eye[0] as f64 - 0.3,
            eye[1] as f64 - 1.62,
            eye[2] as f64 - 0.3,
        ],
        [
            eye[0] as f64 + 0.3,
            eye[1] as f64 + 0.18,
            eye[2] as f64 + 0.3,
        ],
    );
    around.sneaking = sneaking;
    let item = verified(network_item(2, 77));
    let outcome = LocalUse::resolve(
        &item,
        target.position,
        target.face,
        &around,
        &GameModeCapabilities::for_mode(PlayerGameMode::Survival),
    );
    let destination = around.destination(target.position, target.face).0;
    let mut observed = crate::interaction_authority::FrozenBlockObservation::fixture(
        target.position,
        target.face,
        item,
    );
    observed.target.relative_hit = if hit.is_some() {
        [0.5, 1.0, 0.5]
    } else {
        [0.0; 3]
    };
    let start = runtime
        .last_success_destination()
        .is_none()
        .then_some(destination);
    let packets = use_packets(
        (&observed, 9),
        eye,
        trigger,
        outcome,
        start,
        None,
        42,
        |_| true,
        tick,
    );
    runtime.intention.record(
        trigger == ItemUseTrigger::SimulationTick,
        destination,
        outcome,
        true,
        sneaking,
        eye,
    );
    runtime.record(trigger, due, tick, outcome, clock);
    if outcome != LocalUse::Place {
        return None;
    }
    let packet = packets.last().unwrap();
    let McpePacketData::InventoryTransactionPacket(tx) = &packet.data else {
        panic!("placement transaction");
    };
    let InventoryTransactionPacketTransaction::ItemUseInventoryTransaction(tx) = &tx.transaction
    else {
        panic!("item-use transaction");
    };
    assert_eq!(
        [tx.position.x, tx.position.y, tx.position.z],
        target.position
    );
    assert_eq!(tx.face, target.face);
    assert_eq!(tx.target_block_id, 9);
    assert_eq!(
        tx.trigger_type,
        if tick == 1 {
            EnumsItemUseInventoryTransactionTriggerType::Playerinput
        } else {
            EnumsItemUseInventoryTransactionTriggerType::Simulationtick
        }
    );
    assert_eq!(
        [
            tx.click_position.x,
            tx.click_position.y,
            tx.click_position.z
        ],
        observed.target.relative_hit
    );
    Some((destination, packets))
}

/// Starts with a successful side placement followed by resolved forward movement.
fn forward_bridge(jumping: bool) -> Vec<(u64, [i32; 3])> {
    let mut runtime = BlockUseRuntime::default();
    runtime.observe_use(true, true, false, true);
    let mut placed = Vec::new();
    for tick in 1..=16 {
        let x = if tick < 8 {
            0.5
        } else if tick < 11 {
            1.5
        } else if tick < 15 {
            2.5
        } else {
            3.5
        };
        let hit = if tick == 1 {
            Some(PlacementTarget {
                position: [-1, 63, 0],
                face: 5,
            })
        } else if tick <= 8 {
            Some(PlacementTarget {
                position: [0, 63, 0],
                face: 1,
            })
        } else {
            None
        };
        let eye = [x, if jumping && tick > 8 { 66.8 } else { 65.62 }, 0.5];
        if let Some((position, _)) = step(&mut runtime, tick, hit, eye, [0.22, 0.0, 0.0], false) {
            placed.push((tick, position));
        }
    }
    placed
}

#[test]
fn holding_use_walking_off_a_ledge_continues_the_locked_line() {
    assert_eq!(
        forward_bridge(false),
        [
            (1, [0, 63, 0]),
            (8, [1, 63, 0]),
            (11, [2, 63, 0]),
            (15, [3, 63, 0])
        ]
    );
}

#[test]
fn jump_bridging_keeps_the_horizontal_line_while_airborne() {
    assert_eq!(
        forward_bridge(true),
        [
            (1, [0, 63, 0]),
            (8, [1, 63, 0]),
            (11, [2, 63, 0]),
            (15, [3, 63, 0])
        ]
    );
}

#[test]
fn towering_waits_for_the_player_box_to_clear_and_retries_each_tick() {
    let mut runtime = BlockUseRuntime::default();
    runtime.observe_use(true, true, false, true);
    let initial = Some(PlacementTarget {
        position: [0, 63, 0],
        face: 1,
    });
    assert_eq!(
        step(
            &mut runtime,
            1,
            initial,
            [0.5, 66.8, 0.5],
            [0.0, 0.42, 0.0],
            false
        )
        .unwrap()
        .0,
        [0, 64, 0]
    );
    let above = Some(PlacementTarget {
        position: [0, 64, 0],
        face: 1,
    });
    assert!(
        step(
            &mut runtime,
            8,
            above,
            [0.5, 66.8, 0.5],
            [0.0, 0.42, 0.0],
            false
        )
        .is_none()
    );
    assert_eq!(
        step(
            &mut runtime,
            9,
            above,
            [0.5, 67.8, 0.5],
            [0.0, 0.42, 0.0],
            false
        )
        .unwrap()
        .0,
        [0, 65, 0]
    );
    assert_eq!(
        step(
            &mut runtime,
            12,
            None,
            [0.5, 68.8, 0.5],
            [0.0, 0.42, 0.0],
            false
        )
        .unwrap()
        .0,
        [0, 66, 0]
    );
}

#[test]
fn sneak_bridging_backward_recasts_support_without_acquiring_a_line() {
    let mut runtime = BlockUseRuntime::default();
    runtime.observe_use(true, true, false, true);
    let mut placed = Vec::new();
    for (tick, support, x) in [
        (1, [0, 63, 0], -0.5),
        (7, [-1, 63, 0], -1.5),
        (8, [-1, 63, 0], -1.5),
        (15, [-2, 63, 0], -2.5),
    ] {
        let hit = Some(PlacementTarget {
            position: support,
            face: 4,
        });
        if let Some((position, _)) = step(
            &mut runtime,
            tick,
            hit,
            [x, 65.62, 0.5],
            [-0.06, 0.0, 0.0],
            true,
        ) {
            placed.push((tick, position));
        }
    }
    assert_eq!(
        placed,
        [(1, [-1, 63, 0]), (8, [-2, 63, 0]), (15, [-3, 63, 0])]
    );
    assert!(
        runtime
            .intention
            .target(
                None,
                [0.5, 65.62, 0.5],
                [0.5, 59.92, 0.5],
                [-0.06, 0.0, 0.0],
                true
            )
            .is_none()
    );
}

#[test]
fn a_locked_line_rejects_rays_that_end_early_or_turn_away() {
    let mut intent = BuildIntention::default();
    intent.record(
        false,
        [0, 63, 0],
        LocalUse::Place,
        true,
        false,
        [0.5, 64.0, 0.5],
    );
    intent.record(
        true,
        [1, 63, 0],
        LocalUse::Place,
        true,
        false,
        [1.5, 64.0, 0.5],
    );
    assert!(
        intent
            .target(
                None,
                [2.5, 65.62, 0.5],
                [2.5, 64.1, 0.5],
                [0.22, 0.0, 0.0],
                false
            )
            .is_none()
    );
    assert!(
        intent
            .target(
                None,
                [3.5, 65.62, 0.5],
                [3.5, 59.92, 0.5],
                [0.22, 0.0, 0.0],
                false
            )
            .is_none()
    );
    assert_eq!(
        intent.target(
            None,
            [2.5, 65.62, 0.5],
            [2.5, 59.92, 0.5],
            [0.0, 0.42, 0.0],
            false
        ),
        Some(PlacementTarget {
            position: [1, 63, 0],
            face: 5
        })
    );
}

#[test]
fn an_authority_wait_preserves_the_line_and_resumes_with_fresh_evidence() {
    let mut runtime = BlockUseRuntime::default();
    runtime.synchronize((7, 0));
    runtime.observe_use(true, true, false, true);
    for (tick, support, face, x) in [(1, [-1, 63, 0], 5, 0.5), (8, [0, 63, 0], 1, 1.5)] {
        assert!(
            step(
                &mut runtime,
                tick,
                Some(PlacementTarget {
                    position: support,
                    face
                }),
                [x, 65.62, 0.5],
                [0.22, 0.0, 0.0],
                false,
            )
            .is_some()
        );
    }
    runtime.synchronize((7, 1));
    runtime.observe_use(true, false, false, false);
    assert!(
        step(
            &mut runtime,
            11,
            None,
            [2.5, 65.62, 0.5],
            [0.22, 0.0, 0.0],
            false,
        )
        .is_none()
    );
    assert_eq!(runtime.last_success_destination(), Some([1, 63, 0]));
    runtime.observe_use(true, false, false, true);
    let (destination, packets) = step(
        &mut runtime,
        12,
        None,
        [2.5, 65.62, 0.5],
        [0.22, 0.0, 0.0],
        false,
    )
    .unwrap();
    assert_eq!(destination, [2, 63, 0]);
    assert_eq!(packets.len(), 2);
}
