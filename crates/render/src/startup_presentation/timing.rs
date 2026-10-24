//! CPU attribution for the initial render frames, including synchronous driver compilation.

use bevy::{
    prelude::*,
    render::{
        Render, RenderApp, RenderSystems, render_resource::PipelineCache, renderer::render_system,
    },
};
use render_scene::startup_timing::{self, RenderFrame};
use web_time::Instant;

#[derive(Resource, Default)]
struct FrameTiming {
    started: Option<Instant>,
    submission: Option<Instant>,
    frame: RenderFrame,
}

/// Installs bounded render measurements without enabling the full frame profiler.
pub(crate) fn install(app: &mut App) {
    let Some(render) = app.get_sub_app_mut(RenderApp) else {
        return;
    };
    render.init_resource::<FrameTiming>().add_systems(
        Render,
        (
            begin.before(RenderSystems::ExtractCommands),
            compile
                .after(RenderSystems::Prepare)
                .before(RenderSystems::Render),
            submission
                .in_set(RenderSystems::Render)
                .before(render_system),
            finish
                .in_set(RenderSystems::Render)
                .after(render_system)
                .before(crate::device_poll::FrameSubmissions),
        )
            .run_if(recording),
    );
}

/// Starts the initial frame before asset preparation and view setup.
fn begin(mut timing: ResMut<FrameTiming>) {
    timing.started = Some(Instant::now());
    timing.frame = RenderFrame::default();
}

/// Processes queued pipelines before Bevy's empty second pass, separating compilation from draw.
fn compile(mut timing: ResMut<FrameTiming>, mut cache: ResMut<PipelineCache>) {
    let Some(started) = timing.started else {
        return;
    };
    timing.frame.prepare_ms = started.elapsed().as_secs_f64() * 1000.0;
    let start = Instant::now();
    let _span = bevy::log::info_span!("startup.pipeline_and_shader").entered();
    cache.process_queue();
    timing.frame.pipeline_and_shader_ms = start.elapsed().as_secs_f64() * 1000.0;
    timing.frame.pipelines = cache.pipelines().count();
    timing.frame.pending_pipelines = cache.waiting_pipelines().count();
    for pipeline in cache.pipelines() {
        let label = match &pipeline.descriptor {
            bevy::render::render_resource::PipelineDescriptor::RenderPipelineDescriptor(
                descriptor,
            ) => descriptor.label.as_deref(),
            bevy::render::render_resource::PipelineDescriptor::ComputePipelineDescriptor(
                descriptor,
            ) => descriptor.label.as_deref(),
            bevy::render::render_resource::PipelineDescriptor::MeshPipelineDescriptor(
                descriptor,
            ) => descriptor.label.as_deref(),
        }
        .unwrap_or("unlabeled");
        *timing
            .frame
            .pipeline_labels
            .entry(label.into())
            .or_default() += 1;
    }
}

/// Marks command encoding, submission and presentation after pipeline processing.
fn submission(mut timing: ResMut<FrameTiming>) {
    timing.submission = Some(Instant::now());
}

/// Stores the bounded initial sample; a completed menu report ignores subsequent frames.
fn finish(mut timing: ResMut<FrameTiming>) {
    let Some(started) = timing.started.take() else {
        return;
    };
    timing.frame.total_ms = started.elapsed().as_secs_f64() * 1000.0;
    timing.frame.submit_ms = timing
        .submission
        .take()
        .map_or(0.0, |start| start.elapsed().as_secs_f64() * 1000.0);
    if let Some(startup) = startup_timing::current() {
        startup.frame(std::mem::take(&mut timing.frame));
    }
}

/// Stops startup-only schedule work after the one presentation report.
fn recording() -> bool {
    startup_timing::current().is_some_and(|timing| timing.is_recording())
}
