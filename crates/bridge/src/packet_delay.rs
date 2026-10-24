//! Native packet-delay acknowledgement data.

use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct PacketDelayLease {
    pub delay_ms: u32,
    pub lease_ms: u32,
    #[serde(default)]
    pub session_id: u64,
    #[serde(default)]
    pub position: Option<RelayedPosition>,
}

/// Exact network eye anchor of the last own movement flushed upstream.
#[derive(Clone, Copy, Debug, Deserialize)]
pub struct RelayedPosition {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}
