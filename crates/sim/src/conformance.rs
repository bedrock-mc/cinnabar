use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    CollisionWorld, MovementInput, PlayerState, SimulationError, Simulator, TickResult, Vec3,
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
