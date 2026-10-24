//! Account admission observes the original membership independently of worker completion.

use crate::{local_worlds::NativeHostIntent, native_account::HostSlot};
use std::sync::{Arc, Mutex};

/// Retains the original slot, whose pending directory has no strong reference back to its manager.
#[derive(Clone, Default)]
pub(in crate::launcher_account) struct Retirement(Arc<Mutex<Option<HostSlot>>>);

impl Retirement {
    /// Called by the sole coordinator only after its previous membership cleanup succeeded.
    pub(super) fn select(&self, intent: &NativeHostIntent) {
        *self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(intent.host_slot());
    }

    /// Reads existing reservation state; no network operation or server manager is retained here.
    pub(in crate::launcher_account) fn is_empty(&self) -> bool {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
            .is_none_or(HostSlot::is_empty)
    }
}
