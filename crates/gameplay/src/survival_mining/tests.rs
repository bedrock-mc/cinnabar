use protocol::{
    BlockActionKind::{
        AbortDestroy, ContinueDestroy, CrackBlock, PredictDestroy, StartDestroy, StopDestroy,
    },
    NetworkItemStack, VerifiedNetworkItemStack,
};
use sim::{DestroyConditions, HeldTool};

use super::{
    BlockBreakingAuthority::{Client, Server},
    *,
};
use crate::movement::{MovementSource, MovementTicker, flush_player_auth_inputs};

fn target(position: [i32; 3], block: &str, tool: Option<&str>) -> DestroyTarget {
    DestroyTarget {
        position,
        face: 1,
        runtime_id: 9,
        relative_hit: [0.5, 1.0, 0.5],
        block: sim::block_destroy_info(block),
        conditions: DestroyConditions {
            tool: tool.and_then(HeldTool::from_identifier),
            ..DestroyConditions::default()
        },
        selection: FrozenMiningSelection {
            slot: 2,
            item: VerifiedNetworkItemStack::try_new(
                NetworkItemStack::empty(),
                NetworkItemStack::empty().nbt_digest,
            )
            .unwrap(),
        },
        wear: None,
        instant: false,
    }
}

const STILL: TickMotion = TickMotion {
    on_ground: true,
    moved: 0.0,
};

fn kinds(payload: &SurvivalTickPayload) -> Vec<(protocol::BlockActionKind, [i32; 3], u8)> {
    payload
        .actions
        .iter()
        .map(|action| (action.kind, action.position, action.face))
        .collect()
}

fn held(
    machine: &mut DestroyMachine,
    target: &DestroyTarget,
    authority: BlockBreakingAuthority,
) -> SurvivalTickPayload {
    machine.step(DestroyInput::Held(Some(target)), STILL, authority)
}

/// Held ticks after the start tick until completion, from the documented per-tick rate.
fn expected_completion_ticks(hardness: f64, speed: f64, divisor: f64) -> usize {
    let rate = speed / hardness / divisor;
    (0.99999 / rate).ceil() as usize
}

#[test]
fn server_authority_is_silent_while_cracking_and_completes_with_continue_then_predict() {
    let dirt = target([1, 2, 3], "minecraft:dirt", None);
    let expected = expected_completion_ticks(0.5, 1.0, 30.0);
    assert_eq!(expected, 15, "dirt by hand takes 0.75 s");
    let mut machine = DestroyMachine::default();
    assert_eq!(
        kinds(&held(&mut machine, &dirt, Server)),
        [(StartDestroy, [1, 2, 3], 1)]
    );
    for tick in 1..=expected {
        let payload = held(&mut machine, &dirt, Server);
        if tick < expected {
            assert!(
                payload.is_empty(),
                "tick {tick}: no crack actions under server authority"
            );
            continue;
        }
        assert_eq!(
            kinds(&payload),
            [
                (ContinueDestroy, [1, 2, 3], 1),
                (PredictDestroy, [1, 2, 3], 1)
            ]
        );
        assert_eq!(payload.destroy, None);
    }
    let next = target([1, 1, 3], "minecraft:dirt", None);
    for _ in 0..DESTROY_DELAY_TICKS {
        assert!(held(&mut machine, &next, Server).is_empty());
    }
    // The destroy stays active on the broken block, so the next block continues it.
    assert_eq!(
        kinds(&held(&mut machine, &next, Server)),
        [(ContinueDestroy, [1, 1, 3], 1)]
    );
}

