use serde::Deserialize;
use sha2::{Digest, Sha256};
use sim::{
    Aabb, BlockPhysicsFacts, BlockPhysicsFlags, BlockPhysicsSample, CollisionQuery, CollisionWorld,
    ConformanceError, MovementInput, PlayerState, ScenarioEvidence, Simulator, TickResult,
    TraceRecord, Vec3, WorldCollisionIdentity, WorldQueryError, audit_scenario_trace_jsonl,
    verify_legacy_trace_jsonl, verify_scenario_trace_jsonl, verify_trace_jsonl,
};

struct Floor;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LiquidEvidenceScript {
    #[serde(rename = "scenario")]
    _scenario: Box<str>,
    evidence: ScenarioEvidence,
    initial: PlayerState,
    steps: Vec<LiquidEvidenceStep>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LiquidEvidenceStep {
    world: LiquidEvidenceWorld,
    input: MovementInput,
    expected: TickResult,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LiquidEvidenceWorld {
    name: Box<str>,
    origin: [i32; 3],
    revision: u64,
    boxes: Box<[Aabb]>,
    physics: BlockPhysicsFacts,
    physics_regions: Box<[LiquidPhysicsRegion]>,
    unloaded: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LiquidPhysicsRegion {
    min: [i32; 3],
    max: [i32; 3],
    physics: BlockPhysicsFacts,
}

impl LiquidEvidenceWorld {
    /// Validates the test-only liquid extensions without widening `ScenarioWorld`.
    fn validate(&self) -> Result<(), &'static str> {
        if self.name.is_empty() || self.boxes.len() > 64 || self.physics_regions.len() > 64 {
            return Err("world shape count is outside the bounded fixture contract");
        }
        validate_liquid_facts(self.physics)?;
        for region in &self.physics_regions {
            if region
                .min
                .into_iter()
                .zip(region.max)
                .any(|(min, max)| min >= max)
            {
                return Err("physics region is empty or inverted");
            }
            validate_liquid_facts(region.physics)?;
        }
        for (index, first) in self.physics_regions.iter().enumerate() {
            for second in &self.physics_regions[index + 1..] {
                if first
                    .min
                    .into_iter()
                    .zip(first.max)
                    .zip(second.min.into_iter().zip(second.max))
                    .all(|((first_min, first_max), (second_min, second_max))| {
                        first_min < second_max && second_min < first_max
                    })
                {
                    return Err("physics regions overlap or shadow one another");
                }
            }
        }
        Ok(())
    }

    /// Resolves an explicitly regioned block after validation has excluded shadows.
    fn physics_at(&self, block: [i32; 3]) -> BlockPhysicsFacts {
        self.physics_regions
            .iter()
            .find(|region| {
                region
                    .min
                    .into_iter()
                    .zip(region.max)
                    .zip(block)
                    .all(|((min, max), value)| value >= min && value < max)
            })
            .map_or(self.physics, |region| region.physics)
    }

    /// Recomputes the generator's region-aware identity through the public wire format.
    fn identity(&self) -> WorldCollisionIdentity {
        let mut hash = Sha256::new();
        hash.update(b"sim-scenario-world-v1\0");
        for coordinate in self.origin {
            hash.update(coordinate.to_le_bytes());
        }
        hash.update(self.revision.to_le_bytes());
        hash.update(
            u32::try_from(self.boxes.len())
                .unwrap_or(u32::MAX)
                .to_le_bytes(),
        );
        for shape in &self.boxes {
            for value in [
                shape.min.x,
                shape.min.y,
                shape.min.z,
                shape.max.x,
                shape.max.y,
                shape.max.z,
            ] {
                hash.update(value.to_bits().to_le_bytes());
            }
        }
        hash_liquid_facts(&mut hash, self.physics);
        hash.update([u8::from(self.unloaded)]);
        if !self.physics_regions.is_empty() {
            hash.update(
                u32::try_from(self.physics_regions.len())
                    .unwrap_or(u32::MAX)
                    .to_le_bytes(),
            );
            for region in &self.physics_regions {
                for coordinate in region.min.into_iter().chain(region.max) {
                    hash.update(coordinate.to_le_bytes());
                }
                hash_liquid_facts(&mut hash, region.physics);
            }
        }
        serde_json::from_value(serde_json::json!({
            "protocol": 1001,
            "id_space": "sequential",
            "preg_sha256": <[u8; 32]>::from(hash.finalize()),
            "chunks": [{
                "dimension": 0,
                "x": self.origin[0] >> 4,
                "z": self.origin[2] >> 4,
                "revision": self.revision,
            }],
        }))
        .expect("the bounded liquid fixture identity uses the public wire format")
    }
}

impl CollisionWorld for LiquidEvidenceWorld {
    fn collision_boxes(&self, query: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        if self.unloaded {
            return Err(WorldQueryError::QueryExtentExceeded);
        }
        Ok(CollisionQuery {
            value: self
                .boxes
                .iter()
                .copied()
                .filter(|shape| shape.intersects(query))
                .collect(),
            identity: self.identity(),
        })
    }

    fn block_physics(&self, block: [i32; 3]) -> Result<BlockPhysicsSample, WorldQueryError> {
        if self.unloaded {
            return Err(WorldQueryError::QueryExtentExceeded);
        }
        Ok(BlockPhysicsSample {
            layers: Box::new([self.physics_at(block)]),
            identity: self.identity(),
        })
    }
}

/// Validates a region's facts using the public simulator input bounds.
fn validate_liquid_facts(facts: BlockPhysicsFacts) -> Result<(), &'static str> {
    if !facts.friction.is_finite()
        || !facts.horizontal_speed_factor.is_finite()
        || !facts.vertical_speed_factor.is_finite()
        || !facts.fluid_height_blocks.is_finite()
        || facts.friction <= 0.0
        || !(0.0..=1.0).contains(&facts.horizontal_speed_factor)
        || facts.horizontal_speed_factor == 0.0
        || !(0.0..=1.0).contains(&facts.vertical_speed_factor)
        || facts.vertical_speed_factor == 0.0
        || !(0.0..=1.0).contains(&facts.fluid_height_blocks)
        || facts.flags.bits() & !BlockPhysicsFlags::KNOWN_BITS != 0
    {
        return Err("physics facts are outside the bounded fixture contract");
    }
    Ok(())
}

/// Hashes test-local physics facts in the generator's published field order.
fn hash_liquid_facts(hash: &mut Sha256, facts: BlockPhysicsFacts) {
    for value in [
        facts.friction,
        facts.horizontal_speed_factor,
        facts.vertical_speed_factor,
        facts.fluid_height_blocks,
    ] {
        hash.update(value.to_bits().to_le_bytes());
    }
    hash.update([facts.flags.bits(), facts.surface_response as u8]);
}

/// Parses and validates a region-bearing liquid fixture script locally to this test.
fn parse_liquid_evidence_script(json: &str) -> Result<LiquidEvidenceScript, &'static str> {
    let script: LiquidEvidenceScript = serde_json::from_str(json).map_err(|_| "invalid JSON")?;
    for step in &script.steps {
        step.world.validate()?;
    }
    Ok(script)
}

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
fn historical_input_json_defaults_consumable_use_to_false() {
    let input: MovementInput = serde_json::from_str(
        r#"{"strafe":0.0,"forward":1.0,"yaw_degrees":0.0,"jumping":false,"jump_pressed":false,"sprinting":false,"sneaking":false}"#,
    )
    .unwrap();
    assert!(!input.using_consumable);
    assert!(!MovementInput::default().using_consumable);
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
fn complete_trace_schema_requires_environment_and_world_identity() {
    let input = MovementInput::default();
    let mut state = initial_state();
    let expected = Simulator::default()
        .tick(&mut state, input, &Floor)
        .unwrap();
    let canonical = serde_json::to_value(TraceRecord { input, expected }).unwrap();
    for field in ["environment", "world_identity"] {
        let mut incomplete = canonical.clone();
        incomplete["expected"]
            .as_object_mut()
            .unwrap()
            .remove(field);
        assert!(serde_json::from_value::<TraceRecord>(incomplete).is_err());
    }
}

#[test]
fn pinned_bedsim_v0_1_3_walk_sprint_jump_trace_matches() {
    let replayed = verify_legacy_trace_jsonl(
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

/// Replays one fixture script and compares its float32-origin observations.
fn replay_liquid_script(script: LiquidEvidenceScript) {
    assert_eq!(
        script.evidence,
        ScenarioEvidence::BedsimObservedWithManifestContext
    );
    let mut state = script.initial;
    for step in script.steps {
        let expected = step.expected;
        let actual = Simulator::default()
            .tick(&mut state, step.input, &step.world)
            .expect("fixture world is loaded and bounded");
        for (name, expected, actual) in [
            ("position.x", expected.position.x, actual.position.x),
            ("position.y", expected.position.y, actual.position.y),
            ("position.z", expected.position.z, actual.position.z),
            ("velocity.x", expected.velocity.x, actual.velocity.x),
            ("velocity.y", expected.velocity.y, actual.velocity.y),
            ("velocity.z", expected.velocity.z, actual.velocity.z),
            ("movement.x", expected.movement.x, actual.movement.x),
            ("movement.y", expected.movement.y, actual.movement.y),
            ("movement.z", expected.movement.z, actual.movement.z),
        ] {
            assert!(
                (expected - actual).abs() <= 1.0e-6,
                "{} differs: expected {expected}, actual {actual}",
                name
            );
        }
        assert_eq!(actual.collisions, expected.collisions);
        assert_eq!(actual.on_ground, expected.on_ground);
        assert_eq!(actual.environment, expected.environment);
        assert_eq!(actual.world_identity, expected.world_identity);
    }
}

#[test]
fn pinned_bedsim_v0_1_4_liquid_slice_replays_with_float32_tolerance() {
    let mut scripts = 0;
    for line in include_str!("../fixtures/bedsim-v0.1.4-liquid.jsonl").lines() {
        scripts += 1;
        replay_liquid_script(parse_liquid_evidence_script(line).unwrap());
    }
    assert_eq!(scripts, 4);
}

#[test]
fn liquid_fixture_parser_rejects_inverted_and_overlapping_regions() {
    let line = include_str!("../fixtures/bedsim-v0.1.4-liquid.jsonl")
        .lines()
        .next()
        .unwrap();
    let mut inverted: serde_json::Value = serde_json::from_str(line).unwrap();
    inverted["steps"][0]["world"]["physics_regions"][0]["min"][0] = serde_json::json!(1);
    assert!(matches!(
        parse_liquid_evidence_script(&inverted.to_string()),
        Err("physics region is empty or inverted")
    ));

    let mut overlapping: serde_json::Value = serde_json::from_str(line).unwrap();
    let region = overlapping["steps"][0]["world"]["physics_regions"][0].clone();
    overlapping["steps"][0]["world"]["physics_regions"]
        .as_array_mut()
        .unwrap()
        .push(region);
    assert!(matches!(
        parse_liquid_evidence_script(&overlapping.to_string()),
        Err("physics regions overlap or shadow one another")
    ));
}

#[test]
fn pinned_bedsim_v0_1_4_liquid_provenance_binds_module_generator_and_bytes() {
    let trace = include_bytes!("../fixtures/bedsim-v0.1.4-liquid.jsonl");
    let provenance: serde_json::Value = serde_json::from_str(include_str!(
        "../fixtures/bedsim-v0.1.4-liquid.provenance.json"
    ))
    .unwrap();
    assert_eq!(provenance["module"], "github.com/oomph-ac/bedsim");
    assert_eq!(provenance["version"], "v0.1.4");
    assert_eq!(
        provenance["source_commit"],
        "b55c95016bb53c3df3b13e9a5cd8cbbcacabbe28"
    );
    assert_eq!(
        provenance["module_sum"],
        "h1:oDfPiVgskqWnh9slic8Avdp+/Kd0NKWEJ2z2Ejghdq0="
    );
    assert_eq!(provenance["generator"], "tools/bedsimtrace-v0.1.4");
    assert_eq!(provenance["generator_command"], "GOWORK=off go run .");
    assert_eq!(
        format!("{:x}", Sha256::digest(trace)),
        provenance["sha256"].as_str().unwrap()
    );
    for (path, field) in [
        (
            "../../../tools/bedsimtrace-v0.1.4/main.go",
            "generator_source_sha256",
        ),
        ("../../../tools/bedsimtrace-v0.1.4/go.mod", "go_mod_sha256"),
        ("../../../tools/bedsimtrace-v0.1.4/go.sum", "go_sum_sha256"),
    ] {
        let source = match path {
            "../../../tools/bedsimtrace-v0.1.4/main.go" => {
                include_str!("../../../tools/bedsimtrace-v0.1.4/main.go")
            }
            "../../../tools/bedsimtrace-v0.1.4/go.mod" => {
                include_str!("../../../tools/bedsimtrace-v0.1.4/go.mod")
            }
            "../../../tools/bedsimtrace-v0.1.4/go.sum" => {
                include_str!("../../../tools/bedsimtrace-v0.1.4/go.sum")
            }
            _ => unreachable!("the fixed v0.1.4 provenance file list is exhaustive"),
        }
        .replace("\r\n", "\n");
        assert_eq!(
            format!("{:x}", Sha256::digest(source.as_bytes())),
            provenance[field].as_str().unwrap(),
            "{path}"
        );
    }
}

#[test]
fn pinned_bedsim_v0_1_5_liquid_slice_replays_with_float32_tolerance() {
    let mut scripts = 0;
    for line in include_str!("../fixtures/bedsim-v0.1.5-liquid.jsonl").lines() {
        scripts += 1;
        replay_liquid_script(parse_liquid_evidence_script(line).unwrap());
    }
    assert_eq!(scripts, 4);
}

#[test]
fn pinned_bedsim_v0_1_5_liquid_provenance_binds_module_generator_and_bytes() {
    let trace = include_bytes!("../fixtures/bedsim-v0.1.5-liquid.jsonl");
    let provenance: serde_json::Value = serde_json::from_str(include_str!(
        "../fixtures/bedsim-v0.1.5-liquid.provenance.json"
    ))
    .unwrap();
    assert_eq!(provenance["module"], "github.com/oomph-ac/bedsim");
    assert_eq!(provenance["version"], "v0.1.5");
    assert_eq!(
        provenance["source_commit"],
        "f6a0e6bdf72cf3e735198e3695086d59da456d79"
    );
    assert_eq!(
        provenance["module_sum"],
        "h1:LCAA1aK65z9TBkFOY4tv6qkkTXxXK+NxJeOz/SyUSd8="
    );
    assert_eq!(provenance["generator"], "tools/bedsimtrace-v0.1.5");
    assert_eq!(provenance["generator_command"], "GOWORK=off go run .");
    assert_eq!(
        format!("{:x}", Sha256::digest(trace)),
        provenance["sha256"].as_str().unwrap()
    );
    for (path, field) in [
        (
            "../../../tools/bedsimtrace-v0.1.5/main.go",
            "generator_source_sha256",
        ),
        ("../../../tools/bedsimtrace-v0.1.5/go.mod", "go_mod_sha256"),
        ("../../../tools/bedsimtrace-v0.1.5/go.sum", "go_sum_sha256"),
    ] {
        let source = match path {
            "../../../tools/bedsimtrace-v0.1.5/main.go" => {
                include_str!("../../../tools/bedsimtrace-v0.1.5/main.go")
            }
            "../../../tools/bedsimtrace-v0.1.5/go.mod" => {
                include_str!("../../../tools/bedsimtrace-v0.1.5/go.mod")
            }
            "../../../tools/bedsimtrace-v0.1.5/go.sum" => {
                include_str!("../../../tools/bedsimtrace-v0.1.5/go.sum")
            }
            _ => unreachable!("the fixed v0.1.5 provenance file list is exhaustive"),
        }
        .replace("\r\n", "\n");
        assert_eq!(
            format!("{:x}", Sha256::digest(source.as_bytes())),
            provenance[field].as_str().unwrap(),
            "{path}"
        );
    }
}

#[test]
fn terrain_trace_audits_observed_ticks_without_claiming_unsupported_conformance() {
    let trace = include_str!("../fixtures/bedsim-v0.1.3-terrain.jsonl");
    let audit = audit_scenario_trace_jsonl(trace, &Simulator::default(), 1.0e-12).unwrap();
    assert_eq!(audit.scripts, 31);
    assert_eq!(audit.observed_steps, 38);
    // Only the strata bedsim v0.1.3 genuinely implements are observed. Fluids,
    // bubble columns, scaffolding, honey, cobweb sensing, the step-correction
    // divergence, and the unloaded-chunk error contract have no bedsim oracle,
    // so they stay an explicit coverage ledger rather than a parity claim.
    assert_eq!(audit.unsupported_scripts, 12);
    assert!(matches!(
        verify_scenario_trace_jsonl(trace, &Simulator::default(), 1.0e-12),
        Err(ConformanceError::UnsupportedEvidence { count: 12 })
    ));

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
        provenance["module_sum"],
        "h1:tWZ7O48DL/SaWIY+0zz0hFln+DXN4vfatqKr8zTHVo8="
    );
    assert_eq!(
        provenance["generator_command"],
        "GOWORK=off go run . --terrain"
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
        format!(
            "{:x}",
            Sha256::digest(include_str!("../../../tools/bedsimtrace/go.mod").replace("\r\n", "\n"))
        ),
        provenance["go_mod_sha256"]
    );
    assert_eq!(
        format!(
            "{:x}",
            Sha256::digest(include_str!("../../../tools/bedsimtrace/go.sum").replace("\r\n", "\n"))
        ),
        provenance["go_sum_sha256"]
    );
    assert!(
        provenance["script_sha256"]
            .as_str()
            .is_some_and(|hash| hash.len() == 64)
    );
}

#[test]
fn terrain_scenario_audit_detects_environment_and_content_identity_mutations() {
    let trace = include_str!("../fixtures/bedsim-v0.1.3-terrain.jsonl");
    let mut records = trace
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
        .collect::<Vec<_>>();
    records[0]["steps"][0]["expected"]["environment"]["in_water"] = serde_json::Value::Bool(true);
    let mutated = records
        .iter()
        .map(serde_json::Value::to_string)
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    assert!(matches!(
        audit_scenario_trace_jsonl(&mutated, &Simulator::default(), 1.0e-12),
        Err(ConformanceError::DiscreteMismatch {
            field: "environment",
            ..
        })
    ));

    for path in [
        &["boxes", "0", "max", "x"][..],
        &["physics", "fluid_height_blocks"][..],
        &["origin", "0"][..],
        &["revision"][..],
    ] {
        let mut records = trace
            .lines()
            .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
            .collect::<Vec<_>>();
        let mut target = &mut records[0]["steps"][0]["world"];
        for segment in path {
            target = if let Ok(index) = segment.parse::<usize>() {
                &mut target[index]
            } else {
                &mut target[*segment]
            };
        }
        *target = if matches!(path[0], "origin" | "revision") {
            serde_json::Value::from(target.as_i64().unwrap() + 1)
        } else {
            serde_json::Value::from(target.as_f64().unwrap() + 1.0)
        };
        let mutated = records
            .iter()
            .map(serde_json::Value::to_string)
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
        assert!(
            matches!(
                audit_scenario_trace_jsonl(&mutated, &Simulator::default(), 1.0e-12),
                Err(ConformanceError::DiscreteMismatch {
                    field: "world_identity",
                    ..
                })
            ),
            "identity did not bind {path:?}"
        );
    }
}
