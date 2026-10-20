//! The local-world manager's single active or unresolved native publication reservation.

use super::*;
use std::sync::Mutex;

#[derive(Default)]
struct Slot {
    reserved: bool,
    pending: Option<HostDirectory>,
}

/// The pending owner has no slot/manager strong reference, so retained cleanup cannot form a cycle.
#[derive(Clone, Default)]
pub(crate) struct HostSlot(Arc<Mutex<Slot>>);

impl HostSlot {
    /// Reserves capacity before any account or publication I/O; pending cleanup blocks another host.
    pub(crate) fn open(
        &self,
        account: Arc<NativeAccount>,
        opening: HostOpening,
        deadline: Instant,
    ) -> io::Result<NativeHostedWorld> {
        let directory = HostDirectory::new(account, opening, deadline)?;
        let mut slot = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if slot.reserved || slot.pending.is_some() {
            return Err(busy());
        }
        slot.reserved = true;
        Ok(NativeHostedWorld {
            slot: Some(self.clone()),
            directory: Some(directory),
            peers: Default::default(),
        })
    }

    /// Transfers the original retained owner into one caller-driven cleanup lifetime.
    pub(crate) fn cleanup(&self) -> io::Result<Option<NativeHostedWorld>> {
        let mut slot = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if slot.reserved {
            return Err(busy());
        }
        let Some(directory) = slot.pending.take() else {
            return Ok(None);
        };
        slot.reserved = true;
        Ok(Some(NativeHostedWorld {
            slot: Some(self.clone()),
            directory: Some(directory),
            peers: Default::default(),
        }))
    }

    /// Stops local I/O and returns only an actual retained remote obligation to the bounded slot.
    pub(super) fn release(&self, directory: Option<HostDirectory>) {
        let pending = directory.and_then(|mut directory| {
            directory.stop();
            directory.needs_cleanup().then_some(directory)
        });
        let mut slot = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        slot.pending = pending;
        slot.reserved = false;
    }
}

/// Backpressures another remote publication without discarding the old obligation.
fn busy() -> io::Error {
    io::Error::new(
        io::ErrorKind::WouldBlock,
        "native host membership is active or awaiting cleanup",
    )
}
