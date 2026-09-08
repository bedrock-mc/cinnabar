use std::fmt;

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
    loading_started_millis: Option<u64>,
    frame_generation_baseline: u64,
    readiness_frame_baseline: Option<u64>,
    released: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum StartupReadinessClass {
    DenseOpaque,
    Drained,
}

impl fmt::Display for StartupReadinessClass {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::DenseOpaque => "dense_opaque",
            Self::Drained => "drained",
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct StartupLoadingMilestone {
    app_elapsed_ms: u64,
    pub(super) loading_wait_ms: u64,
    session_generation: u64,
    diagnostics_frame_generation: u64,
    readiness_frame_baseline: u64,
    gpu_frame_generation: u64,
    gpu_opaque_count: u64,
    readiness: StartupReadinessClass,
}

impl fmt::Display for StartupLoadingMilestone {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} phase=terrain_ready app_elapsed_ms={} loading_wait_ms={} session_generation={} diagnostics_frame_generation={} readiness_frame_baseline={} gpu_frame_generation={} gpu_opaque_count={} readiness={}",
            crate::acceptance::markers::LOADING_MILESTONE,
            self.app_elapsed_ms,
            self.loading_wait_ms,
            self.session_generation,
            self.diagnostics_frame_generation,
            self.readiness_frame_baseline,
            self.gpu_frame_generation,
            self.gpu_opaque_count,
            self.readiness,
        )
    }
}

impl StartupPresentationState {
    pub(super) fn observe_with_milestone(
        &mut self,
        input: StartupReadinessInput,
        now_millis: u64,
    ) -> (bool, Option<StartupLoadingMilestone>) {
        let valid_session = input.connected && input.session_generation != 0;
        let session_changed =
            valid_session && self.session_generation != Some(input.session_generation);
        if !valid_session {
            self.loading_started_millis = None;
        } else if session_changed {
            self.loading_started_millis = Some(now_millis);
        }
        let was_released = self.released && !session_changed;
        let released = self.observe(input);
        if !released || was_released {
            return (released, None);
        }

        let gpu_completed_opaque = input
            .snapshot
            .gpu_completed_opaque
            .expect("startup release requires a GPU-completed opaque digest");
        let readiness = if input.visible_rendered >= MIN_VISIBLE_TERRAIN_BEFORE_PRESENTATION
            && gpu_completed_opaque.count != 0
        {
            StartupReadinessClass::DenseOpaque
        } else {
            StartupReadinessClass::Drained
        };
        let loading_started_millis = self.loading_started_millis.unwrap_or(now_millis);
        (
            true,
            Some(StartupLoadingMilestone {
                app_elapsed_ms: now_millis,
                loading_wait_ms: now_millis.saturating_sub(loading_started_millis),
                session_generation: input.session_generation,
                diagnostics_frame_generation: input.diagnostics_frame_generation,
                readiness_frame_baseline: self.readiness_frame_baseline.unwrap_or(0),
                gpu_frame_generation: input.snapshot.frame_generation,
                gpu_opaque_count: gpu_completed_opaque.count,
                readiness,
            }),
        )
    }

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

    #[test]
    fn loading_milestone_is_produced_once_on_the_initial_release_transition() {
        let mut state = StartupPresentationState::default();
        let mut ready = startup_input(7, 10, 10, 3);
        ready.visible_rendered = MIN_VISIBLE_TERRAIN_BEFORE_PRESENTATION;
        let (released, milestone) = state.observe_with_milestone(ready, 1_000);
        assert!(!released);
        assert!(milestone.is_none());

        ready.diagnostics_frame_generation = 11;
        ready.snapshot.frame_generation = 11;
        let (released, milestone) = state.observe_with_milestone(ready, 1_125);
        assert!(released);
        let milestone = milestone.expect("the release transition produces one milestone");
        assert_eq!(milestone.loading_wait_ms, 125);

        let (released, repeated) = state.observe_with_milestone(ready, 1_250);
        assert!(released);
        assert!(repeated.is_none(), "a released session must not emit twice");
    }

    #[test]
    fn loading_milestone_rejects_disconnected_zero_and_stale_gpu_observations() {
        let mut state = StartupPresentationState::default();
        let mut invalid = startup_input(0, 4, 4, 1);
        invalid.visible_rendered = MIN_VISIBLE_TERRAIN_BEFORE_PRESENTATION;
        assert!(state.observe_with_milestone(invalid, 10).1.is_none());
        invalid.session_generation = 1;
        invalid.connected = false;
        assert!(state.observe_with_milestone(invalid, 20).1.is_none());

        invalid.connected = true;
        assert!(state.observe_with_milestone(invalid, 30).1.is_none());
        assert!(state.observe_with_milestone(invalid, 40).1.is_none());
        invalid.diagnostics_frame_generation = 5;
        invalid.snapshot.frame_generation = 5;
        assert!(state.observe_with_milestone(invalid, 50).1.is_some());
    }

