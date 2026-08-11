use bevy::prelude::Resource;
use resource_pack::PackAdmission;

pub(crate) const fn bootstrap_session_generation_is_expected(
    ui_generation: u64,
    world_generation: u64,
    incoming_generation: u64,
) -> bool {
    ui_generation == world_generation
        && matches!(
            world_generation.checked_add(1),
            Some(expected) if expected == incoming_generation
        )
}

pub(crate) const fn bootstrap_session_generation_is_stale(
    ui_generation: u64,
    world_generation: u64,
    incoming_generation: u64,
) -> bool {
    incoming_generation <= world_generation || incoming_generation < ui_generation
}

/// Generation-bound admission for the current session's optional pack stack.
/// Asset application remains unavailable; this resource only owns validated bytes.
#[derive(Debug, Resource)]
pub(crate) struct ResourcePackAdmissionState {
    generation: u64,
    admission: PackAdmission,
}

impl Default for ResourcePackAdmissionState {
    fn default() -> Self {
        Self {
            generation: 0,
            admission: PackAdmission::None,
        }
    }
}

impl ResourcePackAdmissionState {
    /// Starts ownership for a pending generation and releases the prior stack.
    pub(crate) fn begin_generation(&mut self, generation: u64) -> bool {
        if generation <= self.generation {
            return false;
        }
        self.generation = generation;
        self.admission = PackAdmission::None;
        true
    }

    /// Publishes admission only for the pending/current or a newer generation.
    pub(crate) fn replace_for_generation(
        &mut self,
        generation: u64,
        admission: PackAdmission,
    ) -> bool {
        if generation < self.generation {
            return false;
        }
        self.generation = generation;
        self.admission = admission;
        true
    }

    /// Releases admission when the current network session terminates.
    pub(crate) fn clear_current(&mut self) {
        self.admission = PackAdmission::None;
    }

    #[cfg(test)]
    pub(crate) const fn generation(&self) -> u64 {
        self.generation
    }

    #[cfg(test)]
    pub(crate) const fn admission(&self) -> &PackAdmission {
        &self.admission
    }
}

#[cfg(test)]
mod tests {
    use resource_pack::{AdmissionError, PackAdmission};

    use super::ResourcePackAdmissionState;

    #[test]
    fn newer_generation_replaces_atomically_and_stale_results_are_ignored() {
        let mut state = ResourcePackAdmissionState::default();
        assert!(state.begin_generation(2));
        assert!(matches!(state.admission(), PackAdmission::None));
        assert!(
            state.replace_for_generation(2, PackAdmission::Rejected(AdmissionError::MalformedZip))
        );
        assert!(!state.replace_for_generation(1, PackAdmission::None));
        assert_eq!(state.generation(), 2);
        assert!(matches!(
            state.admission(),
            PackAdmission::Rejected(AdmissionError::MalformedZip)
        ));
        assert!(state.begin_generation(3));
        assert!(matches!(state.admission(), PackAdmission::None));
        assert!(!state.begin_generation(2));
        state.clear_current();
        assert_eq!(state.generation(), 3);
        assert!(matches!(state.admission(), PackAdmission::None));
    }
}
