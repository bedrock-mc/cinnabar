use sha2::{Digest, Sha256};
use sim::{
    Aabb, CollisionQuery, CollisionWorld, ConformanceError, MovementInput, PlayerState, Simulator,
    TickResult, TraceRecord, Vec3, WorldQueryError, verify_scenario_trace_jsonl,
    verify_trace_jsonl,
};

struct Floor;

impl CollisionWorld for Floor {
    fn collision_boxes(&self, query: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        let floor = Aabb::new(Vec3::new(-16.0, 0.0, -16.0), Vec3::new(16.0, 1.0, 16.0));
        Ok(CollisionQuery::synthetic(
            floor
                .intersects(query)
                .then_some(floor)
                .into_iter()
                .collect(),
        ))
    }
}

fn initial_state() -> PlayerState {
    let mut state = PlayerState::new(Vec3::new(0.0, 1.0, 0.0));
    state.on_ground = true;
    state
}

#[test]
fn canonical_jsonl_round_trips_and_replays_one_record_per_tick() {
    let input = MovementInput {
        forward: 1.0,
        ..MovementInput::default()
    };
    let mut expected_state = initial_state();
    let expected = Simulator::default()
        .tick(&mut expected_state, input, &Floor)
        .unwrap();
    let record = TraceRecord { input, expected };
    let jsonl = format!("{}\n", serde_json::to_string(&record).unwrap());
    let encoded: serde_json::Value = serde_json::from_str(jsonl.trim()).unwrap();
    assert!(encoded["expected"].get("environment").is_some());
    assert!(encoded["expected"].get("world_identity").is_some());

    let replayed = verify_trace_jsonl(
        &jsonl,
        initial_state(),
        &Simulator::default(),
        &Floor,
        1.0e-12,
    )
    .unwrap();
    assert_eq!(replayed, expected_state);
    assert_eq!(
        serde_json::from_str::<TraceRecord>(jsonl.trim()).unwrap(),
        record
    );
}

#[test]
fn trace_mismatch_names_the_one_based_line_tick_and_field() {
    let record = TraceRecord {
        input: MovementInput::default(),
        expected: TickResult {
            tick: 1,
            position: Vec3::new(0.25, 1.0, 0.0),
            velocity: Vec3::new(0.0, -0.0784, 0.0),
            movement: Vec3::ZERO,
            collisions: Default::default(),
            on_ground: true,
            environment: Default::default(),
            world_identity: CollisionQuery::synthetic(()).identity,
        },
    };
    let jsonl = format!("{}\n", serde_json::to_string(&record).unwrap());

    assert!(matches!(
        verify_trace_jsonl(
            &jsonl,
            initial_state(),
            &Simulator::default(),
            &Floor,
            1.0e-12,
        ),
        Err(ConformanceError::Mismatch {
            line: 1,
            tick: 1,
            field: "position.x",
            ..
        })
    ));
}

#[test]
fn malformed_blank_and_non_contiguous_records_fail_before_claiming_parity() {
    assert!(matches!(
        verify_trace_jsonl(
            "{}\n",
            initial_state(),
            &Simulator::default(),
            &Floor,
            1.0e-12,
        ),
        Err(ConformanceError::Json { line: 1, .. })
    ));
    assert!(matches!(
        verify_trace_jsonl(
            "\n",
            initial_state(),
            &Simulator::default(),
            &Floor,
            1.0e-12,
        ),
        Err(ConformanceError::BlankLine { line: 1 })
    ));
}

#[test]
fn nested_unknown_fields_are_rejected_recursively() {
    let input = MovementInput::default();
    let mut state = initial_state();
    let expected = Simulator::default()
        .tick(&mut state, input, &Floor)
        .unwrap();
    let canonical = serde_json::to_value(TraceRecord { input, expected }).unwrap();

    for path in [
        &["input"][..],
        &["expected"][..],
        &["expected", "position"][..],
        &["expected", "collisions"][..],
    ] {
        let mut record = canonical.clone();
        let mut target = &mut record;
        for segment in path {
            target = target.get_mut(*segment).unwrap();
        }
        target
            .as_object_mut()
            .unwrap()
            .insert("unknown".to_owned(), serde_json::Value::Bool(true));
        assert!(serde_json::from_value::<TraceRecord>(record).is_err());
    }
}

