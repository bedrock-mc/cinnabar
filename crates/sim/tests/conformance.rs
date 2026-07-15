use sha2::{Digest, Sha256};
use sim::{
    Aabb, CollisionWorld, ConformanceError, MovementInput, PlayerState, Simulator, TickResult,
    TraceRecord, Vec3, WorldQueryError, verify_trace_jsonl,
};

struct Floor;

impl CollisionWorld for Floor {
    fn collision_boxes(&self, query: Aabb) -> Result<Vec<Aabb>, WorldQueryError> {
        let floor = Aabb::new(Vec3::new(-16.0, 0.0, -16.0), Vec3::new(16.0, 1.0, 16.0));
        Ok(floor
            .intersects(query)
            .then_some(floor)
            .into_iter()
            .collect())
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
fn pinned_bedsim_v0_1_3_walk_sprint_jump_trace_matches() {
    let replayed = verify_trace_jsonl(
        include_str!("../fixtures/bedsim-v0.1.3-basic.jsonl"),
        initial_state(),
        &Simulator::default(),
        &Floor,
        1.0e-12,
    )
    .unwrap();

    assert_eq!(replayed.tick, 4);
    assert!((replayed.position.y - 1.753_199_999_999_999_9).abs() <= 1.0e-12);
    assert!((replayed.position.z - 0.909_038_811_614_959).abs() <= 1.0e-12);
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
