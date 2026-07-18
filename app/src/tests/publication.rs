use std::time::Duration;

use bevy::{
    ecs::schedule::{IntoSystemSet, NodeId, ScheduleGraph, Schedules, SystemSet},
    prelude::{App, Update},
};
use render::ChunkUploadBudget;

use crate::app::{
    ClientFrameSet, configure_acceptance_finish_system, configure_client_frame_schedule,
    configure_client_production_frame_systems,
};
use crate::local_player::{
    publish_interaction_origin, publish_local_player_frame, resolve_camera_pose,
};
use crate::movement::advance_local_physics;
use crate::runtime::network::{publish_actor_render_frame, receive_network_events};
use crate::runtime::phase3_evidence::emit_phase3_evidence;
use crate::runtime::publication::{
    PublicationController, PublicationControllerConfig, PublicationFrameWork,
    adaptive_publication_diagnostic_line,
};
use crate::runtime::shutdown::finish_acceptance_run;
use crate::runtime::telemetry::send_player_auth_inputs;
use crate::runtime::world::{drive_world_stream, reconcile_world_stream_before_physics};
use crate::semantic_controls::{
    collect_raw_input, finalize_semantic_input_after_ui_authority, route_semantic_input,
    synchronize_semantic_input_authority,
};
use crate::ui_runtime::presentation::publish_ui_runtime;

#[test]
fn production_client_systems_are_members_of_the_eleven_behavioral_sets() {
    let mut app = App::new();
    configure_client_frame_schedule(&mut app);
    configure_client_production_frame_systems(&mut app);

    let schedules = app.world().resource::<Schedules>();
    let graph = schedules
        .get(Update)
        .expect("production Update schedule")
        .graph();
    let stages = [
        ClientFrameSet::RawInput,
        ClientFrameSet::SemanticSample,
        ClientFrameSet::UiAuthority,
        ClientFrameSet::SemanticFinalize,
        ClientFrameSet::Physics,
        ClientFrameSet::Camera,
        ClientFrameSet::Interaction,
        ClientFrameSet::WorldPublication,
        ClientFrameSet::ActorPublication,
        ClientFrameSet::UiPublication,
        ClientFrameSet::NetworkSend,
    ];

    for adjacent in stages.windows(2) {
        assert!(
            graph.dependency().graph().contains_edge(
                stage_node(graph, adjacent[0]),
                stage_node(graph, adjacent[1]),
            ),
            "{:?} must execute directly before {:?}",
            adjacent[0],
            adjacent[1]
        );
    }

    assert_system_in_stage(
        graph,
        collect_raw_input,
        "collect_raw_input",
        ClientFrameSet::RawInput,
    );
    assert_system_in_stage(
        graph,
        route_semantic_input,
        "route_semantic_input",
        ClientFrameSet::SemanticSample,
    );
    assert_system_in_stage(
        graph,
        synchronize_semantic_input_authority,
        "synchronize_semantic_input_authority",
        ClientFrameSet::UiAuthority,
    );
    assert_system_in_stage(
        graph,
        finalize_semantic_input_after_ui_authority,
        "finalize_semantic_input_after_ui_authority",
        ClientFrameSet::SemanticFinalize,
    );
    assert_system_in_stage(
        graph,
        advance_local_physics,
        "advance_local_physics",
        ClientFrameSet::Physics,
    );
    assert_system_in_stage(
        graph,
        resolve_camera_pose,
        "resolve_camera_pose",
        ClientFrameSet::Camera,
    );
    assert_system_in_stage(
        graph,
        publish_local_player_frame,
        "publish_local_player_frame",
        ClientFrameSet::Interaction,
    );
    assert_system_in_stage(
        graph,
        publish_interaction_origin,
        "publish_interaction_origin",
        ClientFrameSet::Interaction,
    );
    assert_system_in_stage(
        graph,
        drive_world_stream,
        "drive_world_stream",
        ClientFrameSet::WorldPublication,
    );
    assert_system_in_stage(
        graph,
        publish_actor_render_frame,
        "publish_actor_render_frame",
        ClientFrameSet::ActorPublication,
    );
    assert_system_in_stage(
        graph,
        publish_ui_runtime,
        "publish_ui_runtime",
        ClientFrameSet::UiPublication,
    );
    assert_system_in_stage(
        graph,
        send_player_auth_inputs,
        "send_player_auth_inputs",
        ClientFrameSet::NetworkSend,
    );
    assert!(
        graph.dependency().graph().contains_edge(
            system_node(graph, emit_phase3_evidence, "emit_phase3_evidence"),
            system_node(graph, send_player_auth_inputs, "send_player_auth_inputs"),
        ),
        "the exact build/session/PREG/BREG identity marker must precede every candidate packet",
    );
    assert_system_in_stage(
        graph,
        emit_phase3_evidence,
        "emit_phase3_evidence",
        ClientFrameSet::NetworkSend,
    );

    assert!(
        graph.dependency().graph().contains_edge(
            system_node(
                graph,
                publish_local_player_frame,
                "publish_local_player_frame"
            ),
            system_node(
                graph,
                publish_interaction_origin,
                "publish_interaction_origin"
            ),
        ),
        "the atomic local-player frame must publish before its interaction consumer"
    );
    assert!(
        graph.dependency().graph().contains_edge(
            system_node(graph, receive_network_events, "receive_network_events"),
            stage_node(graph, ClientFrameSet::Physics),
        ),
        "correction/session/dimension ingress must invalidate state before Physics and Interaction"
    );
    assert!(
        graph.dependency().graph().contains_edge(
            system_node(
                graph,
                reconcile_world_stream_before_physics,
                "reconcile_world_stream_before_physics",
            ),
            stage_node(graph, ClientFrameSet::Physics),
        ),
        "committed correction and dimension reconciliation must finish before Physics",
    );
}