#[test]
fn pinned_bedsim_v0_1_3_walk_sprint_jump_trace_matches() {
    let replayed = verify_trace_jsonl(
        include_str!("../fixtures/bedsim-v0.1.3-basic.jsonl"),
        initial_state(),
        &Simulator::default(),
        &Floor,
        1.0e-12,
    )
    .unwrap();

    assert_eq!(replayed.tick, 5);
    assert!((replayed.position.y - 2.001_336).abs() <= 1.0e-12);
    assert!((replayed.position.z - 1.155_599_523_633_092_5).abs() <= 1.0e-12);
}

#[test]
fn pinned_trace_provenance_binds_module_commit_sum_generator_and_exact_bytes() {
    let trace = include_bytes!("../fixtures/bedsim-v0.1.3-basic.jsonl");
    let provenance: serde_json::Value = serde_json::from_str(include_str!(
        "../fixtures/bedsim-v0.1.3-basic.provenance.json"
    ))
    .unwrap();

    assert_eq!(provenance["module"], "github.com/oomph-ac/bedsim");
    assert_eq!(provenance["version"], "v0.1.3");
    assert_eq!(
        provenance["source_commit"],
        "5be9149df14e30c0ab14f9e01d51dd2acfee5230"
    );
    assert_eq!(
        provenance["module_sum"],
        "h1:tWZ7O48DL/SaWIY+0zz0hFln+DXN4vfatqKr8zTHVo8="
    );
    assert_eq!(provenance["generator"], "tools/bedsimtrace");
    assert_eq!(
        format!("{:x}", Sha256::digest(trace)),
        provenance["sha256"].as_str().unwrap()
    );
}

#[test]
fn terrain_trace_matches_complete_pinned_ticks_and_binds_provenance() {
    let trace = include_str!("../fixtures/bedsim-v0.1.3-terrain.jsonl");
    assert_eq!(
        verify_scenario_trace_jsonl(trace, &Simulator::default(), 1.0e-12).unwrap(),
        27
    );

    let provenance: serde_json::Value = serde_json::from_str(include_str!(
        "../fixtures/bedsim-v0.1.3-terrain.provenance.json"
    ))
    .unwrap();
    assert_eq!(provenance["module"], "github.com/oomph-ac/bedsim");
    assert_eq!(provenance["version"], "v0.1.3");
    assert_eq!(
        provenance["source_commit"],
        "5be9149df14e30c0ab14f9e01d51dd2acfee5230"
    );
    assert_eq!(
        format!("{:x}", Sha256::digest(trace.as_bytes())),
        provenance["sha256"].as_str().unwrap()
    );
    let generator = include_str!("../../../tools/bedsimtrace/main.go").replace("\r\n", "\n");
    assert_eq!(
        format!("{:x}", Sha256::digest(generator.as_bytes())),
        provenance["generator_source_sha256"].as_str().unwrap()
    );
    assert_eq!(
        provenance["script_sha256"],
        "4ef08cd755a0f8e9480b621c9498790692e29b923196fc7b708049f7e94385d8"
    );
}

#[test]
fn terrain_scenario_verifier_detects_environment_and_world_identity_mutations() {
    let trace = include_str!("../fixtures/bedsim-v0.1.3-terrain.jsonl");
    let mut records = trace
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
        .collect::<Vec<_>>();
    records[0]["expected"]["environment"]["in_water"] = serde_json::Value::Bool(true);
    let mutated = records
        .iter()
        .map(serde_json::Value::to_string)
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    assert!(matches!(
        verify_scenario_trace_jsonl(&mutated, &Simulator::default(), 1.0e-12),
        Err(ConformanceError::DiscreteMismatch {
            field: "environment",
            ..
        })
    ));

    let mut records = trace
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
        .collect::<Vec<_>>();
    records[0]["expected"]["world_identity"]["preg_sha256"][0] = serde_json::Value::from(255);
    let mutated = records
        .iter()
        .map(serde_json::Value::to_string)
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    assert!(matches!(
        verify_scenario_trace_jsonl(&mutated, &Simulator::default(), 1.0e-12),
        Err(ConformanceError::DiscreteMismatch {
            field: "world_identity",
            ..
        })
    ));
}
