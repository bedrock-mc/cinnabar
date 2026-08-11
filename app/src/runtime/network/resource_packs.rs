use bevy::prelude::Resource;
use resource_pack::PackAdmission;

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
    pub(crate) fn replace_if_newer(&mut self, generation: u64, admission: PackAdmission) -> bool {
        if generation <= self.generation {
            return false;
        }
        self.generation = generation;
        self.admission = admission;
        true
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
        assert!(state.replace_if_newer(2, PackAdmission::Rejected(AdmissionError::MalformedZip)));
        assert!(!state.replace_if_newer(1, PackAdmission::None));
        assert!(!state.replace_if_newer(2, PackAdmission::None));
        assert_eq!(state.generation(), 2);
        assert!(matches!(
            state.admission(),
            PackAdmission::Rejected(AdmissionError::MalformedZip)
        ));
        assert!(state.replace_if_newer(3, PackAdmission::None));
        assert!(matches!(state.admission(), PackAdmission::None));
    }
}
