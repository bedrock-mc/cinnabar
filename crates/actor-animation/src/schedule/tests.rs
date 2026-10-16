use super::*;

/// Deterministic xorshift so both stores see the same crowd.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, bound: u64) -> u64 {
        self.next() % bound
    }

    fn unit(&mut self) -> f32 {
        (self.next() >> 40) as f32 / (1u64 << 24) as f32
    }
}

const LOCAL: u64 = 1;
/// Metadata flags a crowd toggles: on fire, sneaking, sprinting, using item, baby, swimming.
const FLAGS: [u32; 6] = [0, 1, 3, 4, 11, 37];

fn spawn(rng: &mut Rng, runtime_id: u64, identifiers: &[Box<str>]) -> TestActor {
    let mut actor = super::super::tests::actor_with_metadata(HashMap::new());
    actor.runtime_id = runtime_id;
    actor.unique_id = runtime_id as i64;
    let player = runtime_id == LOCAL || rng.below(4) == 0;
    let identifier = &identifiers[rng.below(identifiers.len() as u64) as usize];
    actor.kind = if !player {
        ActorKind::Entity {
            identifier: Arc::from(&**identifier),
        }
    } else {
        ActorKind::Player {
            uuid: runtime_id.to_le_bytes().repeat(2).try_into().unwrap(),
            username: "crowd".into(),
        }
    };
    actor.position = [rng.unit() * 40.0 - 20.0, 64.0, rng.unit() * 40.0 - 20.0];
    actor.previous_position = actor.position;
    actor
}

/// Walks, turns and re-flags every actor, as a tick of movement packets would.
fn wander(rng: &mut Rng, actors: &mut HashMap<u64, TestActor>) {
    let mut ids: Vec<u64> = actors.keys().copied().collect();
    ids.sort_unstable();
    for id in ids {
        let actor = actors.get_mut(&id).unwrap();
        actor.previous_position = actor.position;
        for axis in [0, 2] {
            actor.position[axis] += rng.unit() * 0.6 - 0.3;
        }
        actor.velocity = [rng.unit() - 0.5, 0.0, rng.unit() - 0.5];
        actor.yaw = (actor.yaw + rng.unit() * 40.0 - 20.0).rem_euclid(360.0);
        actor.head_yaw = (actor.yaw + rng.unit() * 60.0 - 30.0).rem_euclid(360.0);
        actor.pitch = rng.unit() * 60.0 - 30.0;
        actor.on_ground = Some(rng.below(8) != 0);
        if rng.below(10) == 0 {
            let flags = FLAGS
                .iter()
                .filter(|_| rng.below(4) == 0)
                .fold(0u64, |flags, bit| flags | 1 << bit);
            actor.metadata.insert(0, ActorMetadataValue::Flags(flags));
            actor.status.on_fire = flags & 1 != 0;
        }
    }
}

/// Every rig's full retained state plus the store-wide counters.
pub(super) fn fingerprint(store: &ActorAnimationStore) -> String {
    format!(
        "{:?}\n{:?} {:?} {} {}",
        store.rigs,
        store.stats,
        store.first_starved,
        store.next_reset_generation,
        store.next_rest_reset_generation
    )
}

