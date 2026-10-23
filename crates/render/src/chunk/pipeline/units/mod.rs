//! CPU-selected direct terrain batches. Transparency and Enhanced keep their existing paths.

mod buffers;
pub(in crate::chunk) mod commands;
mod descriptors;
mod prepare;
pub(in crate::chunk) mod sources;
#[cfg(test)]
mod tests;

use render_model::terrain_units::{
    IndexedUnitPage, QUADS_PER_UNIT, UNITS_PER_INSTANCE, VisibleTerrainUnit,
};

use crate::chunk::*;
use buffers::UnitPage;
use sources::UnitSources;

use commands::DrawTerrainUnits;

/// Ordinary indexed unit draws require one storage slot instead of the vertex-offset buffer.
pub(in crate::chunk) fn supported(mode: ChunkDrawMode, limits: &wgpu::Limits) -> bool {
    mode == ChunkDrawMode::Direct
        && limits.max_bind_groups >= 3
        && limits.max_storage_buffers_per_shader_stage >= required_vertex_storage_buffers()
        && IndexedUnitPage::capacity_for(limits.max_storage_buffer_binding_size) > 0
        && limits.max_buffer_size >= index_count() as u64 * size_of::<u32>() as u64
}

/// The shared winding repeated across the consecutive units of one instance.
pub(in crate::chunk) fn indices() -> Vec<u32> {
    (0..QUADS_PER_UNIT * UNITS_PER_INSTANCE)
        .flat_map(|quad| STATIC_QUAD_INDICES.map(|index| quad * 4 + index))
        .collect()
}

/// Number of indices submitted for each grouped instance, including rejected tails.
pub(in crate::chunk) const fn index_count() -> u32 {
    QUADS_PER_UNIT * UNITS_PER_INSTANCE * STATIC_QUAD_INDICES.len() as u32
}

/// Work queued or prepared in this frame, summed across views and opaque streams.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(in crate::chunk) struct UnitBatchStats {
    pub(in crate::chunk) phase_items: u64,
    pub(in crate::chunk) draws: u64,
    pub(in crate::chunk) units: u64,
    /// Units repeated by the optional current-frame depth prepass.
    pub(in crate::chunk) occluder_units: u64,
    pub(in crate::chunk) padded_quads: u64,
    pub(in crate::chunk) list_bytes_uploaded: u64,
    pub(in crate::chunk) descriptor_bytes_uploaded: u64,
    pub(in crate::chunk) reference_bytes_uploaded: u64,
}

#[derive(Default)]
struct UnitStream {
    selected: Vec<Entity>,
    scratch: Vec<Entity>,
    descriptors: descriptors::Descriptors,
    indexed: bool,
    references: Vec<u32>,
    fallback: Vec<VisibleTerrainUnit>,
    pages: Vec<UnitPage>,
    padded_quads: u64,
}

#[cfg(test)]
impl UnitStream {
    /// Resolves the exact ordered descriptors read by the current shader submission.
    fn visible_units(&self) -> impl Iterator<Item = &VisibleTerrainUnit> {
        self.references
            .iter()
            .map(|&slot| &self.descriptors.records[slot as usize])
            .chain(self.fallback.iter())
    }
}

struct UnitView {
    sources: Arc<UnitSources>,
    stale: bool,
    queued: bool,
    camera: Option<[f64; 3]>,
    facing: Option<[(f64, bool); 3]>,
    streams: [UnitStream; 4],
    occluders: UnitStream,
}

/// Retained unit lists and their GPU pages; views enter only through the ordinary direct path.
#[derive(Resource, Default)]
pub(in crate::chunk) struct TerrainUnitBatches {
    views: HashMap<Entity, UnitView>,
    index_buffer: Option<Buffer>,
    /// Allows adapter tests to compare the same direct queue with and without batching.
    pub(in crate::chunk) disabled: bool,
    pub(in crate::chunk) stats: UnitBatchStats,
}