#[test]
fn acceptance_terminal_runs_after_the_authoritative_network_send_stage() {
    let mut app = App::new();
    configure_client_frame_schedule(&mut app);
    configure_client_production_frame_systems(&mut app);
    configure_acceptance_finish_system(&mut app);

    let schedules = app.world().resource::<Schedules>();
    let graph = schedules
        .get(Update)
        .expect("production Update schedule")
        .graph();
    assert!(
        graph.dependency().graph().contains_edge(
            stage_node(graph, ClientFrameSet::NetworkSend),
            system_node(graph, finish_acceptance_run, "finish_acceptance_run"),
        ),
        "the terminal evidence marker must sample the outbox only after the final send attempt",
    );
}

fn stage_node(graph: &ScheduleGraph, stage: ClientFrameSet) -> NodeId {
    let key = graph
        .system_sets
        .get_key(stage.intern())
        .unwrap_or_else(|| panic!("missing production stage {stage:?}"));
    NodeId::Set(key)
}

fn assert_system_in_stage<M>(
    graph: &ScheduleGraph,
    system: impl IntoSystemSet<M>,
    label: &str,
    stage: ClientFrameSet,
) {
    assert!(
        graph
            .hierarchy()
            .graph()
            .contains_edge(stage_node(graph, stage), system_node(graph, system, label)),
        "production system {label} is not a member of {stage:?}"
    );
}

fn system_node<M>(graph: &ScheduleGraph, system: impl IntoSystemSet<M>, label: &str) -> NodeId {
    let type_set = graph
        .system_sets
        .get_key(system.into_system_set().intern())
        .unwrap_or_else(|| panic!("missing production system type set {label}"));
    let parent = NodeId::Set(type_set);
    let mut matches = graph.systems.iter().filter_map(|(key, _, _)| {
        let child = NodeId::System(key);
        graph
            .hierarchy()
            .graph()
            .contains_edge(parent, child)
            .then_some(child)
    });
    let node = matches
        .next()
        .unwrap_or_else(|| panic!("missing production system {label}"));
    assert!(
        matches.next().is_none(),
        "production system {label} is registered more than once"
    );
    node
}

fn test_config() -> PublicationControllerConfig {
    PublicationControllerConfig {
        target_frame_time: Duration::from_millis(16),
        recovery_frame_time: Duration::from_millis(12),
        recovery_streak_frames: 3,
        minimum: ChunkUploadBudget::new(2, 256 * 1024),
        initial: ChunkUploadBudget::new(16, 4 * 1024 * 1024),
        maximum: ChunkUploadBudget::new(32, 8 * 1024 * 1024),
        additive_items: 1,
        additive_bytes: 256 * 1024,
        decrease_numerator: 3,
        decrease_denominator: 4,
    }
}

#[test]
fn over_target_frames_multiplicatively_reduce_both_publication_caps() {
    let mut controller = PublicationController::new(test_config());

    controller.begin_frame(Duration::from_millis(25));

    assert_eq!(
        controller.budget(),
        ChunkUploadBudget::new(12, 3 * 1024 * 1024)
    );
    assert_eq!(controller.diagnostics().multiplicative_decreases, 1);
    assert_eq!(controller.diagnostics().under_target_streak, 0);
}

