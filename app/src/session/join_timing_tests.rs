use super::*;
use client_session::join_timing::JoinPhase;
use client_ui::ui_runtime::presentation::startup::{
    StartupPresentationState, StartupReadinessInput,
};
use render::VisibilityDiagnosticsInput;
use render_model::{VisibilityDiagnosticSnapshot, VisibilityKeyDigest};

#[test]
fn automatic_transfers_keep_original_action_time_and_a_user_join_resets_it() {
    let mut controller = SessionController::default();
    controller.begin_join(2, Duration::from_secs(10), JoinOrigin::MenuAction);
    assert_eq!(
        controller.action_elapsed(Duration::from_secs(1)),
        Duration::from_secs(1)
    );
    controller.join_timeline = None;
    controller.begin_join(3, Duration::from_secs(12), JoinOrigin::ServerTransfer);
    assert_eq!(
        controller.action_elapsed(Duration::from_secs(3)),
        Duration::from_secs(5)
    );
    controller.join_timeline = None;
    controller.begin_join(4, Duration::from_secs(16), JoinOrigin::ServerTransfer);
    assert_eq!(
        controller.action_elapsed(Duration::from_secs(2)),
        Duration::from_secs(8)
    );
    controller.begin_join(5, Duration::from_secs(20), JoinOrigin::MenuAction);
    assert_eq!(
        controller.action_elapsed(Duration::from_secs(1)),
        Duration::from_secs(1)
    );
}

#[test]
fn initial_join_timing_keeps_its_origin_and_does_not_restart() {
    let mut controller = SessionController::default();
    let generation = INITIAL_SESSION_GENERATION;
    assert!(!controller.needs_terrain_witness(generation));
    controller.begin_initial_join();
    assert_eq!(controller.join_origin, JoinOrigin::DirectSession);
    assert!(controller.needs_terrain_witness(generation));
    assert!(!controller.needs_terrain_witness(generation + 1));
    controller.observe_join(generation, JoinPhase::Bootstrap);
    controller.observe_join_terrain(generation, true, 3, Some(3));
    controller.begin_initial_join();
    controller.observe_join_terrain(generation, true, 4, Some(4));
    assert!(!controller.needs_terrain_witness(generation));

    let replacement = controller.next_generation();
    controller.begin_join(
        replacement,
        controller.join_clock.elapsed(),
        JoinOrigin::MenuAction,
    );
    controller.begin_initial_join();
    assert_eq!(controller.join_origin, JoinOrigin::MenuAction);
    assert!(!controller.needs_terrain_witness(generation));
    assert!(controller.needs_terrain_witness(replacement));
}

fn update_probe(
    diagnostics: &mut VisibilityDiagnosticsInput,
    startup: StartupPresentationState,
    controller: &SessionController,
    generation: u64,
) {
    crate::ui_runtime::presentation::publish::update_startup_probe(
        diagnostics,
        startup,
        true,
        Some(controller),
        generation,
    );
}

#[test]
fn startup_probe_survives_empty_loading_release_until_terrain_is_presented() {
    let mut controller = SessionController::default();
    let generation = INITIAL_SESSION_GENERATION;
    controller.begin_initial_join();
    let mut startup = StartupPresentationState::default();
    let mut diagnostics = VisibilityDiagnosticsInput::new(false);
    let mut input = StartupReadinessInput {
        session_generation: generation,
        connected: true,
        ..Default::default()
    };
    assert!(!startup.observe(input));
    update_probe(&mut diagnostics, startup, &controller, generation);
    diagnostics.advance([], []);
    input.cohort_target_complete = true;
    input.stream_work_drained = true;
    input.render_work_drained = true;
    input.diagnostics_frame_generation = diagnostics.frame_generation();
    input.snapshot = VisibilityDiagnosticSnapshot {
        frame_generation: input.diagnostics_frame_generation,
        gpu_completed_opaque: Some(VisibilityKeyDigest::default()),
        ..Default::default()
    };
    assert!(!startup.observe(input));
    diagnostics.advance([], []);
    input.diagnostics_frame_generation = diagnostics.frame_generation();
    input.snapshot.frame_generation = input.diagnostics_frame_generation;
    assert!(startup.observe(input));
    assert!(!startup.probe_enabled(true));
    controller.observe_join(generation, JoinPhase::LoadingReleased);

    update_probe(&mut diagnostics, startup, &controller, generation);
    let loading_frame = diagnostics.frame_generation();
    diagnostics.advance([], []);
    assert!(diagnostics.frame_generation() > loading_frame);
    let readiness_frame = diagnostics.frame_generation();
    controller.observe_join_terrain(generation, true, readiness_frame, Some(loading_frame));
    assert!(controller.needs_terrain_witness(generation));
    update_probe(&mut diagnostics, startup, &controller, generation);
    diagnostics.advance([], []);
    controller.observe_join_terrain(
        generation,
        true,
        diagnostics.frame_generation(),
        Some(diagnostics.frame_generation()),
    );
    assert!(!controller.needs_terrain_witness(generation));
    update_probe(&mut diagnostics, startup, &controller, generation);
    assert!(!diagnostics.startup_probe_enabled());
    let completed_frame = diagnostics.frame_generation();
    diagnostics.advance([], []);
    assert_eq!(diagnostics.frame_generation(), completed_frame);
}
