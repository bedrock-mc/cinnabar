//! Dropped-item admission, raster preparation and their recoverable GPU upload.

use super::{ItemGpu, ItemMeshRange, mesh_storage::MeshStorage};
use crate::gpu_allocation;
use bevy::render::{
    render_resource::{
        BufferUsages, Extent3d, TextureDescriptor, TextureDimension, TextureFormat, TextureUsages,
        TextureViewDescriptor, TextureViewDimension,
    },
    renderer::{RenderDevice, RenderQueue},
};
use render_model::{
    DroppedItemModel, ItemMeshVertex, MAX_ITEM_LAYERS, MIN_ITEM_ATLAS_SIDE,
    dropped_item::{
        DroppedItemBounds, atlas_layer_limit, block_mesh, cube_mesh, extruded_sprite_mesh,
        native_dropped_sprite_mesh,
    },
};
use render_scene::DroppedItemScene;

/// Appends a valid square-or-smaller RGBA tile as a new atlas layer.
fn push_layer(atlas: &mut Vec<u8>, side: usize, width: usize, height: usize, rgba8: &[u8]) {
    let layer_bytes = side * side * 4;
    let layer = atlas.len() / layer_bytes;
    atlas.resize(atlas.len() + layer_bytes, 0);
    let row_bytes = width * 4;
    for row in 0..height {
        let target = layer * layer_bytes + row * side * 4;
        atlas[target..target + row_bytes]
            .copy_from_slice(&rgba8[row * row_bytes..(row + 1) * row_bytes]);
    }
}

/// Builds vertices without retaining texture layers; rejects malformed or excessive geometry.
fn build_mesh(
    model: &DroppedItemModel,
    side: u32,
    layers_used: usize,
) -> Option<Vec<ItemMeshVertex>> {
    match model {
        DroppedItemModel::Vacant => None,
        DroppedItemModel::Sprite(sprite) | DroppedItemModel::NativeSprite(sprite) => {
            let (width, height) = (sprite.width as usize, sprite.height as usize);
            if layers_used >= MAX_ITEM_LAYERS
                || sprite.width > side
                || sprite.height > side
                || sprite.rgba8.len() != width * height * 4
            {
                return None;
            }
            let build = if matches!(model, DroppedItemModel::NativeSprite(_)) {
                native_dropped_sprite_mesh
            } else {
                extruded_sprite_mesh
            };
            build(
                sprite.width,
                sprite.height,
                &sprite.rgba8,
                side,
                layers_used as u32,
            )
        }
        DroppedItemModel::Cube(cube) => {
            let tile = cube.tile as usize;
            if cube.tile == 0
                || cube.tile > side
                || layers_used + 6 > MAX_ITEM_LAYERS
                || cube.faces.iter().any(|face| face.len() != tile * tile * 4)
            {
                return None;
            }
            let layers = std::array::from_fn(|face| (layers_used + face) as u32);
            cube_mesh(layers, cube.tints, cube.tile, side)
        }
        DroppedItemModel::Block(block) => {
            for (sprite, _) in block.materials.iter() {
                if sprite.width == 0
                    || sprite.height == 0
                    || sprite.width > side
                    || sprite.height > side
                    || sprite.rgba8.len() != (sprite.width * sprite.height * 4) as usize
                {
                    return None;
                }
            }
            block_mesh(block, layers_used as u32, side)
        }
    }
}

/// Copies valid textures only after their model's geometry and layers have been admitted.
fn copy_model_layers(atlas: &mut Vec<u8>, model: &DroppedItemModel, side: u32) {
    let mut push = |width: u32, height: u32, pixels: &[u8]| {
        push_layer(
            atlas,
            side as usize,
            width as usize,
            height as usize,
            pixels,
        );
    };
    match model {
        DroppedItemModel::Vacant => {}
        DroppedItemModel::Sprite(sprite) | DroppedItemModel::NativeSprite(sprite) => {
            push(sprite.width, sprite.height, &sprite.rgba8);
        }
        DroppedItemModel::Cube(cube) => {
            for face in &cube.faces {
                push(cube.tile, cube.tile, face);
            }
        }
        DroppedItemModel::Block(block) => {
            for (sprite, _) in block.materials.iter() {
                push(sprite.width, sprite.height, &sprite.rgba8);
            }
        }
    }
}

/// Builds one model and its layers for geometry and raster regression fixtures.
#[cfg(test)]
pub(super) fn build_model(
    atlas: &mut Vec<u8>,
    model: &DroppedItemModel,
    side: u32,
) -> Option<Vec<ItemMeshVertex>> {
    let mesh = build_mesh(model, side, atlas.len() / (side * side * 4) as usize)?;
    copy_model_layers(atlas, model, side);
    Some(mesh)
}

/// CPU payload containing only models that fit both geometry and physical texture budgets.
pub(super) struct PreparedModels {
    pub(super) side: u32,
    pub(super) atlas: Vec<u8>,
    pub(super) vertices: MeshStorage,
    pub(super) ranges: Vec<ItemMeshRange>,
}