#[test]
fn recovery_is_additive_only_after_a_sustained_under_target_streak() {
    let mut controller = PublicationController::new(test_config());
    controller.begin_frame(Duration::from_millis(25));
    let reduced = controller.budget();

    for _ in 0..2 {
        controller.begin_frame(Duration::from_millis(8));
        assert_eq!(controller.budget(), reduced);
    }
    controller.begin_frame(Duration::from_millis(8));

    assert_eq!(
        controller.budget(),
        ChunkUploadBudget::new(
            reduced.max_per_frame + 1,
            reduced.max_bytes_per_frame + 256 * 1024
        )
    );
    assert_eq!(controller.diagnostics().additive_increases, 1);
}

#[test]
fn neutral_or_spiky_frames_reset_recovery_and_hard_caps_are_never_exceeded() {
    let mut controller = PublicationController::new(test_config());
    controller.begin_frame(Duration::from_millis(8));
    controller.begin_frame(Duration::from_millis(14));
    controller.begin_frame(Duration::from_millis(8));
    controller.begin_frame(Duration::from_millis(8));
    assert_eq!(controller.diagnostics().under_target_streak, 2);

    for _ in 0..1000 {
        controller.begin_frame(Duration::from_millis(8));
    }

    assert_eq!(controller.budget(), test_config().maximum);
}

#[test]
fn per_frame_work_distinguishes_backlog_from_visibility_loss() {
    let mut controller = PublicationController::new(test_config());
    controller.begin_frame(Duration::from_millis(10));
    controller.finish_frame(PublicationFrameWork {
        mesh_jobs_dispatched: 7,
        mesh_changes_published: 5,
        mesh_bytes_published: 900_000,
        pending_mesh_jobs: 123,
        in_flight_mesh_jobs: 11,
        upload_queue_items: 17,
        upload_queue_bytes: 2_000_000,
        cohort_expected: 1_089,
        cohort_loaded: 900,
        resident_meshes: 850,
        cave_visible_meshes: 700,
        frustum_visible_meshes: 410,
        submitted_meshes: 410,
        gpu_completed_meshes: 410,
    });

    let diagnostics = controller.diagnostics();
    assert_eq!(diagnostics.last_work.pending_mesh_jobs, 123);
    assert_eq!(diagnostics.last_work.frustum_visible_meshes, 410);
    assert_eq!(diagnostics.last_work.submitted_meshes, 410);
    assert_eq!(diagnostics.last_work.gpu_completed_meshes, 410);
}

#[test]
fn adaptive_publication_diagnostic_is_deterministic_and_cohort_tagged() {
    let mut controller = PublicationController::new(test_config());
    controller.begin_frame(Duration::from_millis(10));
    controller.finish_frame(PublicationFrameWork {
        mesh_jobs_dispatched: 7,
        mesh_changes_published: 5,
        mesh_bytes_published: 900_000,
        pending_mesh_jobs: 123,
        in_flight_mesh_jobs: 11,
        upload_queue_items: 17,
        upload_queue_bytes: 2_000_000,
        cohort_expected: 1_089,
        cohort_loaded: 900,
        resident_meshes: 850,
        cave_visible_meshes: 700,
        frustum_visible_meshes: 410,
        submitted_meshes: 410,
        gpu_completed_meshes: 410,
    });

    assert_eq!(
        adaptive_publication_diagnostic_line(controller.diagnostics()),
        "ADAPTIVE_PUBLICATION frame=1 frame_us=10000 cap_items=16 cap_bytes=4194304 under_target_streak=1 decreases=0 increases=0 dispatched=7 published=5 published_bytes=900000 pending=123 in_flight=11 upload_items=17 upload_bytes=2000000 cohort_loaded=900 cohort_expected=1089 resident=850 cave=700 frustum=410 submitted=410 gpu_completed=410"
    );
}

#[test]
fn application_wires_controller_before_world_handoff_and_render_apply() {
    let source = include_str!("../app.rs");

    assert!(source.contains("insert_resource(PublicationController::default())"));
    let publication_frame = source[source
        .rfind("begin_publication_frame")
        .expect("publication frame system is registered")..]
        .split_once(".add_systems(")
        .expect("publication frame registration is bounded")
        .0;
    assert!(publication_frame.contains(".before(ChunkRenderApplySet)"));
    let world_publication = source[source
        .rfind("drive_world_stream")
        .expect("world publication system is registered")..]
        .split_once(".add_systems(")
        .expect("world publication registration is bounded")
        .0;
    assert!(world_publication.contains(".after(receive_network_events)"));
    assert!(world_publication.contains(".before(ChunkRenderApplySet)"));
}

