use std::time::Duration;

use bevy::prelude::{App, IntoScheduleConfigs, ResMut, Resource, Update};
use render::ChunkUploadBudget;

use crate::app::{ClientFrameSet, configure_client_frame_schedule};
use crate::runtime::publication::{
    PublicationController, PublicationControllerConfig, PublicationFrameWork,
    adaptive_publication_diagnostic_line,
};

#[derive(Resource, Default)]
struct ObservedClientFrameOrder(Vec<ClientFrameSet>);

#[test]
fn client_frame_schedule_executes_every_behavioral_barrier_in_contract_order() {
    let mut app = App::new();
    app.init_resource::<ObservedClientFrameOrder>();
    configure_client_frame_schedule(&mut app);
    app.add_systems(
        Update,
        (
            (|mut order: ResMut<ObservedClientFrameOrder>| {
                order.0.push(ClientFrameSet::RawInput);
            })
            .in_set(ClientFrameSet::RawInput),
            (|mut order: ResMut<ObservedClientFrameOrder>| {
                order.0.push(ClientFrameSet::SemanticSample);
            })
            .in_set(ClientFrameSet::SemanticSample),
            (|mut order: ResMut<ObservedClientFrameOrder>| {
                order.0.push(ClientFrameSet::UiAuthority);
            })
            .in_set(ClientFrameSet::UiAuthority),
            (|mut order: ResMut<ObservedClientFrameOrder>| {
                order.0.push(ClientFrameSet::SemanticFinalize);
            })
            .in_set(ClientFrameSet::SemanticFinalize),
            (|mut order: ResMut<ObservedClientFrameOrder>| {
                order.0.push(ClientFrameSet::Physics);
            })
            .in_set(ClientFrameSet::Physics),
            (|mut order: ResMut<ObservedClientFrameOrder>| {
                order.0.push(ClientFrameSet::Camera);
            })
            .in_set(ClientFrameSet::Camera),
            (|mut order: ResMut<ObservedClientFrameOrder>| {
                order.0.push(ClientFrameSet::Interaction);
            })
            .in_set(ClientFrameSet::Interaction),
            (|mut order: ResMut<ObservedClientFrameOrder>| {
                order.0.push(ClientFrameSet::WorldPublication);
            })
            .in_set(ClientFrameSet::WorldPublication),
            (|mut order: ResMut<ObservedClientFrameOrder>| {
                order.0.push(ClientFrameSet::ActorPublication);
            })
            .in_set(ClientFrameSet::ActorPublication),
            (|mut order: ResMut<ObservedClientFrameOrder>| {
                order.0.push(ClientFrameSet::UiPublication);
            })
            .in_set(ClientFrameSet::UiPublication),
            (|mut order: ResMut<ObservedClientFrameOrder>| {
                order.0.push(ClientFrameSet::NetworkSend);
            })
            .in_set(ClientFrameSet::NetworkSend),
        ),
    );

    app.update();

    assert_eq!(
        app.world().resource::<ObservedClientFrameOrder>().0,
        [
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
        ]
    );
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
    assert!(source.contains("drive_world_stream.before(ChunkRenderApplySet)"));
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
