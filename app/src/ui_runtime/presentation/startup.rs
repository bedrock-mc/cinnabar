use render::VisibilityDiagnosticSnapshot;

pub(super) const MIN_VISIBLE_TERRAIN_BEFORE_PRESENTATION: usize = 1_024;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct StartupReadinessInput {
    pub(super) session_generation: u64,
    pub(super) connected: bool,
    pub(super) diagnostics_frame_generation: u64,
    pub(super) snapshot: VisibilityDiagnosticSnapshot,
    pub(super) visible_rendered: usize,
    pub(super) cohort_target_complete: bool,
    pub(super) stream_work_drained: bool,
    pub(super) render_work_drained: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct StartupPresentationState {
    session_generation: Option<u64>,
    frame_generation_baseline: u64,
    readiness_frame_baseline: Option<u64>,
    released: bool,
}

impl StartupPresentationState {
    pub(super) fn observe(&mut self, input: StartupReadinessInput) -> bool {
        let latest_frame_generation = input
            .diagnostics_frame_generation
            .max(input.snapshot.frame_generation);
        if !input.connected || input.session_generation == 0 {
            self.reset(latest_frame_generation);
            return false;
        }
        if self.session_generation != Some(input.session_generation) {
            self.session_generation = Some(input.session_generation);
            // A completion callback can lag the main-world session transition.
            // Baseline both counters so no pre-session callback can release this
            // session, even when the latest callback has not published yet.
            self.frame_generation_baseline = latest_frame_generation;
            self.readiness_frame_baseline = None;
            self.released = false;
        }
        if self.released {
            return true;
        }

        let dense_view_ready = input.visible_rendered >= MIN_VISIBLE_TERRAIN_BEFORE_PRESENTATION;
        let bounded_small_or_zero_opaque_view_ready =
            input.cohort_target_complete && input.stream_work_drained && input.render_work_drained;
        if !dense_view_ready && !bounded_small_or_zero_opaque_view_ready {
            self.readiness_frame_baseline = None;
            return false;
        }

        let readiness_baseline = self
            .readiness_frame_baseline
            .get_or_insert(latest_frame_generation);
        let Some(gpu_completed_opaque) = input.snapshot.gpu_completed_opaque else {
            return false;
        };
        if input.snapshot.frame_generation <= self.frame_generation_baseline
            || input.snapshot.frame_generation <= *readiness_baseline
        {
            return false;
        }
        if (dense_view_ready && gpu_completed_opaque.count != 0)
            || bounded_small_or_zero_opaque_view_ready
        {
            self.released = true;
        }
        self.released
    }

    pub(super) const fn probe_enabled(self, connected: bool) -> bool {
        connected && !self.released
    }

    fn reset(&mut self, frame_generation: u64) {
        self.session_generation = None;
        self.frame_generation_baseline = frame_generation;
        self.readiness_frame_baseline = None;
        self.released = false;
    }
}

#[cfg(test)]
mod tests {
    use render::{VisibilityDiagnosticSnapshot, VisibilityKeyDigest};

    use super::{
        MIN_VISIBLE_TERRAIN_BEFORE_PRESENTATION, StartupPresentationState, StartupReadinessInput,
    };

    fn startup_input(
        session_generation: u64,
        diagnostics_frame_generation: u64,
        gpu_frame_generation: u64,
        opaque_count: u64,
    ) -> StartupReadinessInput {
        StartupReadinessInput {
            session_generation,
            connected: true,
            diagnostics_frame_generation,
            snapshot: VisibilityDiagnosticSnapshot {
                frame_generation: gpu_frame_generation,
                gpu_completed_opaque: Some(VisibilityKeyDigest {
                    count: opaque_count,
                    hash: opaque_count,
                }),
                ..VisibilityDiagnosticSnapshot::default()
            },
            visible_rendered: 0,
            cohort_target_complete: false,
            stream_work_drained: false,
            render_work_drained: false,
        }
    }

    #[test]
    fn dense_view_requires_a_gpu_completed_frame_after_population_threshold() {
        let mut state = StartupPresentationState::default();
        assert!(!state.observe(startup_input(1, 0, 0, 0)));

        let mut evidence = startup_input(1, 1, 1, 0);
        evidence.visible_rendered = MIN_VISIBLE_TERRAIN_BEFORE_PRESENTATION - 1;
        assert!(!state.observe(evidence));

        evidence.visible_rendered = MIN_VISIBLE_TERRAIN_BEFORE_PRESENTATION;
        assert!(!state.observe(evidence));

        evidence.diagnostics_frame_generation = 2;
        evidence.snapshot.frame_generation = 2;
        assert!(!state.observe(evidence));

        evidence.diagnostics_frame_generation = 3;
        evidence.snapshot.frame_generation = 3;
        evidence.snapshot.gpu_completed_opaque = Some(VisibilityKeyDigest { count: 1, hash: 1 });
        assert!(state.observe(evidence));
    }

    #[test]
    fn small_or_zero_opaque_view_waits_for_drain_and_a_later_gpu_frame() {
        let mut state = StartupPresentationState::default();
        assert!(!state.observe(startup_input(1, 0, 0, 0)));

        let mut zero_opaque = startup_input(1, 1, 1, 0);
        assert!(!state.observe(zero_opaque));

        zero_opaque.stream_work_drained = true;
        zero_opaque.render_work_drained = true;
        assert!(!state.observe(zero_opaque));

        zero_opaque.cohort_target_complete = true;
        assert!(!state.observe(zero_opaque));

        zero_opaque.diagnostics_frame_generation = 2;
        zero_opaque.snapshot.frame_generation = 2;
        assert!(state.observe(zero_opaque));
    }

    #[test]
    fn rejects_gpu_snapshot_at_or_before_session_baseline() {
        let mut state = StartupPresentationState::default();
        let mut baseline = startup_input(1, 7, 7, 1);
        baseline.visible_rendered = MIN_VISIBLE_TERRAIN_BEFORE_PRESENTATION;
        baseline.snapshot.gpu_completed_opaque = None;
        assert!(!state.observe(baseline));

        let mut stale = baseline;
        stale.snapshot.gpu_completed_opaque = Some(VisibilityKeyDigest { count: 1, hash: 1 });
        assert!(!state.observe(stale));
        stale.diagnostics_frame_generation = 8;
        stale.snapshot.frame_generation = 8;
        assert!(state.observe(stale));
    }

    #[test]
    fn resets_on_disconnect_and_requires_new_session_evidence() {
        let mut state = StartupPresentationState::default();
        let mut ready = startup_input(1, 0, 0, 1);
        ready.visible_rendered = MIN_VISIBLE_TERRAIN_BEFORE_PRESENTATION;
        assert!(!state.observe(ready));
        ready.diagnostics_frame_generation = 1;
        ready.snapshot.frame_generation = 1;
        assert!(state.observe(ready));

        let mut disconnected = ready;
        disconnected.connected = false;
        assert!(!state.observe(disconnected));

        let mut stale_new_session = ready;
        stale_new_session.session_generation = 2;
        assert!(!state.observe(stale_new_session));
        stale_new_session.diagnostics_frame_generation = 2;
        stale_new_session.snapshot.frame_generation = 2;
        assert!(state.observe(stale_new_session));
    }

    #[test]
    fn stays_released_when_later_visibility_changes() {
        let mut state = StartupPresentationState::default();
        let mut ready = startup_input(1, 0, 0, 1);
        ready.visible_rendered = MIN_VISIBLE_TERRAIN_BEFORE_PRESENTATION;
        assert!(!state.observe(ready));
        ready.diagnostics_frame_generation = 1;
        ready.snapshot.frame_generation = 1;
        assert!(state.observe(ready));

        let mut later = startup_input(1, 2, 2, 0);
        later.visible_rendered = 0;
        later.stream_work_drained = false;
        later.render_work_drained = false;
        assert!(state.observe(later));
    }
}
