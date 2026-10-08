use super::*;
use crate::block_use::{BuildIntention, PlacementTarget};
use crate::movement::{MovementSource, MovementTicker};
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
        match trigger {
            ItemUseTrigger::PlayerInput => EnumsItemUseInventoryTransactionTriggerType::Playerinput,
            ItemUseTrigger::SimulationTick => {
                EnumsItemUseInventoryTransactionTriggerType::Simulationtick
            }
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

/// Queues one tick ending at `eye` with the given tick-end velocity and displacement.
fn advance(ticker: &mut MovementTicker, tick: u64, eye: [f32; 3], velocity: [f32; 3]) {
    let mut sample = crate::test_support::survival_mining::completed(tick);
    sample.position = eye;
    sample.velocity = velocity;
    sample.movement = [velocity[0], 0.0, velocity[2]];
    ticker.enqueue_completed_physics(sample).unwrap();
}

/// A repeat due on the jump tick reads the pre-jump motion, so it extends the bridge
/// sideways instead of stacking on the top face under the crosshair.
#[test]
fn a_repeat_due_on_the_jump_tick_extends_the_bridge_instead_of_stacking() {
    let mut ticker = MovementTicker::default();
    ticker.reset(7, 0, [0.5, 65.62, 0.5]);
    ticker.set_source(MovementSource::Physics);
    let mut runtime = BlockUseRuntime::default();
    runtime.observe_use(true, true, false, true);
    let side = PlacementTarget {
        position: [-1, 63, 0],
        face: 5,
    };
    let top = PlacementTarget {
        position: [0, 63, 0],
        face: 1,
    };
    let mut placed = Vec::new();
    for tick in 1..=8 {
        let jumping = tick == 8;
        let eye = [
            0.5 + 0.12 * tick as f32,
            if jumping { 66.04 } else { 65.62 },
            0.5,
        ];
        let velocity = if jumping {
            [0.12, 0.3332, 0.0]
        } else {
            [0.12, -0.0784, 0.0]
        };
        // The build action resolves before this tick simulates.
        let state = ticker.build_action_state().unwrap();
        let hit = if tick == 1 { side } else { top };
        if let Some((position, _)) = step(
            &mut runtime,
            tick,
            Some(hit),
            state.position,
            state.delta,
            state.sneaking,
        ) {
            placed.push((tick, position));
        }
        advance(&mut ticker, tick, eye, velocity);
    }
    assert_eq!(placed, [(1, [0, 63, 0]), (8, [1, 63, 0])]);
}

/// Cadence is timed from the previous tick's motion: stopping still repeats on the moving delay.
#[test]
fn held_cadence_uses_the_motion_of_the_previous_tick() {
    let mut ticker = MovementTicker::default();
    ticker.reset(7, 0, [0.5, 65.62, 0.5]);
    ticker.set_source(MovementSource::Physics);
    advance(&mut ticker, 1, [0.75, 65.62, 0.5], [0.25, 0.0, 0.0]);
    // Tick 2's build action reads tick 1's motion; the stop simulated on tick 2 is later.
    let state = ticker.build_action_state().unwrap();
    advance(&mut ticker, 2, [0.75, 65.62, 0.5], [0.0; 3]);
    let mut runtime = BlockUseRuntime::default();
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
    let clock =
        |now_millis| RepeatClock::for_state(now_millis, &state, Some(PlayerGameMode::Survival));
    runtime.record(
        ItemUseTrigger::PlayerInput,
        1_000,
        1,
        LocalUse::Place,
        clock(1_000),
    );
    assert_eq!(clock(0).speed, 5.0);
    assert_eq!(runtime.due(true, 2, clock(1_180)), None);
    assert_eq!(
        runtime.due(true, 2, clock(1_181)),
        Some((ItemUseTrigger::SimulationTick, 1_180))
    );
}

/// Releasing use stops at the last destination and drops the line; a new press is fresh.
#[test]
fn releasing_use_resets_the_placement_lock() {
    let mut runtime = BlockUseRuntime::default();
    runtime.observe_use(true, true, false, true);
    for (tick, support, face, x) in [(1, [-1, 63, 0], 5, 0.5), (8, [0, 63, 0], 1, 1.5)] {
        let hit = Some(PlacementTarget {
            position: support,
            face,
        });
        assert!(
            step(
                &mut runtime,
                tick,
                hit,
                [x, 65.62, 0.5],
                [0.22, 0.0, 0.0],
                false
            )
            .is_some()
        );
    }
    assert!(!runtime.intention.unlined());
    assert_eq!(
        runtime.stop_packets(42, false),
        [protocol::stop_item_use_on_packet(42, [1, 63, 0])]
    );
    assert!(runtime.admit_stop(true));
    assert!(!runtime.observe_use(false, false, false, true));
    assert_eq!(runtime.last_success_destination(), None);
    runtime.observe_use(true, true, false, true);
    let top = Some(PlacementTarget {
        position: [1, 63, 0],
        face: 1,
    });
    let (destination, packets) = step(
        &mut runtime,
        20,
        top,
        [2.5, 65.62, 0.5],
        [0.22, 0.0, 0.0],
        false,
    )
    .unwrap();
    assert_eq!(destination, [1, 64, 0]);
    assert_eq!(packets.len(), 3);
}