/// Runs one crowd through a serial and a parallel store, requiring identical state every tick.
fn assert_parallel_matches_serial(
    assets: Arc<RuntimeEntityAssets>,
    identifiers: &[Box<str>],
    count: u64,
    ticks: u64,
    world_budget: usize,
) -> ActorAnimationStats {
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15 ^ count);
    let mut serial = ActorAnimationStore::with_assets(Arc::clone(&assets));
    let mut parallel = ActorAnimationStore::with_assets(assets);
    serial.schedule = TestSchedule {
        serial: true,
        world_budget,
    };
    parallel.schedule.world_budget = world_budget;
    let mut actors = HashMap::new();
    for runtime_id in 1..=count {
        let actor = spawn(&mut rng, runtime_id, identifiers);
        serial.insert(1, 0, &actor);
        parallel.insert(1, 0, &actor);
        actors.insert(runtime_id, actor);
    }
    let view = ActorAnimationView {
        planes: [[-1.0, 0.0, 0.0, 8.0]; 6],
        camera: [0.0, 64.0, 0.0],
        player_distance: 64.0,
        entity_radius: 72.0,
    };
    let sword: Arc<str> = Arc::from("minecraft:diamond_sword");
    for tick in 0..ticks {
        wander(&mut rng, &mut actors);
        for _ in 0..3 {
            let id = 1 + rng.below(count);
            match rng.below(3) {
                0 => {
                    serial.mark_reset(id);
                    parallel.mark_reset(id);
                }
                _ => {
                    serial.start_swing(id, 6, 6);
                    parallel.start_swing(id, 6, 6);
                }
            }
        }
        let view = (tick % 3 != 0).then_some(&view);
        let evaluate = tick % 5 != 4;
        let reset_history = tick % 7 == 0;
        let first_person = tick % 4 < 2;
        let context = |actor: &TestActor| ActorTickContext {
            is_local_first_person: first_person && actor.runtime_id == LOCAL,
            main_hand: (actor.runtime_id % 3 == tick % 3).then(|| Arc::clone(&sword)),
            camera_position: [0.0, 64.0, 0.0],
            ..ActorTickContext::default()
        };
        for store in [&mut serial, &mut parallel] {
            store.advance_tick(&actors, view, Some(LOCAL), evaluate, reset_history, context);
            if tick % 6 == 5 {
                store.refresh_local_view(&actors, LOCAL, context);
            }
        }
        assert_eq!(fingerprint(&serial), fingerprint(&parallel), "tick {tick}");
    }
    parallel.stats()
}

fn installed_entity_assets() -> Option<Arc<RuntimeEntityAssets>> {
    let root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.local/assets/compiled");
    let Ok(entries) = std::fs::read_dir(&root) else {
        eprintln!(
            "skipping vanilla crowd schedule test: missing entity fixture directory {}",
            root.display()
        );
        return None;
    };
    let mut paths: Vec<_> = entries
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "mcbeent")
        })
        .collect();
    paths.sort();
    let Some(path) = paths.first() else {
        eprintln!(
            "skipping vanilla crowd schedule test: missing entity fixture {}/*.mcbeent",
            root.display()
        );
        return None;
    };
    Some(Arc::new(
        RuntimeEntityAssets::decode(&std::fs::read(path).unwrap()).unwrap(),
    ))
}

/// Random vanilla rigs evaluate in parallel to the exact serial state, with and without the
/// world budget running out mid-tick.
#[test]
fn parallel_vanilla_crowd_matches_serial_evaluation() {
    let Some(assets) = installed_entity_assets() else {
        return;
    };
    let identifiers: Vec<Box<str>> = assets
        .rig_bindings()
        .iter()
        .map(|rig| {
            assets.symbols()[rig.entity_symbol as usize]
                .identifier
                .clone()
        })
        .filter(|identifier| &**identifier != "minecraft:player")
        .collect();
    assert!(identifiers.len() > 50, "{} entity rigs", identifiers.len());
    let open = assert_parallel_matches_serial(
        Arc::clone(&assets),
        &identifiers,
        120,
        30,
        MAX_MOLANG_OPS_PER_WORLD_TICK,
    );
    assert_eq!(open.world_budget_exhaustions, 0);
    assert!(open.evaluated_molang_ops > 0);
    let starved = assert_parallel_matches_serial(
        assets,
        &identifiers,
        240,
        30,
        3 * MAX_MOLANG_OPS_PER_ACTOR_TICK + 777,
    );
    assert!(starved.world_budget_exhaustions > 0, "{starved:?}");
}

/// The synthetic counting rig needs no installed carrier, so this runs everywhere.
#[test]
fn parallel_synthetic_crowd_matches_serial_through_world_budget_exhaustion() {
    let assets = crate::test_support::counting_random_assets();
    let identifiers = [Box::from("minecraft:test")];
    let open = assert_parallel_matches_serial(
        Arc::clone(&assets),
        &identifiers,
        300,
        20,
        MAX_MOLANG_OPS_PER_WORLD_TICK,
    );
    assert_eq!(open.world_budget_exhaustions, 0);
    let starved = assert_parallel_matches_serial(
        assets,
        &identifiers,
        1500,
        12,
        2 * MAX_MOLANG_OPS_PER_ACTOR_TICK + 500,
    );
    assert!(starved.world_budget_exhaustions > 0, "{starved:?}");
}

mod refresh_tests;