#[test]
fn completion_never_lands_a_tick_late_for_documented_tool_rates() {
    for (block, tool, hardness, speed, divisor) in [
        (
            "minecraft:stone",
            Some("minecraft:wooden_pickaxe"),
            1.5,
            2.0,
            30.0,
        ),
        ("minecraft:stone", None, 1.5, 1.0, 100.0),
        (
            "minecraft:oak_log",
            Some("minecraft:stone_axe"),
            2.0,
            4.0,
            30.0,
        ),
        (
            "minecraft:obsidian",
            Some("minecraft:diamond_pickaxe"),
            35.0,
            8.0,
            30.0,
        ),
    ] {
        let destroyed = target([0, 0, 0], block, tool);
        let mut machine = DestroyMachine::default();
        held(&mut machine, &destroyed, Server);
        let mut ticks = 0;
        while !kinds(&held(&mut machine, &destroyed, Server)).contains(&(
            PredictDestroy,
            [0, 0, 0],
            1,
        )) {
            ticks += 1;
            assert!(ticks < 10_000, "{block}");
        }
        assert_eq!(
            ticks + 1,
            expected_completion_ticks(hardness, speed, divisor),
            "{block}"
        );
    }
}

#[test]
fn server_target_change_is_one_continue_and_release_aborts_with_progress_percent() {
    let first = target([0, 0, 0], "minecraft:stone", None);
    let second = target([0, 0, 1], "minecraft:stone", None);
    let mut machine = DestroyMachine::default();
    held(&mut machine, &first, Server);
    for _ in 0..30 {
        held(&mut machine, &first, Server);
    }
    // Face changes on the same block send nothing.
    let turned = DestroyTarget {
        face: 4,
        ..first.clone()
    };
    assert!(held(&mut machine, &turned, Server).is_empty());
    assert_eq!(
        kinds(&held(&mut machine, &second, Server)),
        [(ContinueDestroy, [0, 0, 1], 1)]
    );
    for _ in 0..75 {
        held(&mut machine, &second, Server);
    }
    // 75 ticks of 1/150 per tick is half the block.
    assert_eq!(
        kinds(&machine.step(DestroyInput::Released, STILL, Server)),
        [(AbortDestroy, [0, 0, 1], 50)]
    );
    assert!(
        machine
            .step(DestroyInput::Released, STILL, Server)
            .is_empty()
    );
    assert_eq!(
        kinds(&held(&mut machine, &first, Server)),
        [(StartDestroy, [0, 0, 0], 1)]
    );
    assert_eq!(
        kinds(&machine.step(DestroyInput::Held(None), STILL, Server)),
        [(AbortDestroy, [0, 0, 0], 0)]
    );
}

#[test]
fn client_authority_cracks_each_tick_and_completes_with_stop_and_destroy_transaction() {
    let dirt = target([4, 5, 6], "minecraft:dirt", None);
    let mut machine = DestroyMachine::default();
    assert_eq!(
        kinds(&held(&mut machine, &dirt, Client)),
        [(StartDestroy, [4, 5, 6], 1), (CrackBlock, [4, 5, 6], 1)]
    );
    for _ in 1..expected_completion_ticks(0.5, 1.0, 30.0) {
        assert_eq!(
            kinds(&held(&mut machine, &dirt, Client)),
            [(CrackBlock, [4, 5, 6], 1)]
        );
    }
    let done = held(&mut machine, &dirt, Client);
    assert_eq!(kinds(&done), [(StopDestroy, [0, 0, 0], 0)]);
    let (interactions, _) = done.into_interactions([0.5, 64.0, 0.5]);
    assert!(matches!(
        interactions.block_interaction,
        Some(protocol::BlockItemInteraction::Destroy(ref request))
            if request.block_position == [4, 5, 6] && request.selected_slot == 2
    ));
    for _ in 0..DESTROY_DELAY_TICKS {
        held(&mut machine, &dirt, Client);
    }
    let other = target([4, 5, 7], "minecraft:stone", None);
    assert_eq!(
        kinds(&held(&mut machine, &other, Client)),
        [
            (AbortDestroy, [4, 5, 6], 0),
            (StartDestroy, [4, 5, 7], 1),
            (CrackBlock, [4, 5, 7], 1)
        ]
    );
}

