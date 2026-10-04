//! Ice and water share vanilla terrain-blend layer 3, including its face order.
//!
//! Separate GPU encodings remain,
//! but one phase item emits their combined order with existing pipelines.
use crate::chunk::*;
use bevy::render::render_resource::CachedRenderPipelineId;

mod command;
mod plan;
#[cfg(test)]
mod tests;

pub(in crate::chunk) use command::DrawMixedTerrainCommands;
use plan::{MixedTerrainSegment, merge_faces};

// A plan cannot have more segments than its admitted face references. The old
// independent 4096 cap was reached by ordinary ice/water views and incorrectly
// switched them to separate, non-interleaved draws. Keep the existing bounded
// reference-work admission rather than a smaller, order-corrupting draw cap.
const MAX_MIXED_TERRAIN_SEGMENTS_PER_FRAME: usize = DEFAULT_TRANSPARENT_UPLOAD_REFS_PER_FRAME;
const DIAGNOSTIC_INTERVAL: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, PartialEq, Eq)]
struct MixedPlanIdentity {
    asset_identity: ChunkTextureAssetIdentity,
    tint_identity: ChunkBiomeTintIdentity,
    model: TransparentModelAllocationIdentity,
    model_revision: u64,
    water_generation: ViewSortGeneration,
    water_range: Range<u32>,
    camera_position_bits: [u32; 3],
}

struct CachedMixedPlan {
    identity: MixedPlanIdentity,
    segments: Arc<[MixedTerrainSegment]>,
}

struct MixedTerrainDraw {
    identity: MixedPlanIdentity,
    view_entity: Entity,
    water_slot: u8,
    water_pipeline: CachedRenderPipelineId,
    model_pipeline: CachedRenderPipelineId,
    segments: Arc<[MixedTerrainSegment]>,
}

#[derive(Default)]
struct MixedFrameStats {
    chunks: usize,
    refs: usize,
    segments: usize,
    address_fallbacks: usize,
    reference_budget_fallbacks: usize,
    segment_budget_fallbacks: usize,
}

#[derive(Resource, Default)]
pub(in crate::chunk) struct MixedTerrainRuntime {
    frame: Vec<MixedTerrainDraw>,
    cache: HashMap<SubChunkKey, CachedMixedPlan>,
    planned_refs: usize,
    stats: MixedFrameStats,
    last_diagnostic: Option<Instant>,
}

impl MixedTerrainRuntime {
    pub(in crate::chunk) fn begin_frame(&mut self) {
        self.frame.clear();
        self.planned_refs = 0;
        self.stats = MixedFrameStats::default();
    }

