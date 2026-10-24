use {super::*, diagnostics::markers::cumulative_counter_delta};

/// The real telemetry system observes a long real frame even when virtual time is clamped.
#[test]
fn frame_metrics_observe_real_time_instead_of_the_virtual_delta() {
    use crate::runtime::{
        telemetry::{AcceptanceRuntimeConfig, record_metrics},
        visibility::{AppMetrics, CaveVisibilityCache, DiagnosticQuads},
        world::{ClientWorld, WorldStreamFramePoll},
    };
    use bevy::{diagnostic::DiagnosticsStore, prelude::*};
    let mut app = App::new();
    let mut real = Time::<bevy::time::Real>::default();
    real.advance_by(std::time::Duration::from_millis(400));
    let mut virtual_time = Time::<()>::default();
    virtual_time.advance_by(std::time::Duration::from_millis(10));
    app.insert_resource(real)
        .insert_resource(virtual_time)
        .insert_resource(ClientWorld::new(std::sync::Arc::new(
            assets::RuntimeAssets::diagnostic(),
        )))
        .insert_resource(AppMetrics(MetricsCollector::new()))
        .insert_resource(AcceptanceRuntimeConfig {
            build_profile: "test",
        })
        .init_resource::<CaveVisibilityCache>()
        .init_resource::<DiagnosticQuads>()
        .init_resource::<render_scene::ChunkRenderQueue>()
        .init_resource::<render_scene::TransparentSortMetrics>()
        .init_resource::<render_scene::ModelWorkloadMetrics>()
        .init_resource::<render_scene::VisibilityDiagnostics>()
        .init_resource::<render_scene::VisibilityDiagnosticsInput>()
        .init_resource::<DiagnosticsStore>()
        .init_resource::<client_runtime::publication::PublicationController>()
        .init_resource::<WorldStreamFramePoll>();
    #[cfg(feature = "acceptance")]
    app.insert_resource(::acceptance::AcceptanceRun::new(None, None, false, false))
        .init_resource::<client_presentation::local_player::LocalPlayerFrameCarrier>()
        .init_resource::<render_scene::TransparentWitnessEvidence>()
        .init_resource::<render_scene::ModelWitnessEvidence>();
    app.add_systems(Update, record_metrics);
    app.update();
    let metrics = &app.world().resource::<AppMetrics>().0;
    assert_eq!(metrics.frame_count(), 1);
    assert_eq!(metrics.report().max_frame_ms, 400.0);
}

#[test]
fn diagnostic_telemetry_refreshes_only_after_resident_attribution_changes() {
    let key = SubChunkKey::new(0, 1, 2, 3);
    let mut tracker = DiagnosticQuadTracker::default();
    let mut metrics = MetricsCollector::new();
    let mut revision = tracker.revision();

    assert!(refresh_diagnostic_attribution(&mut revision, &tracker, &mut metrics).is_none());
    tracker.upsert(
        key,
        &DiagnosticGeometrySummary::from_counts([DiagnosticGeometryCount::new(
            Some(54),
            537_536_753,
            6,
        )]),
    );
    let marker = refresh_diagnostic_attribution(&mut revision, &tracker, &mut metrics)
        .expect("changed diagnostic residency emits one marker");
    assert!(marker.contains("diagnostic_attribution_top=54|0x200a28f1|minecraft:leaf_litter|6"));
    assert!(
        refresh_diagnostic_attribution(&mut revision, &tracker, &mut metrics).is_none(),
        "unchanged frames must not rebuild or re-emit diagnostic attribution"
    );
    let unchanged_revision = tracker.revision();
    tracker.upsert(
        key,
        &DiagnosticGeometrySummary::from_counts([DiagnosticGeometryCount::new(
            Some(54),
            537_536_753,
            6,
        )]),
    );
    assert_eq!(tracker.revision(), unchanged_revision);
    assert!(
        refresh_diagnostic_attribution(&mut revision, &tracker, &mut metrics).is_none(),
        "an identical remesh must not increment revision or re-emit telemetry"
    );
    tracker.remove(key);
    let cleared = refresh_diagnostic_attribution(&mut revision, &tracker, &mut metrics)
        .expect("eviction publishes the cleared resident state");
    assert!(cleared.contains("diagnostic_attribution_total=0"));
}

#[test]
fn cumulative_counter_delta_tolerates_a_counter_reset() {
    assert_eq!(cumulative_counter_delta(9, 4), 5);
    assert_eq!(cumulative_counter_delta(2, 9), 2);
}
