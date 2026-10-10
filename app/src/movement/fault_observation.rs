//! Projects gameplay faults into the existing app-owned evidence record.

use {super::MovementTicker, gameplay::movement::PhysicsAuthorityFault};

/// A fault observation consumed by the existing evidence adapter.
/// The gameplay ticker remains the only owner of pending fault state.
#[derive(Debug, Clone, PartialEq)]
pub struct PhysicsAuthorityFaultRecord {
    pub session_generation: u64,
    pub fault: PhysicsAuthorityFault,
    pub next_tick: u64,
    pub pending_count: usize,
}

impl From<gameplay::movement::PhysicsAuthorityFaultRecord> for PhysicsAuthorityFaultRecord {
    /// Transfers a drained domain record into the app's existing evidence contract.
    fn from(record: gameplay::movement::PhysicsAuthorityFaultRecord) -> Self {
        Self {
            session_generation: record.session_generation,
            fault: record.fault,
            next_tick: record.next_tick,
            pending_count: record.pending_count,
        }
    }
}

/// A borrowed pending fault; it stores no second copy of gameplay state.
pub(crate) struct PendingPhysicsFault<'a>(
    Option<&'a gameplay::movement::PhysicsAuthorityFaultRecord>,
);

impl PendingPhysicsFault<'_> {
    /// Reports whether gameplay currently retains a fault.
    pub(crate) fn is_some(&self) -> bool {
        self.0.is_some()
    }

    /// Copies an observation only when the evidence adapter requests it.
    #[cfg(feature = "acceptance")]
    pub(crate) fn cloned(&self) -> Option<PhysicsAuthorityFaultRecord> {
        self.0.cloned().map(Into::into)
    }
}

impl MovementTicker {
    /// Borrows the retained domain fault at the existing telemetry boundary.
    pub(crate) fn pending_authority_fault(&self) -> PendingPhysicsFault<'_> {
        PendingPhysicsFault(self.0.pending_authority_fault())
    }

    /// Drains the domain fault once, preserving the existing evidence record type.
    #[cfg(feature = "acceptance")]
    pub(crate) fn take_authority_fault(&mut self) -> Option<PhysicsAuthorityFaultRecord> {
        self.0.take_authority_fault().map(Into::into)
    }
}
