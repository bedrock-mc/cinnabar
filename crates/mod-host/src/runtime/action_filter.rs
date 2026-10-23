//! Independent synchronous input execution; no feature settings are retained here.

use super::*;
use std::sync::atomic::{AtomicU64, Ordering};

/// Full per-invocation fuel, independent of the deferrable presentation budget.
const FILTER_FUEL: u64 = FRAME_FUEL;
static NEXT_IDENTITY: AtomicU64 = AtomicU64::new(1);

/// Identifies a live instance across reorderings, replacements and reloads.
pub(super) fn next_identity() -> u64 {
    NEXT_IDENTITY
        .try_update(Ordering::Relaxed, Ordering::Relaxed, |v| v.checked_add(1))
        .expect("action filter identity exhausted")
}
impl Instance {
    /// Only explicitly exporting, active, granted instances can claim filter ownership.
    pub(crate) fn action_filter_identity(&self) -> Option<u64> {
        (self.is_active() && self.store.data().grants.controls && self.exports.has_filter())
            .then_some(self.filter_identity)
    }

    /// Reads the already committed pulse before output collection, without consuming it.
    pub(crate) fn attack_pulse_requested(&self) -> bool {
        self.is_active() && self.store.data().controls.interaction.attack_pulse
    }

    /// Runs on an independent allowance; this callback has no import write authority.
    /// Its return value is the complete transaction. Traps or malformed results quarantine.
    pub(crate) fn filter_actions(&mut self, frame: ActionFrame) -> Result<ActionVerdict> {
        anyhow::ensure!(
            self.action_filter_identity().is_some(),
            "action filter authority denied"
        );
        let count = frame.transitions.len();
        crate::execution::arm(&mut self.store, FILTER_FUEL, false)?;
        self.store.call_hook(|_, hook| {
            anyhow::ensure!(
                !matches!(hook, wasmtime::CallHook::CallingHost),
                "action filters cannot call host imports"
            );
            Ok(())
        });
        let result = self.exports.filter(&mut self.store, frame).and_then(|v| {
            anyhow::ensure!(
                v.accepted.len() == count,
                "action verdict length differs from observed transitions"
            );
            Ok(v)
        });
        self.store.call_hook(|_, _| Ok(()));
        self.last_fuel = FILTER_FUEL - self.store.get_fuel().unwrap_or(0).min(FILTER_FUEL);
        if result.is_err() {
            self.quarantine();
        }
        result
    }
}

impl cinnabar::extension::action_filter::Host for State {}