/// A rig whose first bone's X translation is the negated keyframe value, after an optional
/// pre-animation loop of `pre_animation` iterations.
pub(super) fn rig_assets(
    symbols: &[(assets::MolangSymbolKind, &str)],
    keyframe: Vec<assets::MolangOp>,
    keyframe_stack: u8,
    pre_animation: Option<f32>,
) -> Arc<RuntimeEntityAssets> {
    use assets::{CompiledMolangExpression, MolangOp, MolangSymbol, MolangSymbolKind};
    let scalar = crate::test_support::scalar;
    let mut compiled = crate::test_support::counting_random_compiled("minecraft:test");
    compiled.rig_bindings[0].initialize = None;
    compiled.render.layers[0].color = None;
    compiled.molang_symbols = std::iter::once((MolangSymbolKind::Name, "wield"))
        .chain(symbols.iter().copied())
        .map(|(kind, identifier)| MolangSymbol {
            kind,
            identifier: identifier.into(),
        })
        .collect();
    let length = keyframe.len() as u32;
    let mut ops = vec![MolangOp::Push(scalar(0.0))];
    ops.extend(keyframe);
    ops.extend([MolangOp::Push(scalar(0.0)), MolangOp::Push(scalar(0.0))]);
    let mut expressions = vec![
        (0, 1, 1),
        (1, length, keyframe_stack),
        (1 + length, 1, 1),
        (2 + length, 1, 1),
    ];
    if let Some(iterations) = pre_animation {
        ops.extend([
            MolangOp::Push(scalar(iterations)),
            MolangOp::LoopStart(3),
            MolangOp::LoopNext(2),
            MolangOp::Push(scalar(0.0)),
        ]);
        expressions.push((3 + length, 4, 1));
        compiled.rig_bindings[0].pre_animation = Some(4);
    }
    compiled.molang_ops = ops.into();
    compiled.molang_expressions = expressions
        .into_iter()
        .map(|(first_op, op_count, max_stack)| CompiledMolangExpression {
            first_op,
            op_count: op_count as u16,
            max_stack,
        })
        .collect();
    Arc::new(RuntimeEntityAssets::from_compiled(compiled.with_unresolved_admissions()).unwrap())
}

/// A rig whose first bone translates by one unit per tick of actor age.
pub(super) fn clock_assets(pre_animation: Option<f32>) -> Arc<RuntimeEntityAssets> {
    use assets::MolangOp;
    rig_assets(
        &[(assets::MolangSymbolKind::Query, "query.life_time")],
        vec![
            MolangOp::LoadQuery(1),
            MolangOp::Push(crate::test_support::scalar(20.0)),
            MolangOp::Multiply,
        ],
        2,
        pre_animation,
    )
}

/// The view `x <= 10`, with generous distances.
pub(super) fn near_view() -> ActorAnimationView {
    ActorAnimationView {
        planes: [[-1.0, 0.0, 0.0, 10.0]; 6],
        camera: [0.0; 3],
        player_distance: 100.0,
        entity_radius: 72.0,
    }
}

/// A rig re-entering view blends from its previous tick's pose, as one that never left does.
#[test]
fn a_rig_back_in_view_blends_from_its_previous_tick() {
    let actor = super::super::tests::actor_with_metadata(HashMap::new());
    let actors = HashMap::from([(actor.runtime_id, actor.clone())]);
    let away = ActorAnimationView {
        planes: [[-1.0, 0.0, 0.0, -100.0]; 6],
        camera: [0.0; 3],
        player_distance: 100.0,
        entity_radius: 72.0,
    };
    let (mut visible, mut returning) = (
        ActorAnimationStore::with_assets(clock_assets(None)),
        ActorAnimationStore::with_assets(clock_assets(None)),
    );
    for store in [&mut visible, &mut returning] {
        store.insert(1, 0, &actor);
    }
    // Ticks 2 to 5 are out of view; tick 6 is the first visible one again.
    for tick in 0..7 {
        let view = (2..6).contains(&tick).then_some(&away);
        visible.advance_tick(&actors, None, None, true, true, |_| {
            ActorTickContext::default()
        });
        returning.advance_tick(&actors, view, None, true, true, |_| {
            ActorTickContext::default()
        });
    }
    let (expected, actual) = (
        visible.get(actor.runtime_id).unwrap(),
        returning.get(actor.runtime_id).unwrap(),
    );
    assert_eq!(actual.current, expected.current);
    assert_ne!(
        actual.previous, actual.current,
        "the first visible tick blends"
    );
    assert_eq!(actual.previous, expected.previous);
}

