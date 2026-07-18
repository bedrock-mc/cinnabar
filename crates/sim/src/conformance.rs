use serde::{Deserialize, Serialize};
use thiserror::Error;

use std::collections::BTreeSet;

use sha2::{Digest, Sha256};
use world::{ChunkCollisionRevision, ChunkKey};

use crate::{
    Aabb, BlockPhysicsFacts, BlockPhysicsSample, CollisionIdSpace, CollisionQuery,
    CollisionRegistryIdentity, CollisionWorld, MovementInput, PlayerState, SimulationError,
    Simulator, TickResult, Vec3, WorldCollisionIdentity, WorldQueryError,
};

const MAX_TRACE_LINE_BYTES: usize = 64 * 1024;
const MAX_TRACE_RECORDS: usize = 1_000_000;

/// One canonical bedsim input/output record. Each JSONL line is exactly one
/// 20 Hz movement tick.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TraceRecord {
    pub input: MovementInput,
    pub expected: TickResult,
}

/// Historical bedsim output that predates environmental/PREG evidence. It is
/// intentionally accepted only by `verify_legacy_trace_jsonl`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LegacyTraceRecord {
    pub input: MovementInput,
    pub expected: LegacyTickResult,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LegacyTickResult {
    pub tick: u64,
    pub position: Vec3,
    pub velocity: Vec3,
    pub movement: Vec3,
    pub collisions: crate::AxisCollisions,
    pub on_ground: bool,
}

