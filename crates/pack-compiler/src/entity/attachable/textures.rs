//! Every frame referenced by an attachable's compiled render controllers, not just standby.

use super::{compile_texture_identifiers_with, invalid, read_bounded_source};
use assets::{
    AssetError, CompiledEntityAssets, EntityAssetKind, EntityAssetSource, EquipmentBinding,
    EquipmentTexture,
};
use sha2::{Digest, Sha256};
use std::path::Path;

pub fn compile_textures_for_assets(
    root: &Path,
    assets: &CompiledEntityAssets,
    bindings: &[EquipmentBinding],
) -> Result<Vec<EquipmentTexture>, AssetError> {
    compile_textures_for_assets_with(assets, bindings, &mut |source| {
        let bytes = read_bounded_source(root, &root.join(source.path.as_ref()))?;
        if bytes.len() != source.source_bytes as usize
            || <[u8; 32]>::from(Sha256::digest(&bytes)) != source.source_sha256
        {
            return Err(invalid("equipment raster changed after entity compilation"));
        }
        Ok(bytes)
    })
}

/// In-memory counterpart that includes all retained attachable render-frame candidates.
pub fn compile_textures_for_assets_with(
    assets: &CompiledEntityAssets,
    bindings: &[EquipmentBinding],
    read: &mut dyn FnMut(&EntityAssetSource) -> Result<Vec<u8>, AssetError>,
) -> Result<Vec<EquipmentTexture>, AssetError> {
    let mut identifiers = bindings
        .iter()
        .map(|binding| binding.texture.identifier.as_ref())
        .collect::<Vec<_>>();
    for layer in &assets.render.layers {
        let rig = assets
            .rig_bindings
            .get(layer.rig as usize)
            .ok_or_else(|| invalid("equipment render rig is absent"))?;
        let symbol = assets
            .symbols
            .get(rig.entity_symbol as usize)
            .ok_or_else(|| invalid("equipment render symbol is absent"))?;
        if symbol.kind != EntityAssetKind::Attachable {
            continue;
        }
        let slots = assets
            .render
            .slots
            .get(layer.first_slot as usize..)
            .and_then(|slots| slots.get(..usize::from(layer.slot_count)))
            .ok_or_else(|| invalid("equipment render slot range is invalid"))?;
        for slot in slots {
            let candidates = assets
                .render
                .candidates
                .get(slot.first_candidate as usize..)
                .and_then(|candidates| candidates.get(..usize::from(slot.candidate_count)))
                .ok_or_else(|| invalid("equipment render candidate range is invalid"))?;
            for candidate in candidates {
                let source = assets
                    .sources
                    .get(candidate.source as usize)
                    .ok_or_else(|| invalid("equipment render texture source is absent"))?;
                if let Some(stem) = source
                    .path
                    .strip_suffix(".png")
                    .or_else(|| source.path.strip_suffix(".tga"))
                {
                    identifiers.push(stem);
                }
            }
        }
    }
    compile_texture_identifiers_with(&assets.sources, identifiers, read)
}
