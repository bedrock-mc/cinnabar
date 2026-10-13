//! Dropped-item admission and raster preparation before GPU allocation.

use super::{ItemMeshRange, mesh_storage::MeshStorage};
use render_model::{
    DroppedItemModel, ItemMeshVertex, MAX_ITEM_LAYERS, MIN_ITEM_ATLAS_SIDE,
    dropped_item::{
        DroppedItemBounds, block_mesh, cube_mesh, extruded_sprite_mesh, native_dropped_sprite_mesh,
    },
};

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

/// Counts artwork layers without the reserved white layer.
fn model_texture_layers(model: &DroppedItemModel) -> usize {
    match model {
        DroppedItemModel::Vacant => 0,
        DroppedItemModel::Sprite(_) | DroppedItemModel::NativeSprite(_) => 1,
        DroppedItemModel::Cube(cube) => cube.faces.len(),
        DroppedItemModel::Block(block) => block.materials.len(),
    }
}

/// Returns the minimum physical extent for a model that can fit alongside the white layer.
pub(super) fn model_side(
    model: &DroppedItemModel,
    policy: assets::texture::policy::TexturePolicy,
) -> Option<u32> {
    let side = match model {
        DroppedItemModel::Block(block) => block
            .materials
            .iter()
            .map(|(sprite, _)| sprite.width.max(sprite.height))
            .max(),
        DroppedItemModel::Cube(cube) => Some(cube.tile),
        DroppedItemModel::Sprite(sprite) | DroppedItemModel::NativeSprite(sprite) => {
            Some(sprite.width.max(sprite.height))
        }
        DroppedItemModel::Vacant => None,
    }?
    .checked_next_power_of_two()?
    .max(MIN_ITEM_ATLAS_SIDE);
    let layers = model_texture_layers(model).checked_add(1)?;
    let bytes = policy.page_bytes(side, side)?.checked_mul(layers)?;
    (layers <= MAX_ITEM_LAYERS && bytes <= assets::texture::policy::MAX_SUBSCRIBER_BYTES)
        .then_some(side)
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
    /// Admits models in order without rejected models enlarging pages. The policy must fit
    /// the minimum white layer, which the GPU caller checks before preparation.
    pub(super) fn new(
        models: &[DroppedItemModel],
        policy: assets::texture::policy::TexturePolicy,
        max_array_layers: u32,
        max_buffer_size: u64,
    ) -> Self {
        let mut side = MIN_ITEM_ATLAS_SIDE;
        let mut layers = 1;
        let mut vertices = MeshStorage::new(max_buffer_size);
        let mut entries = Vec::with_capacity(models.len());
        for model in models {
            let start = vertices.len() as u32;
            let mut uv_side = side;
            let needed = model_texture_layers(model);
            if let Some(candidate) = model_side(model, policy).map(|extent| extent.max(side))
                && layers + needed <= max_array_layers as usize
                && policy
                    .page_bytes(candidate, candidate)
                    .and_then(|bytes| bytes.checked_mul(layers + needed))
                    .is_some_and(|bytes| bytes <= assets::texture::policy::MAX_SUBSCRIBER_BYTES)
                && let Some(mesh) = build_mesh(model, candidate, layers)
                && !mesh.is_empty()
                && vertices.append(mesh)
            {
                side = candidate;
                uv_side = candidate;
                layers += needed;
            }
            entries.push((
                ItemMeshRange {
                    vertices: start..vertices.len() as u32,
                    cull_back: model.can_cull_back_faces(),
                    bounds: DroppedItemBounds::from_mesh(&vertices[start as usize..]),
                },
                uv_side,
            ));
        }
        let layer_bytes = side as usize * side as usize * 4;
        let mut atlas = Vec::with_capacity(layer_bytes * layers);
        atlas.resize(layer_bytes, 255);
        for (model, (range, uv_side)) in models.iter().zip(&entries) {
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
