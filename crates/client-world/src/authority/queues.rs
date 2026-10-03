use super::*;

impl WorldAuthority {
    /// Counts consumer deltas that retain world admission credit.
    pub fn retained_commit_count(&self) -> usize {
        self.committed_controls
            .len()
            .saturating_add(self.committed_ui.len())
            .saturating_add(self.committed_audio.len())
            .saturating_add(self.committed_camera.len())
    }

    /// Returns undelivered controls to the front without changing their order.
    pub fn restore_committed_controls(
        &mut self,
        controls: impl DoubleEndedIterator<Item = CommittedControlEvent>,
    ) {
        for control in controls.rev() {
            assert!(self.committed_controls.len() < COMMITTED_CONTROL_CAPACITY);
            self.committed_controls.push_front(control);
        }
    }

    /// Drains committed control events in their original order.
    pub fn take_committed_controls(&mut self) -> Vec<CommittedControlEvent> {
        self.committed_controls.drain(..).collect()
    }
    /// Drains committed UI events in their original order.
    pub fn take_committed_ui(&mut self) -> Vec<CommittedUiEvent> {
        self.committed_ui.drain(..).collect()
    }
    /// Drains committed audio events in their original order.
    pub fn take_committed_audio(&mut self) -> Vec<CommittedAudioEvent> {
        self.committed_audio.drain(..).collect()
    }
    /// Drains committed particle events in their original order.
    pub fn take_committed_particles(&mut self) -> Vec<CommittedParticleEvent> {
        self.committed_particles.drain(..).collect()
    }
    /// Drains committed camera events in their original order.
    pub fn take_committed_camera(&mut self) -> Vec<CommittedCameraEvent> {
        self.committed_camera.drain(..).collect()
    }
}
