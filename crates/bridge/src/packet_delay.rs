//! Private core control; this never exposes packet contents to a component.

use crate::BridgeError;
use serde::{Deserialize, Serialize};
use std::path::Path;

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

#[derive(Serialize)]
struct Request {
    delay_ms: u32,
    show_real_position: bool,
}

pub async fn set_packet_delay(
    socket_dir: &Path,
    delay_ms: u32,
) -> Result<PacketDelayLease, BridgeError> {
    packet_delay_with_position(socket_dir, delay_ms, false).await
}

pub async fn packet_delay_with_position(
    socket_dir: &Path,
    delay_ms: u32,
    show_real_position: bool,
) -> Result<PacketDelayLease, BridgeError> {
    crate::account::call(
        socket_dir,
        "packet_delay.v1",
        Some(Request {
            delay_ms,
            show_real_position,
        }),
    )
    .await
}
