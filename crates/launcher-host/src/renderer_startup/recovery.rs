//! Crash exclusions retain the adapter and driver that actually failed.

use super::*;

impl Recovery {
    /// Records an automatic crash and the adapter and driver that failed, when known.
    pub fn recover(&mut self, pending: &Attempt) {
        if pending.explicit {
            return;
        }
        self.failed_adapters
            .retain(|(backend, _)| *backend != pending.backend);
        if let Some(adapter) = &pending.adapter {
            self.failed_adapters
                .push((pending.backend, adapter.clone()));
        }
        if !self.failed.contains(&pending.backend) {
            self.failed.push(pending.backend);
        }
        if self
            .working
            .as_ref()
            .is_some_and(|working| working.backend == pending.backend)
        {
            self.working = None;
        }
    }

    /// The sentinel exclusions are applied after automatic vendor ordering.
    pub fn failed_backends(&self) -> &[Backend] {
        &self.failed
    }

    /// Allows a new driver or adapter set to begin a fresh recovery cycle.
    pub fn reset(&mut self) {
        self.failed.clear();
        self.failed_adapters.clear();
        self.working = None;
        self.direct_driver = None;
    }

    /// Remembers a completed choice while retaining exclusions for other crashed APIs.
    pub(super) fn succeeded(&mut self, mut attempt: Attempt) {
        attempt.stage = Stage::Ready;
        self.failed.retain(|backend| *backend != attempt.backend);
        self.failed_adapters
            .retain(|(backend, _)| *backend != attempt.backend);
        self.working = Some(attempt);
    }

    /// Skips unidentified crashes before enumeration; identified failures can check for updates.
    pub fn enumeration_exclusions(&self) -> Vec<Backend> {
        self.failed
            .iter()
            .copied()
            .filter(|backend| {
                !self
                    .failed_adapters
                    .iter()
                    .any(|(known, _)| known == backend)
            })
            .collect()
    }
}

impl StartupGuard {
    /// Clears a device exclusion when its adapter or driver changes, and reports any remaining ban.
    pub fn adapter_excluded(&mut self, backend: Backend, adapter: &Adapter) -> Result<bool> {
        let _active = termination::active()?;
        self.require_lease()?;
        if self
            .recovery
            .failed_adapters
            .iter()
            .any(|(known, previous)| *known == backend && previous != adapter)
        {
            self.recovery.failed.retain(|known| *known != backend);
            self.recovery
                .failed_adapters
                .retain(|(known, _)| *known != backend);
            save(&self.root.join(CHOICE_FILE), &self.recovery)?;
        }
        Ok(self.recovery.failed.contains(&backend))
    }

    /// Clears exhausted exclusions for the allowed APIs once, without changing driver metadata.
    pub fn retry_exclusions(&mut self, allowed: &[Backend]) -> Result<bool> {
        let _active = termination::active()?;
        self.require_lease()?;
        let changed = self
            .recovery
            .failed
            .iter()
            .any(|backend| allowed.contains(backend));
        if changed {
            self.recovery
                .failed
                .retain(|backend| !allowed.contains(backend));
            self.recovery
                .failed_adapters
                .retain(|(backend, _)| !allowed.contains(backend));
            save(&self.root.join(CHOICE_FILE), &self.recovery)?;
        }
        Ok(changed)
    }
}

#[cfg(test)]
mod tests;