#[test]
fn local_player_pipeline_orders_physics_camera_and_interaction_and_has_one_camera_writer() {
    let app_source = include_str!("../app.rs");
    assert!(app_source.contains("LocalPlayerFrameSet::Physics"));
    assert!(app_source.contains("LocalPlayerFrameSet::Camera"));
    assert!(app_source.contains("LocalPlayerFrameSet::Interaction"));

    let production_sources = [
        include_str!("../camera.rs"),
        include_str!("../movement.rs"),
        include_str!("../runtime/network.rs"),
        include_str!("../runtime/world.rs"),
        include_str!("../local_player.rs"),
    ];
    assert_eq!(
        production_sources
            .iter()
            .map(|source| source.matches("&mut Transform").count())
            .sum::<usize>(),
        1,
        "only the CameraPose publication system may mutate the camera Transform"
    );

    let local_player = include_str!("../local_player.rs");
    let resolver = local_player
        .split_once("pub(crate) fn resolve_camera_pose")
        .expect("camera resolver exists")
        .1
        .split_once("pub(crate) fn publish_interaction_origin")
        .expect("camera resolver has a bounded body")
        .0;
    assert!(
        resolver.contains("collision_safe_perspective_pose("),
        "the sole production camera writer must use the swept collision solver"
    );
}

#[test]
fn deterministic_streaming_trace_bounds_frame_spikes_against_fixed_128() {
    const WORK_ITEMS: usize = 1_024;
    const ITEM_BYTES: u64 = 128 * 1024;
    const BASE_FRAME_MS: u64 = 4;
    const ITEM_COST_MS: u64 = 2;

    let fixed_frames = WORK_ITEMS.div_ceil(128);
    let fixed_peak_ms = BASE_FRAME_MS + 128 * ITEM_COST_MS;

    let mut controller = PublicationController::default();
    let mut remaining = WORK_ITEMS;
    let mut adaptive_frames = 0;
    let mut adaptive_peak_ms = 0;
    while remaining != 0 {
        let budget = controller.budget();
        let byte_limited = usize::try_from(budget.max_bytes_per_frame / ITEM_BYTES).unwrap();
        let published = remaining.min(budget.max_per_frame).min(byte_limited);
        assert_ne!(
            published, 0,
            "minimum caps must guarantee deterministic progress"
        );
        remaining -= published;
        adaptive_frames += 1;
        let frame_ms = BASE_FRAME_MS + u64::try_from(published).unwrap() * ITEM_COST_MS;
        adaptive_peak_ms = adaptive_peak_ms.max(frame_ms);
        controller.begin_frame(Duration::from_millis(frame_ms));
    }

    println!(
        "publication_trace before_fixed128_frames={fixed_frames} before_peak_ms={fixed_peak_ms} after_adaptive_frames={adaptive_frames} after_peak_ms={adaptive_peak_ms}"
    );
    assert_eq!((fixed_frames, fixed_peak_ms), (8, 260));
    assert_eq!((adaptive_frames, adaptive_peak_ms), (128, 20));
}

#[test]
fn default_controller_tolerates_60hz_fifo_jitter_and_recovers_additively() {
    let mut controller = PublicationController::default();
    let initial = controller.budget();
    let jitter = [
        Duration::from_micros(15_800),
        Duration::from_micros(16_667),
        Duration::from_micros(17_900),
        Duration::from_micros(16_200),
    ];

    for frame in 0..240 {
        controller.begin_frame(jitter[frame % jitter.len()]);
    }

    assert_eq!(controller.diagnostics().multiplicative_decreases, 0);
    assert_eq!(controller.diagnostics().additive_increases, 2);
    assert_eq!(controller.budget().max_per_frame, initial.max_per_frame + 2);
}

#[test]
fn genuine_stalls_decrease_immediately_then_60hz_frames_recover_conservatively() {
    let mut controller = PublicationController::default();
    let initial = controller.budget();

    controller.begin_frame(Duration::from_millis(80));
    let reduced = controller.budget();
    assert!(reduced.max_per_frame < initial.max_per_frame);

    for frame in 0..119 {
        let paced = if frame % 2 == 0 {
            Duration::from_micros(16_200)
        } else {
            Duration::from_micros(17_600)
        };
        controller.begin_frame(paced);
    }
    assert_eq!(controller.budget(), reduced);

    controller.begin_frame(Duration::from_micros(16_667));
    assert_eq!(controller.budget().max_per_frame, reduced.max_per_frame + 1);
    assert_eq!(controller.diagnostics().multiplicative_decreases, 1);
    assert_eq!(controller.diagnostics().additive_increases, 1);
}
