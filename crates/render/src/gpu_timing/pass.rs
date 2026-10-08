use super::*;
use std::sync::LazyLock;

/// Reads the shared UI diagnostic flag once; ordinary rendering never formats a report.
pub(crate) fn ui_profiling_requested() -> bool {
    static REQUESTED: LazyLock<bool> =
        LazyLock::new(|| std::env::var("RUST_MCBE_GPU_UI").as_deref() == Ok("1"));
    *REQUESTED
}

/// Attaches supported owned-pass queries without adding marker passes or submissions.
pub(crate) fn render_pass_timestamps(
    world: &World,
    stage: RuntimeStage,
) -> Option<wgpu::RenderPassTimestampWrites<'_>> {
    world.get_resource::<GpuTimestamps>()?.render_pass(stage)
}

/// Separates owned UI passes only when requested, retaining the legacy category otherwise.
pub(crate) fn ui_pass_timestamps(
    world: &World,
    category: RuntimeStage,
) -> Option<wgpu::RenderPassTimestampWrites<'_>> {
    debug_assert!(RuntimeStage::GPU_UI.contains(&category));
    let timestamps = world.get_resource::<GpuTimestamps>()?;
    let stage = if timestamps.ui_categories {
        category
    } else {
        RuntimeStage::GpuUi
    };
    timestamps.render_pass(stage)
}

impl GpuTimestamps {
    /// UI diagnostics replace enclosing graph spans to avoid counting the same work twice.
    pub(super) fn graph_span_enabled(&self, stage: RuntimeStage) -> bool {
        !cfg!(target_os = "macos")
            && !(self.ui_categories
                && (stage == RuntimeStage::GpuUi || RuntimeStage::GPU_UI.contains(&stage)))
    }

    /// Uses owned pass queries on Metal and for explicitly requested UI categories.
    pub(super) fn render_pass(
        &self,
        stage: RuntimeStage,
    ) -> Option<wgpu::RenderPassTimestampWrites<'_>> {
        let ui_pass = self.ui_categories
            && (stage == RuntimeStage::GpuUi || RuntimeStage::GPU_UI.contains(&stage));
        if !cfg!(target_os = "macos") && !ui_pass {
            return None;
        }
        let span = self.open_pass(stage)?;
        Some(wgpu::RenderPassTimestampWrites {
            query_set: span.queries,
            beginning_of_pass_write_index: Some(span.begin),
            end_of_pass_write_index: Some(span.begin + 1),
        })
    }
}
