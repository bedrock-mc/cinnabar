use super::*;
use render_scene::RuntimeStage;
use std::sync::LazyLock;

/// Reads the shared UI diagnostic flag once; ordinary rendering never formats a report.
pub(crate) fn ui_profiling_requested() -> bool {
    static REQUESTED: LazyLock<bool> =
        LazyLock::new(|| env_compat::var("CINNABAR_GPU_UI").as_deref() == Ok("1"));
    *REQUESTED
}

/// Attaches supported owned-pass queries without adding marker passes or submissions.
pub(crate) fn render_pass_timestamps(
    world: &World,
    stage: RuntimeStage,
) -> Option<wgpu::RenderPassTimestampWrites<'_>> {
    world.get_resource::<GpuTimestamps>()?.render_pass(stage)
}

/// Times an existing compute pass only in detailed non-Metal frames.
pub(crate) fn compute_pass_timestamps(
    world: &World,
    stage: RuntimeStage,
) -> Option<wgpu::ComputePassTimestampWrites<'_>> {
    let timestamps = world.get_resource::<GpuTimestamps>()?;
    if cfg!(target_os = "macos") || !timestamps.pass_detail {
        return None;
    }
    let span = timestamps.open_pass(stage)?;
    Some(wgpu::ComputePassTimestampWrites {
        query_set: span.queries,
        beginning_of_pass_write_index: Some(span.begin),
        end_of_pass_write_index: Some(span.begin + 1),
    })
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

    /// Uses owned queries on Metal, for UI diagnostics, and for detailed late terrain.
    pub(super) fn render_pass(
        &self,
        stage: RuntimeStage,
    ) -> Option<wgpu::RenderPassTimestampWrites<'_>> {
        let ui_pass = self.ui_categories
            && (stage == RuntimeStage::GpuUi || RuntimeStage::GPU_UI.contains(&stage));
        if stage == RuntimeStage::GpuTerrainLate {
            if cfg!(target_os = "macos") || !self.pass_detail {
                return None;
            }
        } else if !cfg!(target_os = "macos") && !ui_pass {
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