/// Only zero hardness breaks on the start tick (`GameMode::startDestroyBlock`);
/// a block with hardness breaks on the first continued tick however fast.
#[test]
fn only_zero_hardness_breaks_on_the_start_tick() {
    let torch = target([2, 2, 2], "minecraft:torch", None);
    let mut machine = DestroyMachine::default();
    assert_eq!(
        kinds(&held(&mut machine, &torch, Server)),
        [(StartDestroy, [2, 2, 2], 1), (PredictDestroy, [2, 2, 2], 1)]
    );
    let next = target([2, 1, 2], "minecraft:torch", None);
    for _ in 0..DESTROY_DELAY_TICKS {
        assert!(held(&mut machine, &next, Server).is_empty());
    }
    assert!(!held(&mut machine, &next, Server).is_empty());
    // A hoe on leaves is twice the needed rate, yet still waits one tick.
    let leaves = target(
        [2, 2, 2],
        "minecraft:oak_leaves",
        Some("minecraft:golden_hoe"),
    );
    let mut machine = DestroyMachine::default();
    assert_eq!(
        kinds(&held(&mut machine, &leaves, Server)),
        [(StartDestroy, [2, 2, 2], 1)]
    );
    assert_eq!(
        kinds(&held(&mut machine, &leaves, Server)),
        [
            (ContinueDestroy, [2, 2, 2], 1),
            (PredictDestroy, [2, 2, 2], 1)
        ]
    );
    // A rate of at least one skips the post-break delay.
    let below = target(
        [2, 1, 2],
        "minecraft:oak_leaves",
        Some("minecraft:golden_hoe"),
    );
    assert_eq!(
        kinds(&held(&mut machine, &below, Server)),
        [(ContinueDestroy, [2, 1, 2], 1)]
    );
    let mut client = DestroyMachine::default();
    held(&mut client, &leaves, Client);
    let done = held(&mut client, &leaves, Client);
    assert_eq!(kinds(&done), [(StopDestroy, [0, 0, 0], 0)]);
    assert!(done.destroy.is_some());
}

#[test]
fn server_destroys_of_blocks_with_hardness_predict_tool_wear() {
    let worn = |block| DestroyTarget {
        wear: Some(ToolWear {
            current_damage: 4,
            break_damage: 2,
        }),
        ..target([0, 0, 0], block, Some("minecraft:iron_sword"))
    };
    let mut machine = DestroyMachine::default();
    let dirt = worn("minecraft:dirt");
    assert_eq!(held(&mut machine, &dirt, Server).wear, None);
    let done = loop {
        let payload = held(&mut machine, &dirt, Server);
        if !payload.is_empty() {
            break payload;
        }
    };
    assert_eq!(done.wear, Some((2, 6, -1)));
    // Zero-hardness and client-authoritative breaks never wear through this path.
    let torch = worn("minecraft:torch");
    assert_eq!(
        held(&mut DestroyMachine::default(), &torch, Server).wear,
        None
    );
    // A one-tick break of a block with hardness still wears.
    let leaves = DestroyTarget {
        conditions: DestroyConditions {
            tool: HeldTool::from_identifier("minecraft:golden_hoe"),
            ..DestroyConditions::default()
        },
        ..worn("minecraft:oak_leaves")
    };
    let mut quick = DestroyMachine::default();
    held(&mut quick, &leaves, Server);
    assert_eq!(held(&mut quick, &leaves, Server).wear, Some((2, 6, -1)));
    let mut client = DestroyMachine::default();
    held(&mut client, &dirt, Client);
    for _ in 0..20 {
        assert_eq!(held(&mut client, &dirt, Client).wear, None);
    }
}