impl TerrainUnitBatches {
    /// Rewrites retained pages from CPU descriptors after a failed terrain publication.
    pub(in crate::chunk) fn invalidate_publication(&mut self) {
        for view in self.views.values_mut() {
            view.stale = true;
            for page in view
                .streams
                .iter_mut()
                .chain(std::iter::once(&mut view.occluders))
                .flat_map(|stream| &mut stream.pages)
            {
                page.invalidate();
            }
        }
    }

    /// Rewrites selected flags from this frame's bits, or clears all retained flags on fallback.
    pub(in crate::chunk) fn encode_masks(
        &self,
        entity: Entity,
        kernel: &gpu_cull::unit_mask::UnitMasks,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        bits: Option<&wgpu::Buffer>,
    ) {
        if let Some(view) = self.views.get(&entity) {
            for page in view.streams.iter().flat_map(|s| &s.pages) {
                if let Some(bindings) = &page.masks {
                    kernel.encode(device, encoder, bindings, bits.is_some(), page.count);
                }
            }
        }
    }

    /// Starts queueing, forgetting views that were absent from the preceding queue.
    pub(in crate::chunk) fn begin_frame(&mut self) {
        self.views.retain(|_, view| view.queued);
        for view in self.views.values_mut() {
            view.queued = false;
        }
        self.stats = default();
    }

    /// Keeps descriptors while their retained source identity holds, and updates the facing eye.
    pub(in crate::chunk) fn queue(
        &mut self,
        entity: Entity,
        sources: Arc<UnitSources>,
        camera: Option<[f64; 3]>,
    ) {
        let view = self.views.entry(entity).or_insert_with(|| UnitView {
            sources: sources.clone(),
            stale: true,
            queued: false,
            camera,
            facing: None,
            streams: default(),
            occluders: default(),
        });
        view.stale |= !Arc::ptr_eq(&view.sources, &sources);
        view.sources = sources;
        view.camera = camera;
        view.queued = true;
    }

    /// Whether this frame's solid-only pass uses a single unit command for this view.
    pub(in crate::chunk) fn contains(&self, entity: Entity) -> bool {
        self.views.get(&entity).is_some_and(|view| view.queued)
    }
}

/// Adds the unit resource, commands and preparation after current direct-occlusion results.
pub(in crate::chunk) fn install(render_app: &mut SubApp) {
    render_app
        .init_resource::<TerrainUnitBatches>()
        .add_systems(RenderStartup, configure_support)
        .add_render_command::<Opaque3d, DrawTerrainUnits<0>>()
        .add_render_command::<Opaque3d, commands::DrawTerrainOccluders>()
        .add_render_command::<Opaque3d, DrawTerrainUnits<1>>()
        .add_render_command::<Opaque3d, DrawTerrainUnits<2>>()
        .add_render_command::<Opaque3d, DrawTerrainUnits<3>>()
        .add_systems(
            Render,
            prepare::prepare_units
                .run_if(render_scene::panorama::world_passes_enabled)
                .in_set(RenderSystems::PrepareResources)
                .after(gpu_cull::direct::prepare_direct_occlusion),
        );
}

/// Admits unit pipelines before warmup, so other backends never compile unused unit shaders.
fn configure_support(
    adapter: Res<RenderAdapter>,
    device: Res<RenderDevice>,
    mut pipeline: ResMut<ChunkPipeline>,
) {
    let info = adapter.get_info();
    let mode = select_chunk_draw_mode(
        adapter.get_downlevel_capabilities().flags,
        device.features(),
        Backends::from(info.backend),
        apple_gpu(&info),
    );
    pipeline.unit_supported = supported(mode, &device.limits());
}

/// A single additional vertex storage binding, with no vertex buffer or indirect arguments.
pub(in crate::chunk) fn bind_group_layout() -> BindGroupLayoutDescriptor {
    BindGroupLayoutDescriptor::new(
        "visible terrain units",
        &[BindGroupLayoutEntry {
            binding: 0,
            visibility: ShaderStages::VERTEX,
            ty: BindingType::Buffer {
                ty: BufferBindingType::Storage { read_only: true },
                has_dynamic_offset: false,
                min_binding_size: std::num::NonZeroU64::new(size_of::<VisibleTerrainUnit>() as u64),
            },
            count: None,
        }],
    )
}
