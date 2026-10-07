use super::*;

/// Attaches Metal queries to existing GPU work without adding marker passes or submissions.
pub(crate) fn render_pass_timestamps(
    world: &World,
    stage: RuntimeStage,
) -> Option<wgpu::RenderPassTimestampWrites<'_>> {
    world.get_resource::<GpuTimestamps>()?.render_pass(stage)
}

impl GpuTimestamps {
    /// Uses the fixed pass-query pool on Metal; other backends retain their graph markers.
    pub(super) fn render_pass(
        &self,
        stage: RuntimeStage,
    ) -> Option<wgpu::RenderPassTimestampWrites<'_>> {
        if !cfg!(target_os = "macos") {
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