/// Completion removes the block locally; a server rollback is mined afresh after the delay.
#[test]
fn a_completion_predicts_the_break_and_a_rolled_back_block_is_mined_again() {
    // A golden shovel removes 0.8 of dirt per tick: two held ticks after the start.
    let dirt = target([0, 3, 0], "minecraft:dirt", Some("minecraft:golden_shovel"));
    let completion = [
        (ContinueDestroy, [0, 3, 0], 1),
        (PredictDestroy, [0, 3, 0], 1),
    ];
    let mut machine = DestroyMachine::default();
    assert_eq!(held(&mut machine, &dirt, Server).broken, None);
    assert!(held(&mut machine, &dirt, Server).is_empty());
    let done = held(&mut machine, &dirt, Server);
    assert_eq!(kinds(&done), completion);
    assert_eq!(done.broken, Some([0, 3, 0]));
    assert_eq!(
        machine.destroying_target(),
        None,
        "hit sounds and particles stop with the break"
    );
    for _ in 0..DESTROY_DELAY_TICKS {
        assert!(held(&mut machine, &dirt, Server).is_empty());
    }
    // The server restated the block: no hold, destroying resumes at once.
    assert!(held(&mut machine, &dirt, Server).is_empty());
    assert_eq!(kinds(&held(&mut machine, &dirt, Server)), completion);
    let torch = target([1, 1, 1], "minecraft:torch", None);
    let client = held(&mut DestroyMachine::default(), &torch, Client);
    assert_eq!(
        client.broken,
        Some([1, 1, 1]),
        "client authority predicts too"
    );
}

#[test]
fn interruption_aborts_on_the_next_step_only() {
    let stone = target([7, 7, 7], "minecraft:stone", None);
    let mut machine = DestroyMachine::default();
    held(&mut machine, &stone, Server);
    machine.interrupt();
    assert_eq!(
        kinds(&held(&mut machine, &stone, Server)),
        [(AbortDestroy, [7, 7, 7], 0), (StartDestroy, [7, 7, 7], 1)]
    );
    // Unknown blocks keep a destroy open without ever predicting completion.
    let unknown = target([8, 8, 8], "minecraft:not_a_block", None);
    held(&mut machine, &unknown, Server);
    for _ in 0..1_000 {
        assert!(held(&mut machine, &unknown, Server).is_empty());
    }
}

/// Creative completes on the start tick through the negotiated authority's actions only.
#[test]
fn an_instant_destroy_uses_the_negotiated_completion() {
    let stone = DestroyTarget {
        instant: true,
        ..target([3, 4, 5], "minecraft:obsidian", None)
    };
    let server = held(&mut DestroyMachine::default(), &stone, Server);
    assert_eq!(
        kinds(&server),
        [(StartDestroy, [3, 4, 5], 1), (PredictDestroy, [3, 4, 5], 1)]
    );
    assert_eq!(
        server.destroy, None,
        "no legacy transaction beside PredictDestroy"
    );
    assert_eq!(server.broken, Some([3, 4, 5]));
    assert_eq!(machine_target(&stone), None, "instant destroys never crack");
    let mut machine = DestroyMachine::default();
    let client = held(&mut machine, &stone, Client);
    assert_eq!(
        kinds(&client),
        [(StartDestroy, [3, 4, 5], 1), (StopDestroy, [0, 0, 0], 0)]
    );
    assert!(client.destroy.is_some());
    assert_eq!(
        kinds(&machine.step(DestroyInput::Released, STILL, Client)),
        [(AbortDestroy, [3, 4, 5], 0)]
    );
}

fn machine_target(target: &DestroyTarget) -> Option<([i32; 3], u8)> {
    let mut machine = DestroyMachine::default();
    held(&mut machine, target, Server);
    machine.destroying_target()
}

/// Holding attack in Creative keeps destroying: after the delay when still,
/// per block travelled when moving.
#[test]
fn a_held_instant_destroy_repeats_after_the_delay_or_per_block_travelled() {
    let instant = |position| DestroyTarget {
        instant: true,
        ..target(position, "minecraft:stone", None)
    };
    let mut machine = DestroyMachine::default();
    held(&mut machine, &instant([0, 0, 0]), Server);
    let next = instant([0, -1, 0]);
    for _ in 0..DESTROY_DELAY_TICKS {
        assert!(held(&mut machine, &next, Server).is_empty());
    }
    assert_eq!(
        kinds(&held(&mut machine, &next, Server)),
        [
            (ContinueDestroy, [0, -1, 0], 1),
            (PredictDestroy, [0, -1, 0], 1)
        ]
    );
    // Flying at 6 blocks/s ignores the delay and destroys past each travelled block.
    let flying = TickMotion {
        on_ground: false,
        moved: 0.3,
    };
    let ahead = instant([0, -2, 0]);
    let steps = (0..4)
        .map(|_| {
            !machine
                .step(DestroyInput::Held(Some(&ahead)), flying, Server)
                .is_empty()
        })
        .collect::<Vec<_>>();
    assert_eq!(steps, [false, false, false, true]);
    assert!(
        (machine.travel - 0.2).abs() < 1e-5,
        "the fraction carries over"
    );
}

