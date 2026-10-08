//! Session transport and pack preparation without application or presentation ownership.
//!
//! Control and world channels retain separate bounded FIFOs. Prepared presentation data is
//! an opaque generic payload: the session publishes it only at the original bootstrap point.

mod block_physics;
pub mod compile_cache;
pub mod connection;
pub mod pack_language;
mod pack_preparation;
pub mod pack_textures;
mod session;

pub use pack_preparation::{
    BootstrapGenerationDisposition, PackInputs, PackPreparation, RequiredPackRejected,
    ResourcePackAdmissionState, StackFingerprint, classify_bootstrap_generation,
    custom_block_items, prepare_session_packs, required_packs_applied, stack_fingerprint,
};
#[cfg(any(test, feature = "test-support"))]
pub use session::handle_state::CapturedPackets;
pub use session::{
    BatchSendError, NetworkConfig, NetworkControlEvent, NetworkFailureOrigin, NetworkHandle,
    PacketSendError, SequencedWorldEvent, SessionTransferTarget, WORLD_EVENT_CAPACITY,
    WorldIngress, session_failure_display, spawn_network,
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
