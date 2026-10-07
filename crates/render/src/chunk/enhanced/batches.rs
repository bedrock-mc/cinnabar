//! Validated Enhanced terrain submissions, retained until their input changes.

use super::*;
use crate::enhanced::EnhancedViews;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum SubmissionKind {
    Camera,
    Shadow(usize),
    LocalLight(usize),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Region {
    Camera(Mat4),
    Shadow(CascadeBounds),
}

impl Region {
    fn volume(self) -> Volume {
        match self {
            Self::Camera(clip) => Volume::Camera(Frustum::from_clip_from_world(&clip)),
            Self::Shadow(bounds) => Volume::Shadow(bounds),
        }
    }
}

enum Volume {
    Camera(Frustum),
    Shadow(CascadeBounds),
}

impl Volume {
    fn intersects(&self, entry: &ResidentDraw) -> bool {
        match self {
            Self::Camera(frustum) => frustum.intersects_obb_identity(&Aabb {
                center: entry.center.into(),
                half_extents: entry.half_extent.into(),
            }),
            Self::Shadow(bounds) => bounds.intersects_aabb(entry.center, entry.half_extent),
        }
    }
}

struct ResidentDraw {
    entity: Entity,
    allocation: GpuChunkAllocation,
    eligible: bool,
    center: Vec3,
    half_extent: Vec3,
    cube: Option<DrawIndexedIndirectArgs>,
    model: Option<DrawIndexedIndirectArgs>,
}

#[derive(Default)]
struct ResidentScene {
    revision: u64,
    entries: Vec<ResidentDraw>,
}

impl ResidentScene {
    fn refresh(
        &mut self,
        arena: &ChunkGpuArena,
        eligible: impl Fn(Entity, &GpuChunkAllocation) -> bool,
    ) -> bool {
        if self.entries.len() == arena.allocations.len()
            && self.entries.iter().all(|entry| {
                arena
                    .allocations
                    .get(&entry.entity)
                    .is_some_and(|resident| {
                        allocation_matches(&entry.allocation, &resident.gpu)
                            && entry.eligible == eligible(entry.entity, &resident.gpu)
                    })
            })
        {
            return false;
        }
        self.entries.clear();
        self.entries
            .extend(arena.allocations.iter().map(|(&entity, resident)| {
                let allocation = &resident.gpu;
                let (center, half_extent) = chunk_bounds(allocation);
                ResidentDraw {
                    entity,
                    allocation: allocation.clone(),
                    eligible: eligible(entity, allocation),
                    center,
                    half_extent,
                    cube: cube_coverage_command(allocation),
                    model: model_direct_draw_command(allocation),
                }
            }));
        self.revision = self.revision.wrapping_add(1);
        true
    }
}

// Depth and shadows need both solid and cutout quads, including faces outside the camera view.
fn cube_coverage_command(allocation: &GpuChunkAllocation) -> Option<DrawIndexedIndirectArgs> {
    let (quads, _, base_vertex) = cube_draw_base(allocation)?;
    Some(DrawIndexedIndirectArgs {
        index_count: STATIC_QUAD_INDICES.len() as u32,
        instance_count: quads.end - quads.start,
        first_index: 0,
        base_vertex,
        first_instance: quads.start,
    })
}

fn allocation_matches(left: &GpuChunkAllocation, right: &GpuChunkAllocation) -> bool {
    left.key == right.key
        && left.generation == right.generation
        && left.tint_identity == right.tint_identity
        && left.metadata_index == right.metadata_index
        && direct_stream_addresses(left) == direct_stream_addresses(right)
        && left.has_depth_liquid == right.has_depth_liquid
        && left.has_transparent_liquid == right.has_transparent_liquid
        && left.depth_liquid_range == right.depth_liquid_range
}

#[derive(Default)]
pub(super) struct GeometryBatch {
    region: Option<Region>,
    scene_revision: u64,
    selected: Vec<usize>,
    pub(super) commands: Vec<DrawIndexedIndirectArgs>,
    pub(super) cube_count: usize,
    pub(super) indirect: Option<Buffer>,
    capacity: usize,
    uploaded: Vec<DrawIndexedIndirectArgs>,
}

impl GeometryBatch {
    fn refresh(&mut self, scene: &ResidentScene, region: Region) -> bool {
        if self.region == Some(region) && self.scene_revision == scene.revision {
            return false;
        }
        let volume = region.volume();
        self.selected.clear();
        self.selected.extend(
            scene
                .entries
                .iter()
                .enumerate()
                .filter_map(|(index, entry)| {
                    (entry.eligible && volume.intersects(entry)).then_some(index)
                }),
        );
        self.commands.clear();
        self.commands.extend(
            self.selected
                .iter()
                .filter_map(|&index| scene.entries[index].cube),
        );
        self.cube_count = self.commands.len();
        self.commands.extend(
            self.selected
                .iter()
                .filter_map(|&index| scene.entries[index].model),
        );
        self.region = Some(region);
        self.scene_revision = scene.revision;
        true
    }

    fn upload_needed(&self) -> bool {
        !self.commands.is_empty()
            && (self.indirect.is_none()
                || bytemuck::cast_slice::<_, u8>(&self.commands)
                    != bytemuck::cast_slice::<_, u8>(&self.uploaded))
    }

    fn upload(&mut self, device: &RenderDevice, queue: &RenderQueue) {
        if !self.upload_needed() {
            return;
        }
        if self.capacity < self.commands.len() {
            self.capacity = self.commands.len().next_power_of_two();
            self.indirect = Some(device.create_buffer(&BufferDescriptor {
                label: Some("cached Enhanced terrain indirect commands"),
                size: self.capacity as u64 * INDEXED_INDIRECT_BYTES,
                usage: BufferUsages::INDIRECT | BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
        }
        queue.write_buffer(
            self.indirect.as_ref().unwrap(),
            0,
            bytemuck::cast_slice(&self.commands),
        );
        self.uploaded.clear();
        self.uploaded.extend_from_slice(&self.commands);
    }
}

#[derive(Resource, Default)]
pub(crate) struct EnhancedGeometryCache {
    scene: ResidentScene,
    batches: HashMap<(Entity, SubmissionKind), GeometryBatch>,
    pub(super) indirect_enabled: bool,
}

impl EnhancedGeometryCache {
    pub(super) fn batch(
        &self,
        view: Entity,
        kind: SubmissionKind,
        region: Region,
    ) -> Option<&GeometryBatch> {
        self.batches
            .get(&(view, kind))
            .filter(|batch| batch.region == Some(region))
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn prepare_enhanced_geometry(
    mut cache: ResMut<EnhancedGeometryCache>,
    arena: Option<Res<ChunkGpuArena>>,
    tints: Res<ChunkBiomeTints>,
    probe: Res<ActiveFrameProbe>,
    views: Res<EnhancedViews>,
    adapter: Res<RenderAdapter>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
) {
    if !views.0.values().any(|view| {
        !view.settings.reflection_capture
            && (view.scene.is_some() || view.settings.shadows && view.shadow.is_some())
    }) {
        if !cache.batches.is_empty() {
            cache.batches.clear();
        }
        if !cache.scene.entries.is_empty() {
            cache.scene.entries.clear();
        }
        return;
    }
    let Some(arena) = arena else {
        return;
    };
    let mode = select_chunk_draw_mode(
        adapter.get_downlevel_capabilities().flags,
        device.features(),
        Backends::from(adapter.get_info().backend),
        cfg!(debug_assertions),
    );
    if matches!(mode, ChunkDrawMode::Unsupported) {
        if !cache.batches.is_empty() {
            cache.batches.clear();
        }
        return;
    }
    let probe = probe.scope();
    let tint_identity = tints.table_identity();
    cache.scene.refresh(&arena, |entity, allocation| {
        !arena.pending_removals.contains(&entity)
            && chunk_tint_identity_is_active(allocation.tint_identity, tint_identity)
            && probe.accepts(
                entity,
                FrameAllocationIdentity {
                    entity,
                    key: allocation.key,
                    generation: allocation.generation,
                },
            )
    });
    cache.indirect_enabled = matches!(mode, ChunkDrawMode::MultiDrawIndirect);
    let indirect = cache.indirect_enabled;
    let EnhancedGeometryCache { scene, batches, .. } = &mut *cache;
    batches.retain(|(entity, kind), _| {
        views.0.get(entity).is_some_and(|view| {
            !view.settings.reflection_capture
                && match kind {
                    SubmissionKind::Camera => view.scene.is_some(),
                    SubmissionKind::Shadow(index) => {
                        view.settings.shadows
                            && view.shadow.is_some()
                            && *index < view.cascades.len()
                    }
                    SubmissionKind::LocalLight(index) => {
                        view.settings.shadows
                            && view.shadow.is_some()
                            && *index < view.local_lights.shadows.len()
                    }
                }
        })
    });
    for (&entity, view) in &views.0 {
        if view.settings.reflection_capture {
            continue;
        }
        if view.scene.is_some() {
            let batch = batches.entry((entity, SubmissionKind::Camera)).or_default();
            batch.refresh(scene, Region::Camera(view.camera_clip));
            if indirect {
                batch.upload(&device, &queue);
            }
        }
        if view.settings.shadows && view.shadow.is_some() {
            for (index, &bounds) in view.cascades.iter().enumerate() {
                let batch = batches
                    .entry((entity, SubmissionKind::Shadow(index)))
                    .or_default();
                batch.refresh(scene, Region::Shadow(bounds));
                if indirect {
                    batch.upload(&device, &queue);
                }
            }
            for (index, light) in view.local_lights.shadows.iter().enumerate() {
                let batch = batches
                    .entry((entity, SubmissionKind::LocalLight(index)))
                    .or_default();
                batch.refresh(scene, Region::Camera(light.clip));
                if indirect {
                    batch.upload(&device, &queue);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
