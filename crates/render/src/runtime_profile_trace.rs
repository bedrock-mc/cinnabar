use render_scene::RuntimeStage;
/// Brackets surface preparation to distinguish drawable waits from game work.
pub(crate) fn install_surface_trace(app: &mut bevy::app::SubApp) {
    use bevy::prelude::*;
    use bevy::render::{
        Render, RenderSystems, renderer::render_system, view::window::prepare_windows,
    };
    const SUBMISSION: usize = RuntimeStage::RenderSubmission as usize;
    const SURFACE: usize = RuntimeStage::SurfacePreparation as usize;
    const FRAME: usize = RuntimeStage::RenderFrame as usize;
    app.add_systems(bevy::render::ExtractSchedule, extract_frame_identity)
        .init_resource::<render_scene::RuntimeStageSpans>()
        .add_systems(
            Render,
            (
                render_scene::begin_stage_span::<SURFACE>.before(prepare_windows),
                render_scene::end_stage_span::<SURFACE>.after(prepare_windows),
            )
                .in_set(RenderSystems::PrepareViews),
        )
        // Spans all render-world work; the acquisition wait is subtracted at the end.
        .add_systems(
            Render,
            (
                render_scene::begin_stage_span::<FRAME>.before(RenderSystems::ExtractCommands),
                render_scene::runtime_profile::end_render_frame_span.in_set(RenderSystems::Cleanup),
            ),
        )
        .add_systems(
            Render,
            (
                render_scene::begin_stage_span::<SUBMISSION>.before(render_system),
                render_scene::end_stage_span::<SUBMISSION>.after(render_system),
            )
                .in_set(RenderSystems::Render),
        );
}

/// Carries the input identity into the render world before its queued commands run.
fn extract_frame_identity(
    mut commands: bevy::prelude::Commands,
    profiler: bevy::render::Extract<bevy::prelude::Res<render_scene::RuntimeStageProfiler>>,
) {
    commands.insert_resource(profiler.for_frame());
}

#[cfg(test)]
mod tests {
    use render_scene::{RuntimeStage, RuntimeStageProfiler, RuntimeStageSpans};
    use std::time::Duration;

    /// Extracted input identities reach submission, including the frame opened before commands apply.
    #[test]
    fn extraction_carries_each_input_identity_through_submission() {
        use bevy::ecs::schedule::ScheduleLabel;
        use bevy::prelude::*;
        use bevy::render::{Render, RenderApp, RenderSystems, extract_plugin::ExtractPlugin};
        /// Starts a distinct input frame before extracting its render data.
        fn input_frame(profiler: Res<RuntimeStageProfiler>) {
            profiler.begin_frame(true, false);
            profiler.trace_frame(true, false, 0.0);
        }
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("identity.json");
        let profiler = RuntimeStageProfiler::with_trace(true, Some(path.clone()));
        let mut app = App::new();
        app.insert_resource(profiler.clone())
            .add_systems(First, input_frame)
            .add_plugins(ExtractPlugin::<RenderApp>::new(
                |_, _| {},
                Render::base_schedule,
                Render.intern(),
                RenderSystems::ExtractCommands.intern(),
                RenderSystems::PostCleanup.intern(),
            ));
        let render = app.sub_app_mut(RenderApp);
        render.insert_resource(profiler.clone());
        render.update_schedule = Some(Render.intern());
        super::install_surface_trace(render);
        app.update();
        app.update();
        profiler.flush_trace();
        let saved: serde_json::Value =
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        for name in ["render_submission", "render_frame"] {
            let ids: Vec<_> = saved["traceEvents"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|event| event["name"] == name)
                .map(|event| event["args"]["frame_id"].as_u64().unwrap())
                .collect();
            assert_eq!(ids, [1, 2], "{name}");
        }
    }
    /// The render-frame span is open from asset preparation through rendering, closed by Cleanup.
    #[test]
    fn render_frame_span_covers_asset_preparation_through_cleanup() {
        use bevy::prelude::*;
        use bevy::render::{Render, RenderSystems};
        const FRAME: usize = RuntimeStage::RenderFrame as usize;
        #[derive(Resource, Default)]
        struct Open(Vec<bool>);
        /// Records whether the frame span covers the current render stage.
        fn probe(spans: Res<RuntimeStageSpans>, mut open: ResMut<Open>) {
            open.0.push(spans.is_open(RuntimeStage::RenderFrame));
        }
        let profiler = RuntimeStageProfiler::new(true);
        let mut app = App::new();
        app.add_schedule(Render::base_schedule())
            .insert_resource(profiler.clone())
            .init_resource::<Open>();
        crate::runtime_profile_trace::install_surface_trace(app.main_mut());
        app.add_systems(
            Render,
            (
                probe.in_set(RenderSystems::PrepareAssets),
                probe.in_set(RenderSystems::Render),
                probe.in_set(RenderSystems::PostCleanup),
            )
                .chain(),
        );
        app.world_mut().run_schedule(Render);
        assert_eq!(app.world().resource::<Open>().0, [true, true, false]);
        let snapshot = profiler.take_snapshot_if_due(Duration::ZERO).unwrap();
        assert_eq!(snapshot.samples[FRAME].count, 1);
    }
}