/// stopDestroyBlock clears the delay, so a fresh press starts at once.
#[test]
fn release_clears_the_destroy_delay() {
    let torch = target([1, 1, 1], "minecraft:torch", None);
    let mut machine = DestroyMachine::default();
    held(&mut machine, &torch, Server);
    machine.step(DestroyInput::Released, STILL, Server);
    assert_eq!(
        kinds(&held(&mut machine, &torch, Server)),
        [(StartDestroy, [1, 1, 1], 1), (PredictDestroy, [1, 1, 1], 1)]
    );
}

/// Local flight reaches the destroy conditions instead of always reading grounded-or-falling.
#[test]
fn local_flight_exempts_the_airborne_penalty() {
    assert!(exempt_from_airborne_penalty(Some(
        sim::MovementMode::Flying
    )));
    assert!(!exempt_from_airborne_penalty(Some(
        sim::MovementMode::Walking
    )));
    assert!(!exempt_from_airborne_penalty(None));
}

/// Unbreaking III damages only a quarter of rolls.
#[test]
fn unbreaking_suppresses_damage_by_the_reference_chance() {
    assert!(unbreaking_keeps_damage(0, 99));
    let kept = (0..100)
        .filter(|roll| unbreaking_keeps_damage(3, *roll))
        .count();
    assert_eq!(kept, 25);
    assert_eq!(
        (0..100)
            .filter(|roll| unbreaking_keeps_damage(1, *roll))
            .count(),
        50
    );
}

#[test]
fn swords_and_the_trident_cannot_destroy_in_creative() {
    assert!(!destroys_in_creative(Some("minecraft:diamond_sword")));
    assert!(!destroys_in_creative(Some("minecraft:trident")));
    assert!(destroys_in_creative(Some("minecraft:diamond_pickaxe")));
    assert!(destroys_in_creative(Some("minecraft:stick")));
    assert!(destroys_in_creative(None));
}

pub(crate) use crate::test_support::survival_mining::{completed, evidence, ticker_with_ticks};

#[test]
fn each_unsent_tick_is_stepped_once() {
    let mut ticker = MovementTicker::default();
    ticker.reset(7, 100, [0.5, 2.620_01, 0.5]);
    ticker.set_source(MovementSource::Physics);
    for tick in 101..=102 {
        ticker.enqueue_completed_physics(completed(tick)).unwrap();
    }
    let stone = target([0, 1, -3], "minecraft:stone", None);
    let mut runtime = SurvivalMiningRuntime::default();
    let mut swings = Vec::new();
    runtime.step_ticks(
        &mut ticker,
        DestroyInput::Held(Some(&stone)),
        Server,
        |tick| swings.push(tick),
        |_, _| None,
        |_| {},
    );
    // Re-running the frame must not step the same ticks again.
    runtime.step_ticks(
        &mut ticker,
        DestroyInput::Released,
        Server,
        |_| {},
        |_, _| None,
        |_| {},
    );
    ticker.enqueue_completed_physics(completed(103)).unwrap();
    runtime.step_ticks(
        &mut ticker,
        DestroyInput::Released,
        Server,
        |tick| swings.push(tick),
        |_, _| None,
        |_| {},
    );
    assert_eq!(
        swings,
        [101, 102],
        "each held tick on a block attempts a swing"
    );

    let mut packets = Vec::new();
    flush_player_auth_inputs(&mut ticker, 8, Some(evidence()), |_, packet| {
        packets.push(packet);
        Ok::<_, ()>(())
    })
    .unwrap();
    let carries_actions = packets
        .iter()
        .map(|packet| {
            protocol::player_auth_input_trace_sample(packet)
                .unwrap()
                .flag_names
                .contains(&"PerformBlockActions")
        })
        .collect::<Vec<_>>();
    // Start on the first tick, silence while held, abort once released.
    assert_eq!(carries_actions, [true, false, true]);
}

