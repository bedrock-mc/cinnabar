//! Actual guest calls cover registered actors, updates, seating and typed steering.
use crate::common;
use experience_runtime::{
    callback::run,
    load::{engine, load},
    protocol::*,
};

/// Builds an entity callback scoped to a live instance opening and its owned actors.
fn request(call: Call, actor: Option<String>, entities: Vec<EntitySnapshot>) -> Request {
    let world = WorldHandle {
        id: "probe:entities".into(),
        generation: 7,
    };
    Request::Callback {
        seq: 2,
        info: Info {
            context: Some(WorldContext {
                current: world.clone(),
                player: None,
                instances: vec![world],
                stores: Vec::new(),
                entities,
            }),
            simulation: Some(SimulationContext {
                step: SimulationStep {
                    tick: 1,
                    duration_nanos: experience_runtime::limits::SIMULATION_STEP_NANOS,
                },
                sequence: 2,
                schedules: Vec::new(),
            }),
            world_id: "world".into(),
            dimension_id: "overworld".into(),
            tick: 1,
            event_sequence: 2,
        },
        actor,
        world_min_y: -64,
        world_max_y: 319,
        data_budget: 4096,
        snapshot: Vec::new(),
        network: None,
        inventory: None,
        call,
    }
}

#[test]
fn guest_entities_register_spawn_move_mount_and_steer() {
    let dir = common::probe_dir();
    let (engine, _ticker) = engine().unwrap();
    let loaded = load(&engine, dir.path()).unwrap();
    assert_eq!(loaded.entities.len(), 1);
    let step = SimulationStep {
        tick: 1,
        duration_nanos: experience_runtime::limits::SIMULATION_STEP_NANOS,
    };
    let Outcome::Committed { ops } = run(
        &engine,
        &loaded,
        &request(Call::Tick { step }, None, Vec::new()),
    ) else {
        panic!("entity tick failed")
    };
    let [
        Op::Entity {
            action: EntityOp::Spawn { state: spawned },
        },
        Op::Entity {
            action:
                EntityOp::Update {
                    state,
                    teleport: false,
                },
        },
    ] = ops.as_slice()
    else {
        panic!("spawn and update missing: {ops:?}")
    };
    assert_eq!(state.position.x, spawned.position.x + 0.25);
    assert_eq!(state.handle.generation, 7);
    let snapshot = EntitySnapshot {
        state: state.clone(),
        revision: 1,
        riders: Vec::new(),
    };
    let handle = state.handle;
    let event = EntityEvent::Interact {
        player: common::ACTOR.into(),
        hand: EntityHand::Main,
        item: None,
    };
    let outcome = run(
        &engine,
        &loaded,
        &request(
            Call::Entity { handle, event },
            Some(common::ACTOR.into()),
            vec![snapshot.clone()],
        ),
    );
    assert_eq!(
        outcome,
        Outcome::Committed {
            ops: vec![Op::Entity {
                action: EntityOp::Mount { handle, seat: 0 }
            }]
        }
    );
    let event = EntityEvent::Steering {
        input: EntityInput {
            rider: EntityRider {
                player: common::ACTOR.into(),
                seat: 0,
            },
            sideways: 0.5,
            forward: -0.25,
            yaw: 20.0,
            pitch: 0.0,
            jump: false,
            sneak: false,
        },
    };
    let Outcome::Committed { ops } = run(
        &engine,
        &loaded,
        &request(
            Call::Entity { handle, event },
            Some(common::ACTOR.into()),
            vec![snapshot],
        ),
    ) else {
        panic!("steering failed")
    };
    let [
        Op::Entity {
            action: EntityOp::Update { state, .. },
        },
    ] = ops.as_slice()
    else {
        panic!("steering update missing")
    };
    assert_eq!(state.velocity.x, 0.5);
    assert_eq!(state.velocity.z, -0.25);
    let steered = state.clone();
    let Outcome::Committed { ops } = run(
        &engine,
        &loaded,
        &request(
            Call::Tick { step },
            None,
            vec![EntitySnapshot {
                state: steered.clone(),
                revision: 2,
                riders: Vec::new(),
            }],
        ),
    ) else {
        panic!("steered tick failed")
    };
    let [
        Op::Entity {
            action: EntityOp::Update { state, .. },
        },
    ] = ops.as_slice()
    else {
        panic!("steered movement missing")
    };
    assert_eq!(state.position.x, steered.position.x + 0.5);
    assert_eq!(state.position.z, steered.position.z - 0.25);
    let outcome = run(
        &engine,
        &loaded,
        &request(
            Call::Entity {
                handle,
                event: EntityEvent::Despawned {},
            },
            None,
            Vec::new(),
        ),
    );
    assert_eq!(outcome, Outcome::Committed { ops: Vec::new() });
}

#[test]
fn frozen_v0_9_artifact_keeps_its_export_contract() {
    let dir = common::v0_9_dir();
    let (engine, _ticker) = engine().unwrap();
    let loaded = load(&engine, dir.path()).unwrap();
    assert!(loaded.entities.is_empty());
    let step = SimulationStep {
        tick: 1,
        duration_nanos: experience_runtime::limits::SIMULATION_STEP_NANOS,
    };
    assert_eq!(
        run(
            &engine,
            &loaded,
            &request(Call::Tick { step }, None, Vec::new())
        ),
        Outcome::Committed { ops: Vec::new() }
    );
}
