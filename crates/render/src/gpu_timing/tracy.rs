use super::{GpuFrameTimes, RuntimeStage};
use std::sync::LazyLock;
use tracy_client::{Client, PlotName};

static PLOTS: LazyLock<[PlotName; RuntimeStage::GPU.len()]> = LazyLock::new(|| {
    RuntimeStage::GPU
        .map(|stage| PlotName::new_leak(format!("{} elapsed ms (readback)", stage.name())))
});

/// Creates each static plot name once during startup, before frame readbacks.
pub(super) fn initialize() {
    LazyLock::force(&PLOTS);
}

/// Plots delayed pass latencies without inventing GPU execution times on the CPU timeline.
pub(super) fn record(frame: &GpuFrameTimes) {
    let Some(client) = Client::running() else {
        return;
    };
    for (stage, name) in RuntimeStage::GPU.into_iter().zip(PLOTS.iter()) {
        if let Some(elapsed) = frame.get(stage) {
            client.plot(*name, elapsed.as_secs_f64() * 1_000.0);
        }
    }
}

/// Marks post-present intervals; completion submissions and render cleanup may follow.
pub(super) fn install(app: &mut bevy::app::SubApp) {
    use bevy::prelude::*;
    use bevy::render::{Render, RenderSystems, renderer::render_system};
    app.add_systems(
        Render,
        frame_mark
            .after(render_system)
            .in_set(RenderSystems::Render),
    );
}

/// Uses Tracy's default frame series at the post-present boundary.
fn frame_mark() {
    if let Some(client) = Client::running() {
        client.frame_mark();
    }
}