#[test]
fn a_worn_tool_completion_carries_the_mine_block_request_on_its_tick() {
    let mut ticker = MovementTicker::default();
    ticker.reset(7, 100, [0.5, 2.620_01, 0.5]);
    ticker.set_source(MovementSource::Physics);
    let stack = NetworkItemStack {
        network_id: 5,
        stack_network_id: 41,
        count: 1,
        ..NetworkItemStack::empty()
    };
    let dirt = DestroyTarget {
        selection: FrozenMiningSelection {
            slot: 2,
            item: VerifiedNetworkItemStack::try_new(stack.clone(), stack.nbt_digest).unwrap(),
        },
        wear: Some(ToolWear {
            current_damage: 3,
            break_damage: 1,
        }),
        ..target(
            [0, 1, -3],
            "minecraft:dirt",
            Some("minecraft:golden_shovel"),
        )
    };
    let mut runtime = SurvivalMiningRuntime::default();
    let mut ids = [-7, -9].into_iter();
    let mut broken = Vec::new();
    for tick in 101..=103 {
        ticker.enqueue_completed_physics(completed(tick)).unwrap();
        runtime.step_ticks(
            &mut ticker,
            DestroyInput::Held(Some(&dirt)),
            Server,
            |_| {},
            |_, _| ids.next(),
            |position| broken.push(position),
        );
    }
    let mut packets = Vec::new();
    flush_player_auth_inputs(&mut ticker, 8, Some(evidence()), |_, packet| {
        packets.push(packet);
        Ok::<_, ()>(())
    })
    .unwrap();
    let requests = packets
        .iter()
        .map(|packet| {
            protocol::player_auth_input_trace_sample(packet)
                .unwrap()
                .flag_names
                .contains(&"PerformItemStackRequest")
        })
        .collect::<Vec<_>>();
    // Start, crack, then completion with the first allocated id.
    assert_eq!(requests, [false, false, true]);
    assert_eq!(ids.next(), Some(-9), "only the completion allocates an id");
    assert_eq!(
        broken,
        [[0, 1, -3]],
        "the carried completion is predicted once"
    );
}

mod gate {
    use super::super::{blocked_mining_reason, mining_active};
    use client_world::game_mode_capabilities::GameModeCapabilities;
    use protocol::PlayerGameMode::{Adventure, Creative, Spectator, Survival};

    /// The regression: with no negotiated wire mode the gate must still mine.
    /// Authority is not even an input to the gate.
    #[test]
    fn survival_mining_runs_regardless_of_wire_authority() {
        let survival = Some(GameModeCapabilities::for_mode(Survival));
        assert!(
            mining_active(survival, true, true),
            "survival with a focused window and an input snapshot must mine"
        );
    }

    #[test]
    fn gate_requires_edit_focus_and_a_snapshot() {
        let survival = Some(GameModeCapabilities::for_mode(Survival));
        assert!(!mining_active(survival, false, true), "unfocused");
        assert!(!mining_active(survival, true, false), "no snapshot");
        assert!(!mining_active(None, true, true), "no game mode");
        assert!(
            mining_active(Some(GameModeCapabilities::for_mode(Creative)), true, true),
            "creative mines through the same machine"
        );
        assert!(
            !mining_active(Some(GameModeCapabilities::for_mode(Adventure)), true, true),
            "adventure cannot edit without a server grant"
        );
        assert!(!mining_active(
            Some(GameModeCapabilities::for_mode(Spectator)),
            true,
            true
        ));
    }

