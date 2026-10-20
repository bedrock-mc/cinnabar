//! Read access to the packed chunk renderer's GPU arenas and resident sections for the
//! shader-pack geometry adapter, which re-expands them into Iris vertex streams.

use bevy::ecs::schedule::ScheduleConfigs;
use bevy::ecs::system::{ScheduleSystem, SystemParam};

use crate::chunk::gpu_cull::ChunkHiddenEntities;
use crate::chunk::*;
use crate::shaderpack::geometry::jobs::SectionGeometry;
use render_scene::ChunkRenderInstance;

/// The packed-chunk buffers the expansion kernels read, by their chunk-layout binding.
pub(crate) struct PackedChunkBuffers<'a> {
    /// Binding 1: packed cube quads.
    pub(crate) quads: &'a Buffer,
    /// Binding 2: section origins and stream bases.
    pub(crate) origins: &'a Buffer,
    /// Binding 3: the material table.
    pub(crate) materials: &'a Buffer,
    /// Binding 7: per-section biome records.
    pub(crate) biome_records: &'a Buffer,
    /// Binding 8: biome tint colours.
    pub(crate) biome_tints: &'a Buffer,
    /// Binding 12: block-model templates.
    pub(crate) model_templates: &'a Buffer,
    /// Binding 11: the world animation clock.
    pub(crate) animation_clock: &'a Buffer,
    /// Binding 13: the shared lighting, model and liquid word stream.
    pub(crate) geometry_streams: &'a Buffer,
    /// `BIOME_QUERY_TABLES_BINDING`: the biome blend kernel's constant lookup tables.
    pub(crate) biome_query_tables: &'a Buffer,
}

/// Arena and table resources of the packed chunk renderer, when it has prepared them.
#[derive(SystemParam)]
pub(crate) struct PackedChunkSources<'w> {
    arena: Option<Res<'w, ChunkGpuArena>>,
    textures: Option<Res<'w, ChunkGpuTextureAssets>>,
    tints: Option<Res<'w, ChunkGpuBiomeTints>>,
    clock: Option<Res<'w, ChunkGpuAnimationClock>>,
    hidden: Option<Res<'w, ChunkHiddenEntities>>,
}

impl PackedChunkSources<'_> {
    /// Every buffer the expansion binds, or `None` until the chunk renderer has them all.
    pub(crate) fn buffers(&self) -> Option<PackedChunkBuffers<'_>> {
        let arena = self.arena.as_deref()?;
        let textures = self.textures.as_deref()?.prepared.as_ref()?;
        let tints = self.tints.as_deref()?.prepared.as_ref()?;
        Some(PackedChunkBuffers {
            quads: &arena.quad_buffer,
            origins: &arena.origin_buffer,
            materials: &textures.material_buffer,
            biome_records: &arena.biome_buffer,
            biome_tints: &tints.buffer,
            model_templates: &textures.model_template_buffer,
            animation_clock: &self.clock.as_deref()?.buffer,
            geometry_streams: &arena.geometry_stream_buffer,
            biome_query_tables: &tints.query_tables,
        })
    }

    /// Whether the cave culler hides section `entity`.
    pub(crate) fn is_hidden(&self, entity: Entity) -> bool {
        self.hidden
            .as_deref()
            .is_some_and(|hidden| hidden.contains(entity))
    }
}

/// Resident packed sections and their changes this frame.
#[derive(SystemParam)]
pub(crate) struct ResidentSections<'w, 's> {
    sections: Query<
        'w,
        's,
        (
            Entity,
            &'static GpuChunkAllocation,
            &'static ChunkRenderInstance,
        ),
    >,
    changed: Query<'w, 's, Entity, Changed<GpuChunkAllocation>>,
    removed: RemovedComponents<'w, 's, GpuChunkAllocation>,
}

/// One resident section: its packed geometry and its origin in blocks.
pub(crate) struct ResidentSection<'a> {
    pub(crate) geometry: SectionGeometry<'a>,
    pub(crate) origin: [i32; 3],
}

impl ResidentSections<'_, '_> {
    /// Sections whose allocation was replaced or added this frame.
    pub(crate) fn changed(&self) -> impl Iterator<Item = Entity> + '_ {
        self.changed.iter()
    }

    /// Sections whose allocation was removed since the last read.
    pub(crate) fn take_removed(&mut self) -> Vec<Entity> {
        self.removed.read().collect()
    }

    /// Every resident section.
    pub(crate) fn entities(&self) -> impl Iterator<Item = Entity> + '_ {
        self.sections.iter().map(|(entity, ..)| entity)
    }

    /// The packed geometry `entity`'s current allocation was uploaded from, or `None` when the
    /// section is gone or its extracted streams are newer than its allocation.
    pub(crate) fn get(&self, entity: Entity) -> Option<ResidentSection<'_>> {
        let (_, allocation, instance) = self.sections.get(entity).ok()?;
        if instance.key != allocation.key || instance.generation != allocation.generation {
            return None;
        }
        let record = |range: &Option<Range<u32>>, words: u32| {
            range.as_ref().map_or(0, |range| range.start / words)
        };
        Some(ResidentSection {
            geometry: SectionGeometry {
                metadata_index: allocation.metadata_index,
                quad_base: allocation.quad_range.start,
                cube_quads: &instance.cube_quads,
                solid_len: allocation.cube_layout.solid_len(),
                model_refs: &instance.model_refs,
                model_draw_base: record(&allocation.model_draw_range, 2),
                model_draw_refs: &instance.model_draw_refs,
                transparent_model_draw_base: record(&allocation.transparent_model_draw_range, 2),
                transparent_model_draw_refs: &instance.transparent_model_draw_refs,
                liquid_base: record(&allocation.liquid_range, 4),
                liquid_quads: &instance.liquid_quads,
            },
            origin: instance.origin,
        })
    }
}

/// Orders `systems` after the packed chunk upload that publishes this frame's allocations.
pub(crate) fn after_packed_chunk_upload<M>(
    systems: impl IntoScheduleConfigs<ScheduleSystem, M>,
) -> ScheduleConfigs<ScheduleSystem> {
    systems.after(prepare_gpu_chunks)
}

/// Publishes a replacement decision before opaque and translucent terrain consume it.
pub(crate) fn before_native_terrain_queues<M>(
    systems: impl IntoScheduleConfigs<ScheduleSystem, M>,
) -> ScheduleConfigs<ScheduleSystem> {
    systems
        .after(bevy::render::RenderSystems::PrepareViews)
        .before(prepare_transparent_sorts)
        .before(prepare_transparent_model_sorts)
        .before(bevy::render::RenderSystems::Queue)
}

/// Whether terrain may draw through multi-draw-indirect on this device, by the packed chunk
/// renderer's own policy; `None` when terrain cannot draw at all.
pub(crate) fn terrain_multi_draw_indirect(
    adapter: &bevy::render::renderer::RenderAdapter,
    device: &RenderDevice,
) -> Option<bool> {
    match select_chunk_draw_mode(
        adapter.get_downlevel_capabilities().flags,
        device.features(),
        Backends::from(adapter.get_info().backend),
        apple_gpu(&adapter.get_info()),
    ) {
        ChunkDrawMode::Direct => Some(false),
        ChunkDrawMode::MultiDrawIndirect => Some(true),
        ChunkDrawMode::Unsupported => None,
    }
}