    #[test]
    fn loading_milestone_resets_timing_for_disconnect_and_changed_session() {
        let mut state = StartupPresentationState::default();
        let mut ready = startup_input(1, 0, 0, 1);
        ready.visible_rendered = MIN_VISIBLE_TERRAIN_BEFORE_PRESENTATION;
        assert!(state.observe_with_milestone(ready, 100).1.is_none());
        ready.diagnostics_frame_generation = 1;
        ready.snapshot.frame_generation = 1;
        assert_eq!(
            state
                .observe_with_milestone(ready, 175)
                .1
                .expect("first session milestone")
                .loading_wait_ms,
            75
        );

        ready.connected = false;
        assert!(state.observe_with_milestone(ready, 200).1.is_none());
        ready.connected = true;
        ready.session_generation = 2;
        assert!(state.observe_with_milestone(ready, 900).1.is_none());
        ready.diagnostics_frame_generation = 2;
        ready.snapshot.frame_generation = 2;
        assert_eq!(
            state
                .observe_with_milestone(ready, 950)
                .1
                .expect("new session milestone")
                .loading_wait_ms,
            50
        );

        ready.session_generation = 3;
        assert!(state.observe_with_milestone(ready, 2_000).1.is_none());
        ready.diagnostics_frame_generation = 3;
        ready.snapshot.frame_generation = 3;
        assert_eq!(
            state
                .observe_with_milestone(ready, 2_025)
                .1
                .expect("changed session milestone without a disconnect observation")
                .loading_wait_ms,
            25
        );
    }

    #[test]
    fn first_valid_observation_defines_wait_start() {
        let mut state = StartupPresentationState::default();
        let mut ready = startup_input(4, 8, 8, 1);
        ready.visible_rendered = MIN_VISIBLE_TERRAIN_BEFORE_PRESENTATION;
        assert!(state.observe_with_milestone(ready, 5_000).1.is_none());
        ready.diagnostics_frame_generation = 9;
        ready.snapshot.frame_generation = 9;
        assert_eq!(
            state
                .observe_with_milestone(ready, 5_040)
                .1
                .expect("milestone after first observed session")
                .loading_wait_ms,
            40
        );
    }

    #[test]
    fn drained_zero_opaque_release_reports_its_bounded_readiness_class() {
        let mut state = StartupPresentationState::default();
        let mut ready = startup_input(9, 20, 20, 0);
        ready.cohort_target_complete = true;
        ready.stream_work_drained = true;
        ready.render_work_drained = true;
        assert!(state.observe_with_milestone(ready, 500).1.is_none());
        ready.diagnostics_frame_generation = 21;
        ready.snapshot.frame_generation = 21;
        let milestone = state
            .observe_with_milestone(ready, 550)
            .1
            .expect("drained release milestone");
        assert_eq!(milestone.readiness, super::StartupReadinessClass::Drained);
        assert_eq!(milestone.gpu_opaque_count, 0);
    }

    #[test]
    fn loading_wait_saturates_if_the_monotonic_sample_regresses() {
        let mut state = StartupPresentationState::default();
        let mut ready = startup_input(5, 1, 1, 1);
        ready.visible_rendered = MIN_VISIBLE_TERRAIN_BEFORE_PRESENTATION;
        assert!(state.observe_with_milestone(ready, 100).1.is_none());
        ready.diagnostics_frame_generation = 2;
        ready.snapshot.frame_generation = 2;
        assert_eq!(
            state
                .observe_with_milestone(ready, 90)
                .1
                .expect("release milestone")
                .loading_wait_ms,
            0
        );
    }

    #[test]
    fn loading_milestone_has_an_exact_bounded_private_data_free_format() {
        let mut state = StartupPresentationState::default();
        let mut ready = startup_input(7, 10, 10, 3);
        ready.visible_rendered = MIN_VISIBLE_TERRAIN_BEFORE_PRESENTATION;
        assert!(state.observe_with_milestone(ready, 1_000).1.is_none());
        ready.diagnostics_frame_generation = 11;
        ready.snapshot.frame_generation = 11;
        let milestone = state
            .observe_with_milestone(ready, 1_125)
            .1
            .expect("release milestone");
        assert_eq!(
            milestone.to_string(),
            "RUST_MCBE_LOADING_MILESTONE phase=terrain_ready app_elapsed_ms=1125 loading_wait_ms=125 session_generation=7 diagnostics_frame_generation=11 readiness_frame_baseline=10 gpu_frame_generation=11 gpu_opaque_count=3 readiness=dense_opaque"
        );
    }
}