/// A multi-tick evidence script. Only bedsim-observed steps are conformance
/// claims. Unsupported scripts are retained as an explicit coverage ledger.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScenarioScript {
    pub scenario: Box<str>,
    pub evidence: ScenarioEvidence,
    pub initial: PlayerState,
    pub steps: Box<[ScenarioStep]>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum ScenarioEvidence {
    /// Numeric movement/collision fields came from bedsim; the empty
    /// environment and content identity come from the canonical world manifest.
    BedsimObservedWithManifestContext,
    UnsupportedNonConformance {
        reason: Box<str>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScenarioStep {
    pub world: ScenarioWorld,
    pub input: MovementInput,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected: Option<TickResult>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScenarioWorld {
    pub name: Box<str>,
    pub origin: [i32; 3],
    pub revision: u64,
    pub boxes: Box<[Aabb]>,
    pub physics: BlockPhysicsFacts,
    #[serde(default)]
    pub unloaded: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScenarioAudit {
    pub scripts: usize,
    pub observed_steps: usize,
    pub unsupported_scripts: usize,
}

#[derive(Debug, Error)]
pub enum ConformanceError {
    #[error("trace epsilon must be finite and non-negative")]
    InvalidEpsilon,
    #[error("trace is empty")]
    EmptyTrace,
    #[error("trace line {line} is blank")]
    BlankLine { line: usize },
    #[error("trace line {line} exceeds {max} bytes")]
    LineTooLong { line: usize, max: usize },
    #[error("trace exceeds {max} records")]
    TooManyRecords { max: usize },
    #[error("trace line {line} is not canonical TraceRecord JSON: {source}")]
    Json {
        line: usize,
        #[source]
        source: serde_json::Error,
    },
    #[error("trace line {line} expected tick {expected}, found {actual}")]
    TickSequence {
        line: usize,
        expected: u64,
        actual: u64,
    },
    #[error("trace line {line} could not simulate: {source}")]
    Simulation {
        line: usize,
        #[source]
        source: SimulationError,
    },
    #[error(
        "trace line {line} tick {tick} field {field} differs: expected {expected}, actual {actual}, epsilon {epsilon}"
    )]
    Mismatch {
        line: usize,
        tick: u64,
        field: &'static str,
        expected: f64,
        actual: f64,
        epsilon: f64,
    },
    #[error("trace line {line} tick {tick} field {field} differs")]
    DiscreteMismatch {
        line: usize,
        tick: u64,
        field: &'static str,
    },
    #[error("scenario trace coverage differs from the required Task 3 strata")]
    ScenarioCoverage,
    #[error("scenario trace line {line} has an invalid world: {reason}")]
    InvalidScenarioWorld { line: usize, reason: &'static str },
    #[error("scenario trace line {line} has invalid evidence/expected fields")]
    InvalidScenarioEvidence { line: usize },
    #[error("scenario trace includes {count} explicitly unsupported non-conformance scripts")]
    UnsupportedEvidence { count: usize },
}

const REQUIRED_TERRAIN_SCENARIOS: [&str; 27] = [
    "flat_walk",
    "diagonal",
    "sprint_jump",
    "slab_step",
    "stair_step",
    "sneak_north",
    "sneak_south",
    "sneak_east",
    "sneak_west",
    "head_collision",
    "ladder_ascend",
    "ladder_descend",
    "ladder_hold",
    "water_enter",
    "water_swim",
    "water_exit",
    "lava",
    "cobweb",
    "slime_bounce",
    "slime_sneak",
    "bed_bounce",
    "soul_sand",
    "honey",
    "scaffolding",
    "bubble_up",
    "bubble_down",
    "unloaded_boundary",
];

impl ScenarioWorld {
    fn identity(&self) -> WorldCollisionIdentity {
        WorldCollisionIdentity::new(
            CollisionRegistryIdentity {
                protocol: 1001,
                id_space: CollisionIdSpace::Sequential,
                preg_sha256: self.content_digest(),
            },
            [ChunkCollisionRevision {
                chunk: ChunkKey::new(0, self.origin[0] >> 4, self.origin[2] >> 4),
                revision: self.revision,
            }],
        )
        .expect("one scenario identity chunk is bounded")
    }

    fn content_digest(&self) -> [u8; 32] {
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
        for value in [
            self.physics.friction,
            self.physics.horizontal_speed_factor,
            self.physics.vertical_speed_factor,
            self.physics.fluid_height_blocks,
        ] {
            hash.update(value.to_bits().to_le_bytes());
        }
        hash.update([
            self.physics.flags.bits(),
            self.physics.surface_response as u8,
        ]);
        hash.update([u8::from(self.unloaded)]);
        hash.finalize().into()
    }

    fn validate(&self, line: usize) -> Result<(), ConformanceError> {
        if self.name.is_empty() || self.boxes.len() > 64 {
            return Err(ConformanceError::InvalidScenarioWorld {
                line,
                reason: "empty name or more than 64 collision boxes",
            });
        }
        for shape in &self.boxes {
            if !shape.min.is_finite()
                || !shape.max.is_finite()
                || shape.min.x >= shape.max.x
                || shape.min.y >= shape.max.y
                || shape.min.z >= shape.max.z
            {
                return Err(ConformanceError::InvalidScenarioWorld {
                    line,
                    reason: "non-finite or inverted collision box",
                });
            }
        }
        for value in [
            self.physics.friction,
            self.physics.horizontal_speed_factor,
            self.physics.vertical_speed_factor,
            self.physics.fluid_height_blocks,
        ] {
            if !value.is_finite() {
                return Err(ConformanceError::InvalidScenarioWorld {
                    line,
                    reason: "non-finite movement metadata",
                });
            }
        }
        if self.physics.friction <= 0.0
            || self.physics.horizontal_speed_factor <= 0.0
            || self.physics.horizontal_speed_factor > 1.0
            || self.physics.vertical_speed_factor <= 0.0
            || self.physics.vertical_speed_factor > 1.0
            || !(0.0..=1.0).contains(&self.physics.fluid_height_blocks)
            || self.physics.flags.bits() & !crate::BlockPhysicsFlags::KNOWN_BITS != 0
        {
            return Err(ConformanceError::InvalidScenarioWorld {
                line,
                reason: "movement metadata is outside its pinned bounds",
            });
        }
        Ok(())
    }
}

impl CollisionWorld for ScenarioWorld {
    fn collision_boxes(&self, query: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        if self.unloaded {
            return Err(WorldQueryError::UnloadedChunk(ChunkKey::new(0, 2, 3)));
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

    fn block_physics(&self, _block: [i32; 3]) -> Result<BlockPhysicsSample, WorldQueryError> {
        if self.unloaded {
            return Err(WorldQueryError::UnloadedChunk(ChunkKey::new(0, 2, 3)));
        }
        Ok(BlockPhysicsSample {
            layers: Box::new([self.physics]),
            identity: self.identity(),
        })
    }
}

/// Audits all Task 3 scripts while replaying only evidence actually observed
/// from the pinned bedsim module.
pub fn audit_scenario_trace_jsonl(
    jsonl: &str,
    simulator: &Simulator,
    epsilon: f64,
) -> Result<ScenarioAudit, ConformanceError> {
    if !epsilon.is_finite() || epsilon < 0.0 {
        return Err(ConformanceError::InvalidEpsilon);
    }
    if jsonl.is_empty() {
        return Err(ConformanceError::EmptyTrace);
    }
    let required = REQUIRED_TERRAIN_SCENARIOS
        .into_iter()
        .collect::<BTreeSet<_>>();
    let mut scenarios = BTreeSet::new();
    let mut records = 0;
    let mut observed_steps = 0;
    let mut unsupported_scripts = 0;
    for (index, raw_line) in jsonl.split_terminator('\n').enumerate() {
        let line = index + 1;
        records += 1;
        if records > REQUIRED_TERRAIN_SCENARIOS.len() {
            return Err(ConformanceError::ScenarioCoverage);
        }
        let raw_line = raw_line.strip_suffix('\r').unwrap_or(raw_line);
        if raw_line.is_empty() {
            return Err(ConformanceError::BlankLine { line });
        }
        if raw_line.len() > MAX_TRACE_LINE_BYTES {
            return Err(ConformanceError::LineTooLong {
                line,
                max: MAX_TRACE_LINE_BYTES,
            });
        }
        let record: ScenarioScript = serde_json::from_str(raw_line)
            .map_err(|source| ConformanceError::Json { line, source })?;
        if !scenarios.insert(record.scenario.to_string()) || record.steps.len() < 2 {
            return Err(ConformanceError::ScenarioCoverage);
        }
        let mut state = record.initial;
        match record.evidence {
            ScenarioEvidence::BedsimObservedWithManifestContext => {
                for step in record.steps {
                    step.world.validate(line)?;
                    let expected = step
                        .expected
                        .ok_or(ConformanceError::InvalidScenarioEvidence { line })?;
                    let expected_tick =
                        state
                            .tick
                            .checked_add(1)
                            .ok_or(ConformanceError::TickSequence {
                                line,
                                expected: u64::MAX,
                                actual: expected.tick,
                            })?;
                    if expected.tick != expected_tick {
                        return Err(ConformanceError::TickSequence {
                            line,
                            expected: expected_tick,
                            actual: expected.tick,
                        });
                    }
                    let actual = simulator
                        .tick(&mut state, step.input, &step.world)
                        .map_err(|source| ConformanceError::Simulation { line, source })?;
                    compare_tick(line, expected, actual, epsilon)?;
                    observed_steps += 1;
                }
            }
            ScenarioEvidence::UnsupportedNonConformance { reason } => {
                if reason.is_empty() || record.steps.iter().any(|step| step.expected.is_some()) {
                    return Err(ConformanceError::InvalidScenarioEvidence { line });
                }
                for step in record.steps {
                    step.world.validate(line)?;
                }
                unsupported_scripts += 1;
            }
        }
    }
    if scenarios
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>()
        != required
    {
        return Err(ConformanceError::ScenarioCoverage);
    }
    Ok(ScenarioAudit {
        scripts: records,
        observed_steps,
        unsupported_scripts,
    })
}

/// Requires every script to be backed by pinned bedsim observations. This is
/// deliberately stricter than the audit and currently rejects the Phase 3
/// ledger while unsupported environmental strata remain.
pub fn verify_scenario_trace_jsonl(
    jsonl: &str,
    simulator: &Simulator,
    epsilon: f64,
) -> Result<ScenarioAudit, ConformanceError> {
    let audit = audit_scenario_trace_jsonl(jsonl, simulator, epsilon)?;
    if audit.unsupported_scripts != 0 {
        return Err(ConformanceError::UnsupportedEvidence {
            count: audit.unsupported_scripts,
        });
    }
    Ok(audit)
}

/// Parses and replays a canonical pinned-bedsim JSONL trace.
///
/// The returned state is the state after the final record. Any parse, world,
/// or parity failure stops at the first line and returns evidence naming that
/// exact tick and field.
pub fn verify_trace_jsonl(
    jsonl: &str,
    mut state: PlayerState,
    simulator: &Simulator,
    world: &impl CollisionWorld,
    epsilon: f64,
) -> Result<PlayerState, ConformanceError> {
    if !epsilon.is_finite() || epsilon < 0.0 {
        return Err(ConformanceError::InvalidEpsilon);
    }
    if jsonl.is_empty() {
        return Err(ConformanceError::EmptyTrace);
    }

    let mut records = 0_usize;
    for (index, raw_line) in jsonl.split_terminator('\n').enumerate() {
        let line = index + 1;
        records += 1;
        if records > MAX_TRACE_RECORDS {
            return Err(ConformanceError::TooManyRecords {
                max: MAX_TRACE_RECORDS,
            });
        }
        let raw_line = raw_line.strip_suffix('\r').unwrap_or(raw_line);
        if raw_line.is_empty() {
            return Err(ConformanceError::BlankLine { line });
        }
        if raw_line.len() > MAX_TRACE_LINE_BYTES {
            return Err(ConformanceError::LineTooLong {
                line,
                max: MAX_TRACE_LINE_BYTES,
            });
        }
        let record: TraceRecord = serde_json::from_str(raw_line)
            .map_err(|source| ConformanceError::Json { line, source })?;
        let expected_tick = state
            .tick
            .checked_add(1)
            .ok_or(ConformanceError::TickSequence {
                line,
                expected: u64::MAX,
                actual: record.expected.tick,
            })?;
        if record.expected.tick != expected_tick {
            return Err(ConformanceError::TickSequence {
                line,
                expected: expected_tick,
                actual: record.expected.tick,
            });
        }
        let actual = simulator
            .tick(&mut state, record.input, world)
            .map_err(|source| ConformanceError::Simulation { line, source })?;
        compare_tick(line, record.expected, actual, epsilon)?;
    }
    if records == 0 {
        return Err(ConformanceError::EmptyTrace);
    }
    Ok(state)
}

/// Replays the historical numeric-only bedsim fixture without promoting it to
/// environmental or identity conformance evidence.
pub fn verify_legacy_trace_jsonl(
    jsonl: &str,
    mut state: PlayerState,
    simulator: &Simulator,
    world: &impl CollisionWorld,
    epsilon: f64,
) -> Result<PlayerState, ConformanceError> {
    if !epsilon.is_finite() || epsilon < 0.0 {
        return Err(ConformanceError::InvalidEpsilon);
    }
    if jsonl.is_empty() {
        return Err(ConformanceError::EmptyTrace);
    }
    let mut records = 0_usize;
    for (index, raw_line) in jsonl.split_terminator('\n').enumerate() {
        let line = index + 1;
        records += 1;
        if records > MAX_TRACE_RECORDS {
            return Err(ConformanceError::TooManyRecords {
                max: MAX_TRACE_RECORDS,
            });
        }
        let raw_line = raw_line.strip_suffix('\r').unwrap_or(raw_line);
        if raw_line.is_empty() {
            return Err(ConformanceError::BlankLine { line });
        }
        if raw_line.len() > MAX_TRACE_LINE_BYTES {
            return Err(ConformanceError::LineTooLong {
                line,
                max: MAX_TRACE_LINE_BYTES,
            });
        }
        let record: LegacyTraceRecord = serde_json::from_str(raw_line)
            .map_err(|source| ConformanceError::Json { line, source })?;
        let expected_tick = state
            .tick
            .checked_add(1)
            .ok_or(ConformanceError::TickSequence {
                line,
                expected: u64::MAX,
                actual: record.expected.tick,
            })?;
        if record.expected.tick != expected_tick {
            return Err(ConformanceError::TickSequence {
                line,
                expected: expected_tick,
                actual: record.expected.tick,
            });
        }
        let actual = simulator
            .tick(&mut state, record.input, world)
            .map_err(|source| ConformanceError::Simulation { line, source })?;
        compare_vec(
            line,
            record.expected.tick,
            "position",
            record.expected.position,
            actual.position,
            epsilon,
        )?;
        compare_vec(
            line,
            record.expected.tick,
            "velocity",
            record.expected.velocity,
            actual.velocity,
            epsilon,
        )?;
        compare_vec(
            line,
            record.expected.tick,
            "movement",
            record.expected.movement,
            actual.movement,
            epsilon,
        )?;
        for (field, differs) in [
            (
                "collisions.x",
                record.expected.collisions.x != actual.collisions.x,
            ),
            (
                "collisions.y",
                record.expected.collisions.y != actual.collisions.y,
            ),
            (
                "collisions.z",
                record.expected.collisions.z != actual.collisions.z,
            ),
            ("on_ground", record.expected.on_ground != actual.on_ground),
        ] {
            if differs {
                return Err(ConformanceError::DiscreteMismatch {
                    line,
                    tick: record.expected.tick,
                    field,
                });
            }
        }
    }
    if records == 0 {
        return Err(ConformanceError::EmptyTrace);
    }
    Ok(state)
}

fn compare_tick(
    line: usize,
    expected: TickResult,
    actual: TickResult,
    epsilon: f64,
) -> Result<(), ConformanceError> {
    compare_vec(
        line,
        expected.tick,
        "position",
        expected.position,
        actual.position,
        epsilon,
    )?;
    compare_vec(
        line,
        expected.tick,
        "velocity",
        expected.velocity,
        actual.velocity,
        epsilon,
    )?;
    compare_vec(
        line,
        expected.tick,
        "movement",
        expected.movement,
        actual.movement,
        epsilon,
    )?;
    for (field, differs) in [
        ("collisions.x", expected.collisions.x != actual.collisions.x),
        ("collisions.y", expected.collisions.y != actual.collisions.y),
        ("collisions.z", expected.collisions.z != actual.collisions.z),
        ("on_ground", expected.on_ground != actual.on_ground),
        ("environment", expected.environment != actual.environment),
        (
            "world_identity",
            expected.world_identity != actual.world_identity,
        ),
    ] {
        if differs {
            return Err(ConformanceError::DiscreteMismatch {
                line,
                tick: expected.tick,
                field,
            });
        }
    }
    Ok(())
}

fn compare_vec(
    line: usize,
    tick: u64,
    prefix: &'static str,
    expected: Vec3,
    actual: Vec3,
    epsilon: f64,
) -> Result<(), ConformanceError> {
    let fields = match prefix {
        "position" => ["position.x", "position.y", "position.z"],
        "velocity" => ["velocity.x", "velocity.y", "velocity.z"],
        "movement" => ["movement.x", "movement.y", "movement.z"],
        _ => unreachable!("all conformance vectors use known field prefixes"),
    };
    for (field, expected, actual) in [
        (fields[0], expected.x, actual.x),
        (fields[1], expected.y, actual.y),
        (fields[2], expected.z, actual.z),
    ] {
        if !expected.is_finite() || !actual.is_finite() || (expected - actual).abs() > epsilon {
            return Err(ConformanceError::Mismatch {
                line,
                tick,
                field,
                expected,
                actual,
                epsilon,
            });
        }
    }
    Ok(())
}
