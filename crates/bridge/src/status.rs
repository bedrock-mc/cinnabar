//! Session-scoped latency observation shared with presentation.

use serde::Deserialize;

/// Session-scoped upstream latency; an unsampled transport has no fabricated zero.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct NetworkConnection {
    pub attempt_id: u64,
    pub local_runtime_id: u64,
    pub round_trip_ms: Option<u32>,
}
