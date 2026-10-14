//! Deterministic frame, event and session-loading allowances shared across local instances.

use server_experience::runtime::LOAD_FUEL;
use std::sync::{Arc, Mutex};

/// Expensive event and loading lanes each admit one complete callback per frame.
const LARGE_CALLBACKS_PER_FRAME: usize = 1;

#[derive(Default)]
struct Allowance {
    frame: u64,
    events: usize,
    loads: usize,
}

/// Whether a guest callback ran or must be delivered in a later frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CallbackOutcome {
    Executed,
    Deferred,
}

/// Preserves each callback's full fuel limit instead of trapping on a partial frame allowance.
#[derive(Clone)]
pub struct CallbackBudget(Arc<Mutex<Allowance>>);
impl CallbackBudget {
    /// Creates the shared regular-frame allowance and bounded event/loading lanes.
    pub fn new(fuel: u64) -> Self {
        let budget = Self(Arc::new(Mutex::new(Allowance::default())));
        budget.reset(fuel);
        budget
    }
    /// Starts a new frame before any guest input or lifecycle callback.
    pub fn reset(&self, fuel: u64) {
        *self.0.lock().expect("mod callback budget") = Allowance {
            frame: fuel,
            events: LARGE_CALLBACKS_PER_FRAME,
            loads: LARGE_CALLBACKS_PER_FRAME,
        };
    }
    /// Reserves a full callback allowance, or defers it without reducing its guest fuel limit.
    pub(crate) fn reserve(&self, fuel: u64) -> Option<u64> {
        let mut remaining = self.0.lock().expect("mod callback budget");
        if fuel == LOAD_FUEL {
            if remaining.loads == 0 {
                return None;
            }
            remaining.loads -= 1;
        } else if fuel > crate::FRAME_FUEL {
            if remaining.events == 0 {
                return None;
            }
            remaining.events -= 1;
        } else {
            if remaining.frame < fuel {
                return None;
            }
            remaining.frame -= fuel;
        }
        Some(fuel)
    }
    /// Returns regular-frame fuel which a completed callback did not use.
    pub(crate) fn refund(&self, fuel: u64) {
        self.0.lock().expect("mod callback budget").frame += fuel;
    }
    /// Whether the loading lane can admit another hot replacement this frame.
    pub(crate) fn can_load(&self) -> bool {
        self.0.lock().expect("mod callback budget").loads != 0
    }
    /// Reports regular-frame fuel remaining after completed frame callbacks.
    pub fn remaining(&self) -> u64 {
        self.0.lock().expect("mod callback budget").frame
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reservations_never_turn_a_partial_frame_allowance_into_a_guest_fuel_limit() {
        let budget = CallbackBudget::new(crate::FRAME_FUEL - 1);
        assert_eq!(budget.reserve(crate::FRAME_FUEL), None);
        assert_eq!(budget.remaining(), crate::FRAME_FUEL - 1);
        assert_eq!(budget.reserve(LOAD_FUEL), Some(LOAD_FUEL));
        assert_eq!(budget.reserve(LOAD_FUEL), None);
        let event = server_experience::runtime::CALLBACK_FUEL;
        assert_eq!(budget.reserve(event), Some(event));
        assert_eq!(budget.reserve(event), None);
        budget.reset(mod_api::TOTAL_MOD_FRAME_FUEL);
        assert_eq!(budget.reserve(LOAD_FUEL), Some(LOAD_FUEL));
    }
}
