//! Bounded local light selection, tiled admission and cached point-shadow coverage.

use bevy::{
    prelude::*,
    render::{
        render_resource::*,
        renderer::{RenderDevice, RenderQueue},
    },
};
use bytemuck::{Pod, Zeroable};
use std::{
    collections::{HashMap, hash_map::DefaultHasher},
    hash::{Hash, Hasher},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
#[path = "local_light_selection.rs"]
mod selection;
#[cfg(test)]
#[path = "local_light_tests.rs"]
mod tests;

pub(crate) const MAX_LOCAL_LIGHTS: usize = 32;
pub(crate) const MAX_SHADOWED_LIGHTS: usize = 2;
pub(crate) const POINT_SHADOW_FACES: usize = 6;
pub(crate) const TILE_SIDE: u32 = 32;
const TILE_LIGHTS: usize = 8;
pub(crate) const POINT_SHADOW_RESOLUTION: u32 = 256;
pub(crate) const POINT_SHADOW_ANIMATION_INTERVAL: f32 = 1.0 / 15.0;
pub(crate) const LIGHT_RADIUS: f32 = 12.0;
const POINT_SHADOW_NEAR: f32 = 0.05;
const SOURCE_RADIUS: f32 = 0.22;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct LightSource {
    pub position: Vec3,
    pub level: u8,
    pub dimension: i32,
}

#[derive(Resource, Default)]
pub(crate) struct LocalLightSources {
    chunks: HashMap<Entity, Vec<LightSource>>,
    casters: HashMap<Entity, CasterGeometry>,
    pub revision: u64,
    lights_revision: u64,
}

struct CasterGeometry {
    key: world::SubChunkKey,
    cubes: Arc<[meshing::PackedQuad]>,
    models: Arc<[meshing::PackedModelRef]>,
}

impl CasterGeometry {
    fn matches(
        &self,
        key: world::SubChunkKey,
        cubes: &Arc<[meshing::PackedQuad]>,
        models: &Arc<[meshing::PackedModelRef]>,
    ) -> bool {
        self.key == key
            && (Arc::ptr_eq(&self.cubes, cubes) || self.cubes.as_ref() == cubes.as_ref())
            && (Arc::ptr_eq(&self.models, models)
                || (self.models.len() == models.len()
                    && self.models.iter().zip(models.iter()).all(|(old, new)| {
                        // Lighting arena addresses do not change a model's shadow coverage.
                        let [old_transform, old_template, _, old_mask] = old.words();
                        let [new_transform, new_template, _, new_mask] = new.words();
                        (old_transform, old_template, old_mask)
                            == (new_transform, new_template, new_mask)
                    })))
    }
}

impl LocalLightSources {
    fn record_changes(&mut self, geometry_dirty: bool, lights_dirty: bool) {
        if geometry_dirty {
            self.revision = self.revision.wrapping_add(1);
        }
        if lights_dirty {
            self.lights_revision = self.lights_revision.wrapping_add(1);
        }
    }
}

pub(crate) fn collect_sources(
    mut cache: ResMut<LocalLightSources>,
    changed: Query<(Entity, &crate::ChunkRenderInstance), Changed<crate::ChunkRenderInstance>>,
    mut removed: RemovedComponents<crate::ChunkRenderInstance>,
) {
    let mut geometry_dirty = false;
    let mut lights_dirty = false;
    for entity in removed.read() {
        geometry_dirty |= cache.casters.remove(&entity).is_some();
        lights_dirty |= cache.chunks.remove(&entity).is_some();
    }
    for (entity, chunk) in &changed {
        let key = chunk.key();
        let (cubes, models, _, _) = chunk.indirect_geometry();
        if cubes.is_empty() && models.is_empty() {
            geometry_dirty |= cache.casters.remove(&entity).is_some();
        } else if let Some(previous) = cache.casters.get_mut(&entity) {
            geometry_dirty |= !previous.matches(key, &cubes, &models);
            *previous = CasterGeometry { key, cubes, models };
        } else {
            cache
                .casters
                .insert(entity, CasterGeometry { key, cubes, models });
            geometry_dirty = true;
        }
        let origin =
            Vec3::new(key.x as f32, key.y as f32, key.z as f32) * world::SUB_CHUNK_SIDE as f32;
        let lights = || {
            chunk
                .light_emitters()
                .iter()
                .filter(|light| light.emission > 0)
                .map(|light| LightSource {
                    position: origin
                        + Vec3::from_array(light.position.map(f32::from))
                        + Vec3::splat(0.5),
                    level: light.emission.min(15),
                    dimension: key.dimension,
                })
        };
        let unchanged = cache.chunks.get(&entity).map_or_else(
            || lights().next().is_none(),
            |previous| previous.iter().copied().eq(lights()),
        );
        if !unchanged {
            let values = cache.chunks.entry(entity).or_default();
            values.clear();
            values.extend(lights());
            if values.is_empty() {
                cache.chunks.remove(&entity);
            }
            lights_dirty = true;
        }
    }
    cache.record_changes(geometry_dirty, lights_dirty);
}

#[repr(C)]
#[derive(Clone, Copy, PartialEq, Pod, Zeroable)]
struct LocalLightGpu {
    position_radius: [f32; 4],
    radiance_shadow: [f32; 4],
    shape: [f32; 4],
    clip: [[[f32; 4]; 4]; POINT_SHADOW_FACES],
}

#[repr(C)]
#[derive(Clone, Copy, PartialEq, Pod, Zeroable)]
struct LocalLightBlock {
    info: [u32; 4],
    lights: [LocalLightGpu; MAX_LOCAL_LIGHTS],
}
impl LocalLightBlock {
    fn illumination_signature(&self) -> u64 {
        let mut signature = self.info[0] as u64;
        for light in self.lights.iter().take(self.info[0] as usize) {
            let mut hash = DefaultHasher::new();
            for value in light
                .position_radius
                .into_iter()
                .chain(light.radiance_shadow[..3].iter().copied())
            {
                value.to_bits().hash(&mut hash);
            }
            signature ^= hash.finish().rotate_left(17);
        }
        signature
    }
}
pub(crate) const LOCAL_LIGHT_BUFFER_BYTES: usize = std::mem::size_of::<LocalLightBlock>();

#[derive(Clone, Copy, PartialEq)]
struct PreparedInputs {
    lights_revision: u64,
    dimension: Option<i32>,
    camera: Vec3,
    clip: Mat4,
    size: [u32; 2],
    first_layer: u32,
}

pub(crate) struct PointShadowView {
    pub clip: Mat4,
    pub position: Vec3,
    pub layer: u32,
}

pub(crate) struct LocalLightView {
    pub buffer: Buffer,
    pub tiles: Buffer,
    tile_capacity: usize,
    pub shadows: Vec<PointShadowView>,
    data: LocalLightBlock,
    uploaded: LocalLightBlock,
    upload_valid: bool,
    uploaded_tiles: Vec<u32>,
    candidates: Vec<LightSource>,
    tile_scratch: Vec<u32>,
    tile_scores: Vec<f32>,
    tile_rays: Vec<Vec3>,
    prepared: Option<PreparedInputs>,
    pub dirty: bool,
    pub submitted: AtomicBool,
    revision: u64,
    pub rebuilds: u64,
    pub uploads: u64,
}

impl LocalLightView {
    /// Selected radiance only; unrelated streaming and shadow readiness do not relight GI.
    pub(crate) fn illumination_signature(&self) -> u64 {
        self.data.illumination_signature()
    }
    pub(crate) fn shadow_identity(&self) -> u64 {
        let mut hash = DefaultHasher::new();
        self.prepared.map(|input| input.dimension).hash(&mut hash);
        for light in self.data.lights.iter().take(MAX_SHADOWED_LIGHTS) {
            for value in light
                .position_radius
                .into_iter()
                .chain([light.radiance_shadow[3]])
            {
                value.to_bits().hash(&mut hash);
            }
        }
        hash.finish()
    }
    pub fn new(device: &RenderDevice) -> Self {
        let buffer = device.create_buffer(&BufferDescriptor {
            label: Some("Enhanced local light sources"),
            size: LOCAL_LIGHT_BUFFER_BYTES as u64,
            usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let tiles = device.create_buffer(&BufferDescriptor {
            label: Some("Enhanced local light tiles"),
            size: 16,
            usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            buffer,
            tiles,
            tile_capacity: 16,
            shadows: Vec::with_capacity(MAX_SHADOWED_LIGHTS * POINT_SHADOW_FACES),
            data: LocalLightBlock::zeroed(),
            uploaded: LocalLightBlock::zeroed(),
            upload_valid: false,
            uploaded_tiles: Vec::new(),
            candidates: Vec::with_capacity(MAX_LOCAL_LIGHTS),
            tile_scratch: Vec::new(),
            tile_scores: Vec::new(),
            tile_rays: Vec::new(),
            prepared: None,
            dirty: true,
            submitted: AtomicBool::new(false),
            revision: u64::MAX,
            rebuilds: 0,
            uploads: 0,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn prepare(
        &mut self,
        device: &RenderDevice,
        queue: &RenderQueue,
        sources: &LocalLightSources,
        dimension: Option<i32>,
        camera: Vec3,
        clip: Mat4,
        size: [u32; 2],
        first_layer: u32,
        actor_dirty: bool,
        shadow_ready: bool,
    ) -> bool {
        let was_submitted = self.submitted.swap(false, Ordering::Relaxed);
        let inputs = PreparedInputs {
            lights_revision: sources.lights_revision,
            dimension,
            camera,
            clip,
            size,
            first_layer,
        };
        let inputs_changed = self.prepared != Some(inputs);
        if inputs_changed {
            self.rebuilds += 1;
            selection::select(&mut self.candidates, sources, &inputs);
            self.data.lights.fill(LocalLightGpu::zeroed());
            self.shadows.clear();
            for (index, source) in self.candidates.iter().enumerate() {
                let strength = (f32::from(source.level) / 15.0).powi(2) * 3.0;
                self.data.lights[index].position_radius =
                    source.position.extend(LIGHT_RADIUS).to_array();
                self.data.lights[index].radiance_shadow =
                    [strength, strength * 0.68, strength * 0.38, 0.0];
                self.data.lights[index].shape = [SOURCE_RADIUS, POINT_SHADOW_NEAR, 0.0, 0.0];
                if index < MAX_SHADOWED_LIGHTS {
                    let layer = first_layer + (index * POINT_SHADOW_FACES) as u32;
                    self.data.lights[index].radiance_shadow[3] = (layer + 1) as f32;
                    for (face, matrix) in point_shadow_matrices(source.position)
                        .into_iter()
                        .enumerate()
                    {
                        self.data.lights[index].clip[face] = matrix.to_cols_array_2d();
                        self.shadows.push(PointShadowView {
                            clip: matrix,
                            position: source.position,
                            layer: layer + face as u32,
                        });
                    }
                }
            }
            selection::tiles(
                &self.candidates,
                &inputs,
                &mut self.tile_scratch,
                &mut self.tile_scores,
                &mut self.tile_rays,
            );
            self.prepared = Some(inputs);
        }
        self.data.info = [
            self.candidates.len() as u32,
            POINT_SHADOW_RESOLUTION,
            u32::from(shadow_ready),
            0,
        ];
        let selected_changed = !self.upload_valid
            || (inputs_changed
                && (self.uploaded.lights != self.data.lights
                    || self.uploaded.info[0] != self.data.info[0]));
        self.dirty = selected_changed
            || sources.revision != self.revision
            || actor_dirty
            || (!was_submitted && self.dirty);
        self.revision = sources.revision;
        if selected_changed || self.uploaded.info != self.data.info {
            queue.write_buffer(&self.buffer, 0, bytemuck::bytes_of(&self.data));
            self.uploads += 1;
            self.uploaded = self.data;
            self.upload_valid = true;
        }
        let required = self.tile_scratch.len() * std::mem::size_of::<u32>();
        let resized = required > self.tile_capacity;
        if resized {
            self.tile_capacity = required.next_power_of_two();
            self.tiles = device.create_buffer(&BufferDescriptor {
                label: Some("Enhanced local light tiles"),
                size: self.tile_capacity as u64,
                usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }
        if resized || self.uploaded_tiles != self.tile_scratch {
            queue.write_buffer(&self.tiles, 0, bytemuck::cast_slice(&self.tile_scratch));
            self.uploads += 1;
            self.uploaded_tiles.clone_from(&self.tile_scratch);
        }
        resized
    }
}

pub(crate) fn point_shadow_matrices(position: Vec3) -> [Mat4; POINT_SHADOW_FACES] {
    let directions = [
        (Vec3::X, -Vec3::Y),
        (-Vec3::X, -Vec3::Y),
        (Vec3::Y, Vec3::Z),
        (-Vec3::Y, -Vec3::Z),
        (Vec3::Z, -Vec3::Y),
        (-Vec3::Z, -Vec3::Y),
    ];
    let projection = Mat4::perspective_rh(
        std::f32::consts::FRAC_PI_2,
        1.0,
        POINT_SHADOW_NEAR,
        LIGHT_RADIUS,
    );
    directions
        .map(|(direction, up)| projection * Mat4::look_at_rh(position, position + direction, up))
}

/// Hashes submitted nearby caster poses rather than advancing animation clock stamps.
pub(crate) fn near_actor_signature(
    frame: &crate::ActorRenderFrame,
    shadows: &[PointShadowView],
) -> u64 {
    let mut hash = DefaultHasher::new();
    for entry in frame.rig.manifest.iter() {
        if entry.route == crate::ActorRigRoute::NoDraw {
            continue;
        }
        let Some(instance) = frame.rig.instances.get(entry.instance_index as usize) else {
            continue;
        };
        let position = Vec3::new(
            instance.world_from_actor[0][3],
            instance.world_from_actor[1][3],
            instance.world_from_actor[2][3],
        );
        if !shadows
            .iter()
            .step_by(POINT_SHADOW_FACES)
            .any(|light| position.distance_squared(light.position) <= (LIGHT_RADIUS + 2.0).powi(2))
        {
            continue;
        }
        instance.geometry_id.hash(&mut hash);
        instance.texture_layer.hash(&mut hash);
        frame.skin_revision.hash(&mut hash);
        frame.artwork.identity().hash(&mut hash);
        frame
            .instance_pages
            .get(entry.instance_index as usize)
            .hash(&mut hash);
        frame.rig.geometry_revision.hash(&mut hash);
        for row in instance.world_from_actor {
            for (column, value) in row.into_iter().enumerate() {
                ((value * if column == 3 { 64.0 } else { 1024.0 }).round() as i32).hash(&mut hash);
            }
        }
        let partial = instance.partial_tick.clamp(0.0, 1.0);
        for bone in 0..entry.bone_count as usize {
            let Some(previous) = frame
                .rig
                .previous_bones
                .get(entry.previous_bone_base as usize + bone)
            else {
                continue;
            };
            let Some(current) = frame
                .rig
                .current_bones
                .get(entry.current_bone_base as usize + bone)
            else {
                continue;
            };
            for (previous, current) in previous.iter().flatten().zip(current.iter().flatten()) {
                let posed = previous + (current - previous) * partial;
                ((posed * 1024.0).round() as i32).hash(&mut hash);
            }
        }
    }
    hash.finish()
}
