//! Measures borrowed item artwork before model construction.
use super::*;
use render_model::dropped_item::{ItemModelCost, block_quad_vertex_count};

/// Measures a selected sprite with the exact reduction and alpha used during construction.
pub(super) fn icon_cost(
    icons: &UiPresentationRuntime,
    icon: ui::IconRef,
    native: bool,
) -> Option<ItemModelCost> {
    icons.resolved_item_cost(
        icon,
        assets::texture::policy::device_policy()
            .max_side()
            .min(MAX_ITEM_SPRITE_SIDE),
        native,
    )
}

/// Measures the same carried sheet and compatible shape used by the startup cube route.
pub(super) fn carried_cost(
    icons: &UiPresentationRuntime,
    assets: &RuntimeAssets,
    mode: NetworkIdMode,
    id: u32,
) -> Option<ItemModelCost> {
    let block = assets.resolve(mode, id);
    if !block.is_known() {
        return None;
    }
    let visual = match mode {
        NetworkIdMode::Sequential => id,
        NetworkIdMode::Hashed => assets.sequential_id_for_hash(id)?,
    };
    if block.kind() != VisualKind::Cube
        && assets::gui_item::full_cube_model_quads(assets, assets::BlockVisualId(visual)).is_none()
    {
        return None;
    }
    icons.carried_block_cost(visual, assets.provenance().source_manifest_sha256)
}

/// Measures cube faces or the block's authored template without retaining any pixels.
pub(super) fn block_cost(
    assets: &RuntimeAssets,
    mode: NetworkIdMode,
    id: u32,
) -> Option<ItemModelCost> {
    let block = assets.resolve(mode, id);
    if !block.is_known() {
        return None;
    }
    if block.kind() == VisualKind::Cube {
        let mut side = None;
        let mut pixels = 0;
        for face in BlockFace::ALL {
            let material = assets.material(block.face(face).material_id());
            let mip = assets
                .texture_pages()
                .get(material.texture.page() as usize)?
                .texture
                .mips
                .first()?;
            let size = mip.size;
            if size == 0 || size > MAX_ITEM_SPRITE_SIDE || *side.get_or_insert(size) != size {
                return None;
            }
            let bytes = (size as usize).checked_mul(size as usize)?.checked_mul(4)?;
            let start = (material.texture.layer() as usize).checked_mul(bytes)?;
            mip.rgba8.get(start..start.checked_add(bytes)?)?;
            pixels += bytes;
        }
        if pixels > assets::texture::policy::MAX_SUBSCRIBER_BYTES {
            return None;
        }
        return ItemModelCost::new(side?, BlockFace::ALL.len(), pixels, 36);
    }
    if !matches!(block.kind(), VisualKind::Model | VisualKind::Cross) {
        return None;
    }
    template_cost(assets, block.model_template()?)
}

/// Measures distinct template materials and drawable quads using downstream validation rules.
pub(super) fn template_cost(assets: &RuntimeAssets, mut template_id: u32) -> Option<ItemModelCost> {
    let mut materials = std::collections::HashSet::new();
    let mut side = 0;
    let mut pixels = 0usize;
    let mut vertices = 0usize;
    loop {
        let template = assets.model_templates().get(template_id as usize)?;
        let first = template.quad_start as usize;
        for quad in assets
            .model_quads()
            .get(first..first + template.quad_count as usize)?
        {
            vertices = vertices.checked_add(block_quad_vertex_count(quad))?;
            if !materials.insert(quad.material) {
                continue;
            }
            let material = assets.material(quad.material);
            let mip = assets
                .texture_pages()
                .get(material.texture.page() as usize)?
                .texture
                .mips
                .first()?;
            if mip.size == 0 || mip.size > assets::MAX_TILE_SIZE {
                return None;
            }
            let bytes = (mip.size as usize)
                .checked_mul(mip.size as usize)?
                .checked_mul(4)?;
            let first = (material.texture.layer() as usize).checked_mul(bytes)?;
            mip.rgba8.get(first..first.checked_add(bytes)?)?;
            side = side.max(mip.size);
            pixels = pixels.checked_add(bytes)?;
            if pixels > assets::texture::policy::MAX_SUBSCRIBER_BYTES {
                return None;
            }
        }
        if template.flags & assets::MODEL_TEMPLATE_FLAG_COMPOUND_NEXT == 0 {
            break;
        }
        template_id = template_id.checked_add(1)?;
    }
    ItemModelCost::new(side, materials.len(), pixels, vertices)
}
