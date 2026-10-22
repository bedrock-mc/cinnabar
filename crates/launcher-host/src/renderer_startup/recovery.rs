//! Backend-only crash exclusions and one retry after usable candidates are exhausted.

use super::*;

impl Recovery {
    /// Records a backend crash without tying recovery to adapter identity; an explicit crash
    /// marks only that choice, leaving automatic order untouched.
    pub fn recover(&mut self, pending: &Attempt) {
        if pending.explicit {
            self.crashed_choice = Some(pending.backend);
            return;
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

    /// Returns the APIs excluded after interrupted automatic startup.
    pub fn failed_backends(&self) -> &[Backend] {
        &self.failed
    }

    /// The explicit API that stopped an earlier startup under the current settings revision.
    pub fn crashed_choice(&self) -> Option<Backend> {
        self.crashed_choice
    }

    /// Starts a fresh recovery cycle while retaining the version and settings revision.
    pub fn reset(&mut self) {
        self.failed.clear();
        self.working = None;
        self.crashed_choice = None;
    }

    /// Remembers a completed choice while retaining exclusions for other crashed APIs.
    pub(super) fn succeeded(&mut self, mut attempt: Attempt) {
        attempt.stage = Stage::Ready;
        self.failed.retain(|backend| *backend != attempt.backend);
        self.crashed_choice
            .take_if(|backend| *backend == attempt.backend);
        self.working = Some(attempt);
    }
}

impl StartupGuard {
    /// Clears allowed exclusions once after the caller exhausts all usable candidates.
    pub fn retry_exclusions(&mut self, allowed: &[Backend]) -> Result<bool> {
        let _active = termination::active()?;
        self.require_lease()?;
        if self.retried
            || allowed.is_empty()
            || !allowed
                .iter()
                .any(|backend| self.recovery.failed.contains(backend))
        {
            return Ok(false);
        }
        self.recovery
            .failed
            .retain(|backend| !allowed.contains(backend));
        save(&self.root.join(CHOICE_FILE), &self.recovery)?;
        self.retried = true;
        Ok(true)
    }
}

#[cfg(test)]
mod tests;
