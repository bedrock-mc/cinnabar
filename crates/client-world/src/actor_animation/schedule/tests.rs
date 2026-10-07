use super::*;
use crate::ActorPose;

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

fn spawn(rng: &mut Rng, runtime_id: u64, identifiers: &[Box<str>]) -> ActorSnapshot {
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
    actor.previous_pose.position = actor.position;
    actor
}

/// Walks, turns and re-flags every actor, as a tick of movement packets would.
fn wander(rng: &mut Rng, actors: &mut HashMap<u64, ActorSnapshot>) {
    let mut ids: Vec<u64> = actors.keys().copied().collect();
    ids.sort_unstable();
    for id in ids {
        let actor = actors.get_mut(&id).unwrap();
        actor.previous_pose = ActorPose {
            position: actor.position,
            pitch: actor.pitch,
            yaw: actor.yaw,
            head_yaw: actor.head_yaw,
        };
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
        }
    }
}

/// Every rig's full retained state plus the store-wide counters.
fn fingerprint(store: &ActorAnimationStore) -> String {
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
                    serial.start_swing(id, 6);
                    parallel.start_swing(id, 6);
                }
            }
        }
        let view = (tick % 3 != 0).then_some(&view);
        let evaluate = tick % 5 != 4;
        let reset_history = tick % 7 == 0;
        let first_person = tick % 4 < 2;
        let context = |actor: &ActorSnapshot| ActorTickContext {
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
    let assets = super::super::render_frame::tests::counting_random_assets();
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