    #[test]
    fn adventure_with_build_grant_mines() {
        let mut caps = GameModeCapabilities::for_mode(Adventure);
        caps.can_mine = true;
        assert!(mining_active(Some(caps), true, true));
    }

    #[test]
    fn blocked_reason_names_the_first_failing_gate() {
        let survival = Some(GameModeCapabilities::for_mode(Survival));
        assert_eq!(
            blocked_mining_reason(None, true, true, false, false),
            Some("game mode unknown")
        );
        assert_eq!(
            blocked_mining_reason(
                Some(GameModeCapabilities::for_mode(Spectator)),
                true,
                true,
                false,
                false
            ),
            Some("can_mine=false for this game mode")
        );
        assert_eq!(
            blocked_mining_reason(survival, false, true, false, false),
            Some("window or menu not focused")
        );
        assert_eq!(
            blocked_mining_reason(survival, true, false, false, false),
            Some("no input snapshot yet")
        );
        assert_eq!(
            blocked_mining_reason(survival, true, true, true, false),
            Some("an actor in front owns the press")
        );
        assert_eq!(
            blocked_mining_reason(survival, true, true, false, false),
            Some("no breakable block in reach")
        );
        assert_eq!(
            blocked_mining_reason(survival, true, true, false, true),
            None
        );
    }
}

#[test]
fn review_frozen_mining_target_completes_once_per_catchup_batch() {
    let mut ticker = ticker_with_ticks(3);
    let mut runtime = SurvivalMiningRuntime::default();
    let mut target = target([1, 2, 3], "minecraft:dirt", None);
    target.block.as_mut().unwrap().hardness = 0.01;
    let mut completions = 0;
    runtime.step_ticks(
        &mut ticker,
        DestroyInput::Held(Some(&target)),
        Client,
        |_| {},
        |_, _| None,
        |_| completions += 1,
    );
    assert_eq!(completions, 1);
}

/// First-hit destroys publish a local effect without an intermediate cracking frame.
#[test]
fn instant_plants_publish_the_carried_destroy_effect_once() {
    for authority in [Server, Client] {
        for name in [
            "minecraft:short_grass",
            "minecraft:tall_grass",
            "minecraft:dandelion",
            "minecraft:torch",
        ] {
            let mut runtime = SurvivalMiningRuntime::default();
            let mut ticker = ticker_with_ticks(1);
            let plant = target([1, 2, 3], name, None);
            let payload = held(&mut DestroyMachine::default(), &plant, authority);
            assert_eq!(payload.wear, None);
            let (interactions, _) = payload.into_interactions([0.0; 3]);
            match authority {
                Server => {
                    assert_eq!(
                        interactions
                            .block_actions
                            .iter()
                            .map(|action| action.kind)
                            .collect::<Vec<_>>(),
                        [StartDestroy, PredictDestroy]
                    );
                    assert!(interactions.block_interaction.is_none());
                }
                Client => {
                    assert_eq!(
                        interactions
                            .block_actions
                            .iter()
                            .map(|action| action.kind)
                            .collect::<Vec<_>>(),
                        [StartDestroy, StopDestroy]
                    );
                    assert!(matches!(
                        interactions.block_interaction,
                        Some(protocol::BlockItemInteraction::Destroy(_))
                    ));
                }
            }
            let mut predictions = Vec::new();
            runtime.step_ticks(
                &mut ticker,
                DestroyInput::Held(Some(&plant)),
                authority,
                |_| {},
                |_, _| None,
                |cell| predictions.push(cell),
            );
            assert_eq!(predictions, [[1, 2, 3]]);
            assert_eq!(
                runtime.destroying_target(),
                None,
                "no crack/hit interval for instant breaks"
            );
            assert_eq!(
                runtime.take_break_cues(),
                [BlockBreakCue::Break {
                    position: [1, 2, 3],
                    block_runtime_id: plant.runtime_id as i32,
                }]
            );
            assert!(runtime.take_break_cues().is_empty());
        }
    }
}
