//! Allocation ownership shared by every renderer serving one signed bundle.

use std::{
    ops::Deref,
    sync::{Arc, Mutex},
};

/// A bundle's signed limit and the bytes its live GPU allocations still own.
#[derive(Clone, Debug)]
pub struct GpuLedger(Arc<Ledger>);

#[derive(Debug)]
struct Ledger {
    limit: u64,
    used: Mutex<u64>,
}

impl PartialEq for GpuLedger {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}
impl Eq for GpuLedger {}

impl GpuLedger {
    /// Starts an empty ledger for one bundle lifetime and its immutable signed allowance.
    pub fn new(limit: u64) -> Self {
        Self(Arc::new(Ledger {
            limit,
            used: Mutex::new(0),
        }))
    }

    /// Returns the signed ceiling; cloning a ledger never grants another allowance.
    pub fn limit(&self) -> u64 {
        self.0.limit
    }

    /// Returns bytes held by allocations, including retained capacity and pending releases.
    pub fn used(&self) -> u64 {
        *self.0.used.lock().unwrap()
    }

    /// Reserves bytes before allocating, rolling back if the allocation fails.
    pub fn allocate<T>(
        &self,
        bytes: u64,
        allocate: impl FnOnce() -> Option<T>,
    ) -> Option<GpuAllocation<T>> {
        let mut used = self.0.used.lock().unwrap();
        let next = used.checked_add(bytes)?;
        if next > self.0.limit {
            return None;
        }
        *used = next;
        drop(used);
        let charge = Arc::new(Charge {
            ledger: self.clone(),
            bytes,
        });
        Some(GpuAllocation {
            storage: allocate()?,
            _charge: Some(charge),
        })
    }
}

#[derive(Debug)]
struct Charge {
    ledger: GpuLedger,
    bytes: u64,
}
impl Drop for Charge {
    fn drop(&mut self) {
        *self.ledger.0.used.lock().unwrap() -= self.bytes;
    }
}

/// Keeps an allocation charged until its last storage owner is dropped.
#[derive(Clone, Debug)]
pub struct GpuAllocation<T> {
    storage: T,
    _charge: Option<Arc<Charge>>,
}
impl<T> GpuAllocation<T> {
    /// Allocates through the bundle ledger, or without a signed limit for host-owned storage.
    pub fn new(
        ledger: Option<&GpuLedger>,
        bytes: u64,
        allocate: impl FnOnce() -> Option<T>,
    ) -> Option<Self> {
        match ledger {
            Some(ledger) => ledger.allocate(bytes, allocate),
            None => Some(Self {
                storage: allocate()?,
                _charge: None,
            }),
        }
    }
}
impl<T> Deref for GpuAllocation<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.storage
    }
}

/// Maps helper resource owners and immediate primitives to one bundle allocation ledger.
#[derive(Clone, Debug, PartialEq)]
pub struct GpuReservation {
    pub owners: Vec<u64>,
    pub ledger: GpuLedger,
    pub primitives: Arc<crate::Primitives>,
    pub passes: Vec<u64>,
}
impl GpuReservation {
    /// Finds the common bundle ledger for a remapped drawing or mask owner.
    pub fn ledger_for(reservations: &[Self], owner: u64) -> Option<&GpuLedger> {
        reservations
            .iter()
            .find(|r| r.owners.contains(&owner))
            .map(|r| &r.ledger)
    }
    /// Finds the allocation owner of one globally unique committed post-effect revision.
    pub fn ledger_for_pass(reservations: &[Self], revision: u64) -> Option<&GpuLedger> {
        reservations
            .iter()
            .find(|r| r.passes.contains(&revision))
            .map(|r| &r.ledger)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn admissions_share_one_limit_and_last_owner_releases_storage() {
        let ledger = GpuLedger::new(100);
        let geometry = ledger.allocate(60, || Some(())).unwrap();
        let media = ledger.clone();
        assert!(media.allocate(41, || Some(())).is_none());
        let texture = media.allocate(40, || Some(())).unwrap();
        let retained = geometry.clone();
        drop(geometry);
        assert_eq!(ledger.used(), 100);
        drop(retained);
        assert_eq!(ledger.used(), 40);
        drop(texture);
        assert_eq!(ledger.used(), 0);
        assert!(ledger.allocate::<()>(100, || None).is_none());
        assert_eq!(ledger.used(), 0);
    }
}
