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
    app.init_resource::<render_scene::RuntimeStageSpans>()
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

#[cfg(test)]
mod tests {
    use render_scene::{RuntimeStage, RuntimeStageProfiler, RuntimeStageSpans};
    use std::time::Duration;
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
