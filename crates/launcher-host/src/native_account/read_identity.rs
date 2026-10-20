//! Protected final identity checks for account-borrowed presentation results.

use super::*;

#[derive(Clone)]
pub(super) struct ReadIdentity {
    pub(super) account: Arc<NativeAccount>,
    pub(super) generation: String,
    pub(super) xuid: String,
}

impl ReadIdentity {
    /// Captures the actual verified owner while its original credential lease is still held.
    pub(super) fn capture(operation: &Operation) -> Self {
        let cached = operation.cached.as_ref().expect("verified account");
        Self {
            account: Arc::clone(&operation.account),
            generation: cached.generation.clone(),
            xuid: cached
                .identity
                .as_ref()
                .expect("verified identity")
                .xuid
                .clone(),
        }
    }

    /// Rechecks selection after asynchronous work without authenticating or replacing its proof.
    pub(super) fn current(&self) -> bool {
        self.current_status().unwrap_or(false)
    }

    /// Retains a completed result through contention; its caller owns cancellation and the cutoff.
    pub(super) async fn wait_current(&self) -> bool {
        loop {
            if let Some(current) = self.current_status() {
                return current;
            }
            tokio::time::sleep(LEASE_RETRY).await;
        }
    }

    /// Checks this same captured identity while its actual Operation still owns the protected lease.
    pub(super) fn current_operation(&self, operation: &Operation) -> bool {
        if operation.rejected || !Arc::ptr_eq(&self.account, &operation.account) {
            return false;
        }
        let (Some(cached), Some(lease)) = (&operation.cached, &operation.lease) else {
            return false;
        };
        self.current_owner(cached, lease)
    }

    /// An unavailable lease or owner is not evidence that its verified identity was replaced.
    pub(super) fn current_status(&self) -> Option<bool> {
        let lease = match self.account.store.try_native_lease() {
            Ok(lease) => lease,
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => return None,
            Err(_) => return Some(false),
        };
        let state = match self.account.available_state() {
            AccountAvailability::Available(state) => state,
            AccountAvailability::Busy => return None,
            AccountAvailability::Unavailable => return Some(false),
        };
        let Some(cached) = state.cached.as_ref() else {
            return Some(false);
        };
        Some(self.current_owner(cached, &lease))
    }

    /// Shares the exact lease/binding/XUID predicate between available and already-held owners.
    fn current_owner(&self, cached: &Cached, lease: &NativeLease) -> bool {
        cached.generation == self.generation
            && lease.generation() == self.generation
            && cached.binding == lease.binding()
            && lease
                .selected_id()
                .is_none_or(|selected| selected == self.xuid)
            && self
                .account
                .selected_id
                .as_deref()
                .is_none_or(|selected| lease.selected_id() == Some(selected))
            && cached
                .identity
                .as_ref()
                .is_some_and(|identity| identity.current_credential && identity.xuid == self.xuid)
    }
}

/// Availability of the actual account owner; Available alone grants no identity or transport authority.
pub(super) enum AccountAvailability<'a> {
    Available(std::sync::MutexGuard<'a, State>),
    Busy,
    Unavailable,
}

impl NativeAccount {
    /// Distinguishes temporary checkout from a missing/poisoned owner without cloning its SDK state.
    /// Callers check their own original identity/proof with this guard, then release it before awaiting.
    pub(super) fn available_state(&self) -> AccountAvailability<'_> {
        let state = match self.state.try_lock() {
            Ok(state) => state,
            Err(std::sync::TryLockError::WouldBlock) => return AccountAvailability::Busy,
            Err(std::sync::TryLockError::Poisoned(_)) => return AccountAvailability::Unavailable,
        };
        if state.busy {
            AccountAvailability::Busy
        } else {
            AccountAvailability::Available(state)
        }
    }
}
