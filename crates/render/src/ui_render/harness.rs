//! Test-only model of retained UI publication and shared GPU arena identities.

use super::resources::{arena_capacity, retained_gpu_bytes};
use super::*;
use render_model::{UiRenderReject, UiScissor};

#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UiPreparedFrame {
    pub revision: u64,
    pub pipeline_id: u64,
    pub bind_group_family_id: u64,
    pub vertex_arena_id: u64,
    pub index_arena_id: u64,
    pub per_node_gpu_allocations: u32,
    draw_order: Arc<[usize]>,
    scissors: Arc<[UiScissor]>,
}

#[allow(dead_code)]
impl UiPreparedFrame {
    #[must_use]
    /// Returns the retained batch order modeled by this preparation.
    pub fn draw_order(&self) -> &[usize] {
        &self.draw_order
    }

    #[must_use]
    /// Returns the scissor rectangles in retained batch order.
    pub fn scissors(&self) -> &[UiScissor] {
        &self.scissors
    }
}

#[allow(dead_code)]
pub struct UiRenderHarness {
    scene: UiRenderScene,
    stats: UiRenderStats,
    vertex_capacity: usize,
    index_capacity: usize,
    vertex_arena_id: u64,
    index_arena_id: u64,
    prepared: Option<UiPreparedFrame>,
}

#[allow(dead_code)]
impl UiRenderHarness {
    #[must_use]
    /// Starts an empty fixture with reusable shared arena identities.
    pub fn new() -> Self {
        Self {
            scene: UiRenderScene::default(),
            stats: UiRenderStats::default(),
            vertex_capacity: 0,
            index_capacity: 0,
            vertex_arena_id: 0,
            index_arena_id: 0,
            prepared: None,
        }
    }

    /// Admits a draw list through the renderer publication validator.
    pub fn publish(&mut self, input: UiRenderInput) -> Result<(), UiRenderReject> {
        self.scene.publish(input, &self.stats)
    }

    /// Models shared arena growth and retains unchanged preparations.
    pub fn prepare(&mut self) -> Result<UiPreparedFrame, UiRenderReject> {
        let Some(input) = self.scene.input.as_ref() else {
            return Err(UiRenderReject {
                revision: self.scene.revision,
                reason: UiRenderRejectReason::NoPublishedScene,
            });
        };
        if let Some(prepared) = &self.prepared
            && prepared.revision == input.revision
        {
            return Ok(prepared.clone());
        }
        if self.vertex_capacity < input.vertices.len() {
            self.vertex_capacity = arena_capacity(input.vertices.len(), MAX_UI_VERTICES);
            self.vertex_arena_id = self.vertex_arena_id.saturating_add(1);
        }
        if self.index_capacity < input.indices.len() {
            self.index_capacity = arena_capacity(input.indices.len(), MAX_UI_INDICES);
            self.index_arena_id = self.index_arena_id.saturating_add(1);
        }
        self.stats.update(|stats| {
            stats.accepted_revision = Some(input.revision);
            stats.uploaded_vertices = input.vertices.len() as u32;
            stats.uploaded_indices = input.indices.len() as u32;
            stats.draw_calls = input.batches.len() as u32;
            stats.vertex_arena_capacity = self.vertex_capacity as u32;
            stats.index_arena_capacity = self.index_capacity as u32;
            stats.per_node_gpu_allocations = 0;
            stats.retained_gpu_bytes = retained_gpu_bytes(
                self.vertex_capacity,
                self.index_capacity,
                input.textures.plan().bytes(),
            );
        });
        let prepared = UiPreparedFrame {
            revision: input.revision,
            pipeline_id: 1,
            bind_group_family_id: 1,
            vertex_arena_id: self.vertex_arena_id,
            index_arena_id: self.index_arena_id,
            per_node_gpu_allocations: 0,
            draw_order: (0..input.batches.len()).collect::<Vec<_>>().into(),
            scissors: input
                .batches
                .iter()
                .map(|batch| batch.scissor)
                .collect::<Vec<_>>()
                .into(),
        };
        self.prepared = Some(prepared.clone());
        Ok(prepared)
    }

    #[must_use]
    /// Borrows the current fixture publication.
    pub const fn scene(&self) -> &UiRenderScene {
        &self.scene
    }

    #[must_use]
    /// Copies the fixture counters without altering its publication.
    pub fn stats(&self) -> render_model::UiRenderStatsSnapshot {
        self.stats.snapshot()
    }
}

impl Default for UiRenderHarness {
    fn default() -> Self {
        Self::new()
    }
}
