//! Atomic ownership of one session's original network or companion terminal.

use std::sync::atomic::{AtomicU8, Ordering};

const UNCLAIMED: u8 = 0;
const NETWORK: u8 = 1;
#[cfg(not(target_arch = "wasm32"))]
const COMPANION: u8 = 2;

/// Shared only by the existing network, outbound and companion futures for one session.
pub(super) struct TerminalOwner(AtomicU8);

impl Default for TerminalOwner {
    fn default() -> Self {
        Self(AtomicU8::new(UNCLAIMED))
    }
}

impl TerminalOwner {
    /// Retains the network boundary before any FIFO, drain or final-telemetry await.
    /// Repeated claims allow a send failure to refine its retained cause to a server transfer.
    pub(super) fn network(&self) -> bool {
        matches!(
            self.0
                .compare_exchange(UNCLAIMED, NETWORK, Ordering::AcqRel, Ordering::Acquire),
            Ok(_) | Err(NETWORK)
        )
    }

    /// Allows companion-triggered shutdown only when no network terminal has already been retained.
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn companion(&self) -> bool {
        self.0
            .compare_exchange(UNCLAIMED, COMPANION, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }
}
