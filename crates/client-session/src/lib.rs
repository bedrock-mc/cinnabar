//! Session transport and join bootstrap without pack compilation or presentation ownership.
//!
//! Control and world channels retain separate bounded FIFOs. Prepared presentation data is
//! an opaque generic payload: the session publishes it only at the original bootstrap point.

pub mod connection;
mod session;

#[cfg(any(test, feature = "test-support"))]
pub use session::handle_state::CapturedPackets;
pub use session::{
    BatchSendError, NetworkConfig, NetworkControlEvent, NetworkFailureOrigin, NetworkHandle,
    PacketSendError, PreparedSession, SequencedWorldEvent, SessionTransferTarget,
    WORLD_EVENT_CAPACITY, WorldIngress, session_failure_display, spawn_network,
};

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