impl PreparedModels {
    /// Admits current models before cached extras, using the same order for mesh and texture
    /// budgets. The policy must fit the minimum white layer, checked by the GPU caller.
    pub(super) fn new(
        models: &[DroppedItemModel],
        current: &[bool],
        policy: assets::texture::policy::TexturePolicy,
        max_array_layers: u32,
        max_buffer_size: u64,
    ) -> Self {
        let mut side = MIN_ITEM_ATLAS_SIDE;
        let mut layers = 1;
        let mut vertices = MeshStorage::new(max_buffer_size);
        let order: Vec<_> = (0..models.len())
            .filter(|&slot| current.get(slot).copied().unwrap_or(false))
            .chain((0..models.len()).filter(|&slot| !current.get(slot).copied().unwrap_or(false)))
            .collect();
        let mut entries: Vec<_> = models
            .iter()
            .map(|model| {
                (
                    ItemMeshRange {
                        vertices: 0..0,
                        cull_back: model.can_cull_back_faces(),
                        bounds: None,
                        capacity_rejected: false,
                    },
                    side,
                )
            })
            .collect();
        for &slot in &order {
            let model = &models[slot];
            let start = vertices.len() as u32;
            let mut uv_side = side;
            let needed = model.texture_layers();
            let mut capacity_rejected = false;
            if let Some(candidate) = model.atlas_side(policy).map(|extent| extent.max(side))
                && layers + needed
                    <= atlas_layer_limit(candidate, policy, max_array_layers as usize)
                && let Some(mesh) = build_mesh(model, candidate, layers)
                && !mesh.is_empty()
            {
                let fits_empty = vertices.fits_empty(mesh.len());
                if vertices.append(mesh) {
                    side = candidate;
                    uv_side = candidate;
                    layers += needed;
                } else {
                    capacity_rejected = fits_empty;
                }
            }
            entries[slot] = (
                ItemMeshRange {
                    vertices: start..vertices.len() as u32,
                    cull_back: model.can_cull_back_faces(),
                    bounds: DroppedItemBounds::from_mesh(&vertices[start as usize..]),
                    capacity_rejected,
                },
                uv_side,
            );
        }
        let layer_bytes = side as usize * side as usize * 4;
        let mut atlas = Vec::with_capacity(layer_bytes * layers);
        atlas.resize(layer_bytes, 255);
        for slot in order {
            let model = &models[slot];
            let (range, uv_side) = &entries[slot];
            if range.vertices.is_empty() {
                continue;
            }
            if *uv_side != side {
                let scale = *uv_side as f32 / side as f32;
                for vertex in &mut vertices.vertices_mut()
                    [range.vertices.start as usize..range.vertices.end as usize]
                {
                    vertex.uv = vertex.uv.map(|coordinate| coordinate * scale);
                    // Preserve texture-local foil coordinates as the atlas extent grows.
                    vertex.glint_scale = vertex.glint_scale.map(|extent| extent / scale);
                }
            }
            copy_model_layers(&mut atlas, model, side);
        }
        Self {
            side,
            atlas,
            vertices,
            ranges: entries.into_iter().map(|(range, _)| range).collect(),
        }
    }
}

/// Uploads current models before cached extras while keeping model handles unchanged.
/// The superseded atlas is released first because its layers no longer match the new ranges.
pub(super) fn rebuild_models(
    scene: &DroppedItemScene,
    current: &[bool],
    device: &RenderDevice,
    queue: &RenderQueue,
    gpu: &mut ItemGpu,
) {
    let policy = assets::texture::policy::TexturePolicy::for_device(
        device.limits().max_texture_dimension_2d,
    );
    if policy
        .page_bytes(MIN_ITEM_ATLAS_SIDE, MIN_ITEM_ATLAS_SIDE)
        .is_none()
    {
        return;
    }
    let PreparedModels {
        side,
        atlas,
        vertices,
        ranges,
    } = PreparedModels::new(
        &scene.models,
        current,
        policy,
        device.limits().max_texture_array_layers,
        device.limits().max_buffer_size,
    );
    let layer_bytes = side as usize * side as usize * 4;
    let layers = atlas.len() / layer_bytes;
    let released = gpu._atlas.take().is_some();
    gpu.mesh_buffer = None;
    gpu.atlas_view = None;
    gpu.bind_group = None;
    gpu.ranges.clear();
    gpu.models_revision = scene.models_revision;
    gpu.prioritized_models = current.to_vec();
    let uploaded = upload(device, queue, side, layers as u32, &atlas, &vertices);
    gpu.upload.record(uploaded.is_some(), released, queue);
    let Some((texture, view, mesh)) = uploaded else {
        return;
    };
    gpu.mesh_buffer = mesh;
    gpu.ranges = ranges;
    gpu.atlas_view = Some(view);
    gpu._atlas = Some(texture);
}

/// Allocates the mesh, layer array and view; any out-of-memory failure returns nothing.
fn upload(
    device: &RenderDevice,
    queue: &RenderQueue,
    side: u32,
    layers: u32,
    atlas: &[u8],
    vertices: &MeshStorage,
) -> Option<(
    bevy::render::render_resource::Texture,
    bevy::render::render_resource::TextureView,
    Option<bevy::render::render_resource::Buffer>,
)> {
    let mesh = if vertices.is_empty() {
        None
    } else {
        Some(gpu_allocation::buffer_with_data(
            device,
            queue,
            "dropped item model meshes",
            bytemuck::cast_slice::<ItemMeshVertex, u8>(vertices),
            BufferUsages::VERTEX,
        )?)
    };
    let texture = gpu_allocation::texture_with_data(
        device,
        queue,
        &TextureDescriptor {
            label: Some("dropped item texture layers"),
            size: Extent3d {
                width: side,
                height: side,
                depth_or_array_layers: layers,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::Rgba8UnormSrgb,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            view_formats: &[],
        },
        atlas,
    )?;
    let view = gpu_allocation::checked(device, "dropped item texture layer array", 0, || {
        texture.create_view(&TextureViewDescriptor {
            label: Some("dropped item texture layer array"),
            dimension: Some(TextureViewDimension::D2Array),
            ..Default::default()
        })
    })?;
    Some((texture, view, mesh))
}
