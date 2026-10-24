//! Session transport and join bootstrap without pack compilation or presentation ownership.
//!
//! Control and world channels retain separate bounded FIFOs. Prepared presentation data is
//! an opaque generic payload: the session publishes it only at the original bootstrap point.

#[cfg(not(target_arch = "wasm32"))]
pub mod connection;
pub mod markers;
mod session;

#[cfg(any(test, feature = "test-support"))]
pub use session::experience::ExperienceIngress;
#[cfg(any(test, feature = "test-support"))]
pub use session::handle_state::CapturedPackets;
pub use session::{
    BatchSendError, CONTROL_EVENT_CAPACITY, NetworkControlEvent, NetworkExecutor,
    NetworkFailureOrigin, NetworkHandle, PacketSendError, PreparedLogin, PreparedSession,
    SequencedWorldEvent, SessionRead, SessionTransferTarget, WORLD_EVENT_CAPACITY, WorldIngress,
    prepare_network, session_failure_display,
};

/// Maximum automatic redirects before the player must request a fresh join.
pub const MAX_TRANSFER_CHAIN_HOPS: u32 = 8;

/// Optional observations at the original transport publication boundaries.
#[derive(Clone, Copy)]
pub struct SessionTrace {
    /// Formats a sanitized movement packet immediately before its socket write.
    pub movement_line: fn(u64, &protocol::Packet) -> Option<String>,
    /// Publishes a formatted movement observation only after a successful write.
    pub write_movement: fn(&str),
    /// Publishes an already serialized inbound packet trace.
    pub packet_trace: fn(&str),
    /// Names the optional transfer action marker emitted after its successful socket write.
    pub fast_transfer_action_marker: Option<&'static str>,
}

impl Default for SessionTrace {
    fn default() -> Self {
        Self {
            movement_line: |_, _| None,
            write_movement: |_| {},
            packet_trace: |_| {},
            fast_transfer_action_marker: None,
        }
    }
}

#[cfg(test)]
#[global_allocator]
static ALLOCATOR: test_alloc::CountingAllocator = test_alloc::CountingAllocator;

#[cfg(not(target_arch = "wasm32"))]
pub use session::{NetworkConfig, packet_log, spawn_network};