    pub(in crate::chunk) fn finish_frame(&mut self) {
        let retained = self
            .frame
            .iter()
            .map(|draw| draw.identity.model.key)
            .collect::<HashSet<_>>();
        self.cache.retain(|key, _| retained.contains(key));
        let now = Instant::now();
        if self.stats.chunks
            + self.stats.address_fallbacks
            + self.stats.reference_budget_fallbacks
            + self.stats.segment_budget_fallbacks
            == 0
            || self
                .last_diagnostic
                .is_some_and(|last| now.duration_since(last) < DIAGNOSTIC_INTERVAL)
        {
            return;
        }
        self.last_diagnostic = Some(now);
        bevy::log::info!(
            chunks = self.stats.chunks,
            refs = self.stats.refs,
            segments = self.stats.segments,
            address_fallbacks = self.stats.address_fallbacks,
            reference_budget_fallbacks = self.stats.reference_budget_fallbacks,
            segment_budget_fallbacks = self.stats.segment_budget_fallbacks,
            "native mixed transparent terrain order"
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::chunk) fn plan(
        &mut self,
        view_entity: Entity,
        camera: Vec3,
        entity: Entity,
        instance: &ChunkRenderInstance,
        allocation: &GpuChunkAllocation,
        models: &TransparentModelSortRuntime,
        assets: &ChunkTextureAssets,
        snapshot: &TransparentOrderedSnapshot,
        group: &TransparentLiquidPhaseGroup,
        water_pipeline: CachedRenderPipelineId,
        model_pipeline: CachedRenderPipelineId,
    ) -> Option<u32> {
        let budget_fallbacks =
            self.stats.reference_budget_fallbacks + self.stats.segment_budget_fallbacks;
        let result = self.plan_validated(
            view_entity,
            camera,
            entity,
            instance,
            allocation,
            models,
            assets,
            snapshot,
            group,
            water_pipeline,
            model_pipeline,
        );
        if result.is_none()
            && budget_fallbacks
                == self.stats.reference_budget_fallbacks + self.stats.segment_budget_fallbacks
        {
            self.stats.address_fallbacks += 1;
        }
        result
    }

    #[allow(clippy::too_many_arguments)]
    fn plan_validated(
        &mut self,
        view_entity: Entity,
        camera: Vec3,
        entity: Entity,
        instance: &ChunkRenderInstance,
        allocation: &GpuChunkAllocation,
        models: &TransparentModelSortRuntime,
        assets: &ChunkTextureAssets,
        snapshot: &TransparentOrderedSnapshot,
        group: &TransparentLiquidPhaseGroup,
        water_pipeline: CachedRenderPipelineId,
        model_pipeline: CachedRenderPipelineId,
    ) -> Option<u32> {
        let water = snapshot
            .key
            .visible_allocations
            .iter()
            .find(|water| water.key == group.key)?;
        if water.mesh_generation != allocation.generation
            || water.metadata_index != allocation.metadata_index
            || !transparent_model_allocation_matches(instance, allocation)
            || snapshot.key.asset_identity != assets.identity()
            || allocation.liquid_range.as_ref() != Some(&water.liquid_range)
        {
            return None;
        }
        let model = TransparentModelAllocationIdentity {
            entity,
            key: allocation.key,
            generation: allocation.generation,
            model_range: allocation.model_range.clone()?,
            draw_range: allocation.transparent_model_draw_range.clone()?,
        };
        let order = models.draw_orders.get(&model)?;
        let identity = MixedPlanIdentity {
            asset_identity: assets.identity(),
            tint_identity: snapshot.key.tint_identity,
            model,
            model_revision: order.revision,
            water_generation: snapshot.generation(),
            water_range: group.ref_range.clone(),
            camera_position_bits: super::model::camera_position_bits(camera)?,
        };
        let refs = order
            .words
            .len()
            .checked_add(group.ref_range.end.checked_sub(group.ref_range.start)? as usize)?;
        let segments = if let Some(cache) = self
            .cache
            .get(&group.key)
            .filter(|cache| cache.identity == identity)
        {
            Arc::clone(&cache.segments)
        } else {
            if self.planned_refs.saturating_add(refs) > DEFAULT_TRANSPARENT_UPLOAD_REFS_PER_FRAME {
                self.stats.reference_budget_fallbacks += 1;
                return None;
            }
            self.planned_refs += refs;
            let faces =
                plan::collect_faces(instance, allocation, &order.words, assets, snapshot, group)?;
            let Some(segments) = merge_faces(
                group.key,
                camera,
                faces,
                MAX_MIXED_TERRAIN_SEGMENTS_PER_FRAME.saturating_sub(self.stats.segments),
            ) else {
                self.stats.segment_budget_fallbacks += 1;
                return None;
            };
            let segments = Arc::from(segments);
            self.cache.insert(
                group.key,
                CachedMixedPlan {
                    identity: identity.clone(),
                    segments: Arc::clone(&segments),
                },
            );
            segments
        };
        if self.stats.segments.saturating_add(segments.len()) > MAX_MIXED_TERRAIN_SEGMENTS_PER_FRAME
        {
            self.stats.segment_budget_fallbacks += 1;
            return None;
        }
        let index = u32::try_from(self.frame.len()).ok()?;
        self.stats.chunks += 1;
        self.stats.refs += refs;
        self.stats.segments += segments.len();
        self.frame.push(MixedTerrainDraw {
            identity,
            view_entity,
            water_slot: snapshot.buffer_slot(),
            water_pipeline,
            model_pipeline,
            segments,
        });
        Some(index)
    }
}
