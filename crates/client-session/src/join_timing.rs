//! Generation-fenced milestones from the join action through terrain presentation.

use std::time::Duration;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum JoinPhase {
    BridgeReady,
    Bootstrap,
    LoadingReleased,
    TerrainReady,
}

impl JoinPhase {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::BridgeReady => "bridge_ready",
            Self::Bootstrap => "bootstrap",
            Self::LoadingReleased => "loading_released",
            Self::TerrainReady => "terrain_ready",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct JoinMilestone {
    pub generation: u64,
    pub phase: JoinPhase,
    pub elapsed: Duration,
    pub since_previous: Duration,
}

#[derive(Debug)]
pub struct JoinTimeline {
    generation: u64,
    started: Duration,
    previous: Duration,
    phase: Option<JoinPhase>,
    terrain_frame: Option<u64>,
}

impl JoinTimeline {
    pub const fn new(generation: u64, started: Duration) -> Self {
        Self {
            generation,
            started,
            previous: started,
            phase: None,
            terrain_frame: None,
        }
    }

    pub fn needs_terrain_witness(&self, generation: u64) -> bool {
        generation == self.generation && self.phase != Some(JoinPhase::TerrainReady)
    }

    /// Emits each forward milestone once, ignoring stale generations and clock regressions.
    pub fn observe(
        &mut self,
        generation: u64,
        phase: JoinPhase,
        now: Duration,
    ) -> Option<JoinMilestone> {
        if generation != self.generation || self.phase.is_some_and(|previous| previous >= phase) {
            return None;
        }
        let now = now.max(self.previous);
        let milestone = JoinMilestone {
            generation,
            phase,
            elapsed: now.saturating_sub(self.started),
            since_previous: now.saturating_sub(self.previous),
        };
        self.previous = now;
        self.phase = Some(phase);
        Some(milestone)
    }

    /// Terrain arriving after initialization needs its own later GPU frame witness.
    pub fn observe_terrain(
        &mut self,
        generation: u64,
        local_ready: bool,
        frame_generation: u64,
        gpu_frame_generation: Option<u64>,
        now: Duration,
    ) -> Option<JoinMilestone> {
        if generation != self.generation || self.phase == Some(JoinPhase::TerrainReady) {
            return None;
        }
        if !local_ready {
            self.terrain_frame = None;
            return None;
        }
        let baseline = *self.terrain_frame.get_or_insert(frame_generation);
        if gpu_frame_generation.is_some_and(|frame| frame > baseline) {
            self.observe(generation, JoinPhase::TerrainReady, now)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn join_elapsed_includes_provisioning_and_terrain_after_bootstrap() {
        let mut timeline = JoinTimeline::new(4, Duration::from_millis(100));
        assert_eq!(
            timeline
                .observe(4, JoinPhase::BridgeReady, Duration::from_millis(150))
                .unwrap()
                .elapsed,
            Duration::from_millis(50)
        );
        let bootstrap = timeline
            .observe(4, JoinPhase::Bootstrap, Duration::from_millis(300))
            .unwrap();
        assert_eq!(bootstrap.since_previous, Duration::from_millis(150));
        let terrain = timeline
            .observe(4, JoinPhase::TerrainReady, Duration::from_millis(600))
            .unwrap();
        assert_eq!(terrain.elapsed, Duration::from_millis(500));
        assert_eq!(terrain.since_previous, Duration::from_millis(300));
        assert!(
            timeline
                .observe(4, JoinPhase::TerrainReady, Duration::from_secs(1))
                .is_none()
        );
    }

    #[test]
    fn stale_or_reordered_milestones_cannot_change_the_active_join() {
        let mut timeline = JoinTimeline::new(8, Duration::from_secs(1));
        assert!(
            timeline
                .observe(7, JoinPhase::TerrainReady, Duration::from_secs(20))
                .is_none()
        );
        let first = timeline
            .observe(8, JoinPhase::Bootstrap, Duration::ZERO)
            .unwrap();
        assert_eq!(first.elapsed, Duration::ZERO);
        assert!(
            timeline
                .observe(8, JoinPhase::BridgeReady, Duration::from_secs(2))
                .is_none()
        );
        let last = timeline
            .observe(8, JoinPhase::TerrainReady, Duration::from_secs(2))
            .unwrap();
        assert_eq!(last.elapsed, Duration::from_secs(1));
        assert_eq!(last.since_previous, Duration::from_secs(1));
    }

    #[test]
    fn loading_release_without_terrain_does_not_complete_the_presentation_witness() {
        let mut timeline = JoinTimeline::new(2, Duration::ZERO);
        assert!(timeline.needs_terrain_witness(2));
        assert!(!timeline.needs_terrain_witness(1));
        timeline
            .observe(2, JoinPhase::LoadingReleased, Duration::from_millis(100))
            .unwrap();
        assert!(
            timeline
                .observe_terrain(2, false, 5, Some(5), Duration::from_millis(200))
                .is_none()
        );
        assert!(
            timeline
                .observe_terrain(1, true, 8, Some(10), Duration::from_millis(250))
                .is_none()
        );
        assert!(
            timeline
                .observe_terrain(2, true, 9, Some(9), Duration::from_millis(300))
                .is_none()
        );
        assert!(
            timeline
                .observe_terrain(2, false, 10, Some(10), Duration::from_millis(400))
                .is_none()
        );
        assert!(
            timeline
                .observe_terrain(2, true, 11, Some(11), Duration::from_millis(450))
                .is_none()
        );
        let ready = timeline
            .observe_terrain(2, true, 12, Some(12), Duration::from_millis(500))
            .unwrap();
        assert_eq!(ready.elapsed, Duration::from_millis(500));
        assert!(!timeline.needs_terrain_witness(2));
        assert!(
            timeline
                .observe_terrain(2, true, 13, Some(13), Duration::from_millis(550))
                .is_none()
        );
    }
}