/// Returning rigs' warm-ups cannot spend the world budget that visible rigs need.
#[test]
fn returning_rig_warm_ups_leave_visible_rigs_their_budget() {
    let assets = clock_assets(Some(1_000.0));
    let spawn = |runtime_id: u64, x: f32| {
        let mut actor = super::super::tests::actor_with_metadata(HashMap::new());
        actor.runtime_id = runtime_id;
        actor.unique_id = runtime_id as i64;
        actor.position = [x, 0.0, 0.0];
        actor.previous_position = actor.position;
        actor
    };
    let cost = {
        let actors = HashMap::from([(1, spawn(1, 0.0))]);
        let mut store = ActorAnimationStore::with_assets(Arc::clone(&assets));
        store.insert(1, 0, &actors[&1]);
        store.advance_tick(&actors, None, None, true, true, |_| {
            ActorTickContext::default()
        });
        let before = store.stats().evaluated_molang_ops;
        store.advance_tick(&actors, None, None, true, true, |_| {
            ActorTickContext::default()
        });
        (store.stats().evaluated_molang_ops - before) as usize
    };
    // The returning rigs' own evaluations fit the world budget, but not twice over.
    let returning = (MAX_MOLANG_OPS_PER_WORLD_TICK * 3 / 5).div_ceil(cost) as u64;
    let visible = returning + 1;
    let actors: HashMap<u64, TestActor> = (1..=returning)
        .map(|id| (id, spawn(id, 50.0)))
        .chain([(visible, spawn(visible, 0.0))])
        .collect();
    let mut store = ActorAnimationStore::with_assets(assets);
    for id in 1..=visible {
        store.insert(1, 0, &actors[&id]);
    }
    let near = near_view();
    let mut drawn = Vec::new();
    for tick in 0..7 {
        let view = (2..4).contains(&tick).then_some(&near);
        store.advance_tick(&actors, view, None, true, true, |_| {
            ActorTickContext::default()
        });
        drawn.push(store.get(visible).unwrap().current[0].translation_scale[0]);
    }
    for (tick, pair) in drawn.windows(2).enumerate() {
        assert_ne!(
            pair[0],
            pair[1],
            "the visible rig starved on tick {}",
            tick + 1
        );
    }
}

/// A returning rig's warm-up and tick evaluation advance script time by the frame's ticks once.
#[test]
fn a_returning_rig_accumulates_frame_time_once() {
    use assets::{MolangOp, MolangSymbolKind};
    let assets = rig_assets(
        &[
            (MolangSymbolKind::Query, "query.delta_time"),
            (MolangSymbolKind::Variable, "variable.t"),
        ],
        vec![
            MolangOp::LoadVariable(2),
            MolangOp::LoadQuery(1),
            MolangOp::Add,
            MolangOp::StoreVariable(2),
            MolangOp::LoadVariable(2),
            MolangOp::Push(crate::test_support::scalar(20.0)),
            MolangOp::Multiply,
        ],
        2,
        None,
    );
    let mut actor = super::super::tests::actor_with_metadata(HashMap::new());
    actor.position = [50.0, 0.0, 0.0];
    actor.previous_position = actor.position;
    let actors = HashMap::from([(actor.runtime_id, actor.clone())]);
    let mut store = ActorAnimationStore::with_assets(assets);
    store.insert(1, 0, &actor);
    let near = near_view();
    let mut before = 0.0;
    for tick in 0..7 {
        let view = (2..6).contains(&tick).then_some(&near);
        let elapsed = (tick == 6).then_some(4);
        store.advance_tick(&actors, view, None, true, true, |_| ActorTickContext {
            animation_elapsed_ticks: elapsed,
            ..Default::default()
        });
        if tick == 1 {
            before = store.get(actor.runtime_id).unwrap().current[0].translation_scale[0];
        }
    }
    let rig = store.get(actor.runtime_id).unwrap();
    let [previous, current] =
        [rig.previous[0], rig.current[0]].map(|bone| bone.translation_scale[0] - before);
    assert!((current + 4.0).abs() < 1.0e-4, "advanced {current} ticks");
    assert!(
        (previous + 3.0).abs() < 1.0e-4,
        "warm-up advanced {previous} ticks"
    );
}
