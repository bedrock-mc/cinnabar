//! Every frame referenced by an attachable's compiled render controllers, not just standby.

use super::{invalid, read_bounded_source};
use assets::{
    AssetError, CompiledEntityAssets, EntityAssetKind, EntityAssetSource, EquipmentBinding,
    EquipmentTexture, MAX_EQUIPMENT_PIXEL_BYTES, MAX_EQUIPMENT_TEXTURE_SIDE,
    MAX_EQUIPMENT_TEXTURES,
};
use image::{ImageFormat, ImageReader, Limits};
use sha2::{Digest, Sha256};
use std::{io::Cursor, path::Path, sync::Arc};

/// Includes the shared enchantment raster alongside every requested equipment image.
pub(super) fn compile_texture_identifiers_with(
    sources: &[EntityAssetSource],
    mut identifiers: Vec<&str>,
    read: &mut dyn FnMut(&EntityAssetSource) -> Result<Vec<u8>, AssetError>,
) -> Result<Vec<EquipmentTexture>, AssetError> {
    identifiers.push(assets::ACTOR_GLINT_TEXTURE_IDENTIFIER);
    identifiers.sort_unstable();
    identifiers.dedup();
    let mut textures = Vec::new();
    let mut pixel_bytes = 0usize;
    for identifier in identifiers {
        if textures.len() == MAX_EQUIPMENT_TEXTURES {
            break;
        }
        let Some(source) = ["png", "tga"].into_iter().find_map(|extension| {
            let path = format!("{identifier}.{extension}");
            sources
                .iter()
                .find(|source| source.path.as_ref() == path)
                .map(|source| (source, extension))
        }) else {
            continue;
        };
        let (source, extension) = source;
        let bytes = read(source)?;
        let format = if extension == "png" {
            ImageFormat::Png
        } else {
            ImageFormat::Tga
        };
        let Some((width, height, rgba8)) =
            decode_raster(&bytes, format, MAX_EQUIPMENT_PIXEL_BYTES - pixel_bytes)
        else {
            continue;
        };
        pixel_bytes += rgba8.len();
        textures.push(EquipmentTexture {
            identifier: identifier.into(),
            width,
            height,
            rgba8,
        });
    }
    Ok(textures)
}

fn decode_raster(
    bytes: &[u8],
    format: ImageFormat,
    remaining: usize,
) -> Option<(u16, u16, Arc<[u8]>)> {
    let (width, height) = ImageReader::with_format(Cursor::new(bytes), format)
        .into_dimensions()
        .ok()?;
    let (width, height) = (u16::try_from(width).ok()?, u16::try_from(height).ok()?);
    let side_ok = |side: u16| (1..=MAX_EQUIPMENT_TEXTURE_SIDE).contains(&side);
    let pixel_bytes = usize::from(width)
        .checked_mul(usize::from(height))?
        .checked_mul(4)?;
    if !side_ok(width) || !side_ok(height) || pixel_bytes > remaining {
        return None;
    }
    let mut reader = ImageReader::with_format(Cursor::new(bytes), format);
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_EQUIPMENT_TEXTURE_SIDE.into());
    limits.max_image_height = Some(MAX_EQUIPMENT_TEXTURE_SIDE.into());
    limits.max_alloc = Some(MAX_EQUIPMENT_PIXEL_BYTES as u64);
    reader.limits(limits);
    let image = reader.decode().ok()?;
    Some((width, height, image.into_rgba8().into_raw().into()))
}

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

#[cfg(test)]
mod tests {
    use super::*;

    fn png(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = Cursor::new(Vec::new());
        image::RgbaImage::from_pixel(width, height, image::Rgba([37, 59, 83, 255]))
            .write_to(&mut bytes, ImageFormat::Png)
            .unwrap();
        bytes.into_inner()
    }

    #[test]
    fn equipment_texture_decoder_refuses_an_image_past_remaining_pixel_budget() {
        let image = png(2, 2);
        assert!(decode_raster(&image, ImageFormat::Png, 15).is_none());
        let (_, _, pixels) = decode_raster(&image, ImageFormat::Png, 16).unwrap();
        assert_eq!(pixels.len(), 16);
        assert!(decode_raster(&png(1, 1), ImageFormat::Png, 15).is_some());
    }
}
