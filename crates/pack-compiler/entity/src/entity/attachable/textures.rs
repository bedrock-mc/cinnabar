//! Every frame referenced by an attachable's compiled render controllers, not just standby.

use pack_compile_input::source::read_bounded_source;

use super::invalid;
use assets::{
    AssetError, CompiledEntityAssets, EntityAssetKind, EntityAssetSource, EquipmentBinding,
    EquipmentCategory, EquipmentTexture, MAX_EQUIPMENT_PIXEL_BYTES, MAX_EQUIPMENT_TEXTURE_SIDE,
    MAX_EQUIPMENT_TEXTURES,
};
use image::{ImageFormat, ImageReader, Limits};
use sha2::{Digest, Sha256};
use std::{io::Cursor, path::Path, sync::Arc};

/// Includes the shared enchantment rasters alongside every requested equipment image.
pub(super) fn compile_texture_identifiers_with(
    sources: &[EntityAssetSource],
    identifiers: Vec<&str>,
    read: &mut dyn FnMut(&EntityAssetSource) -> Result<Vec<u8>, AssetError>,
) -> Result<Vec<EquipmentTexture>, AssetError> {
    compile_texture_identifiers_with_priorities(sources, identifiers, &[], read)
}

/// Compiles requested rasters while retaining selected identifiers before ordinary art.
pub(super) fn compile_texture_identifiers_with_priorities(
    sources: &[EntityAssetSource],
    mut identifiers: Vec<&str>,
    priorities: &[&str],
    read: &mut dyn FnMut(&EntityAssetSource) -> Result<Vec<u8>, AssetError>,
) -> Result<Vec<EquipmentTexture>, AssetError> {
    let mut seen = std::collections::HashSet::new();
    let mut ordered = Vec::with_capacity(priorities.len() + identifiers.len());
    for identifier in priorities.iter().copied() {
        if seen.insert(identifier) {
            ordered.push(identifier);
        }
    }
    identifiers.extend(assets::entity::GLINT_TEXTURE_IDENTIFIERS);
    identifiers.sort_unstable();
    identifiers.dedup();
    ordered.extend(
        identifiers
            .into_iter()
            .filter(|identifier| seen.insert(*identifier)),
    );
    let mut textures = Vec::new();
    let mut pixel_bytes = 0usize;
    for identifier in ordered {
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
    textures.sort_by(|a, b| a.identifier.cmp(&b.identifier));
    Ok(textures)
}

/// Decodes one equipment raster within the per-image and remaining pixel budgets.
pub fn decode_raster(
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
    compile_textures_for_assets_with_priorities(assets, bindings, &[], read)
}

/// In-memory equipment raster compilation with paths that must survive the texture budget.
pub fn compile_textures_for_assets_with_priorities(
    assets: &CompiledEntityAssets,
    bindings: &[EquipmentBinding],
    priorities: &[&str],
    read: &mut dyn FnMut(&EntityAssetSource) -> Result<Vec<u8>, AssetError>,
) -> Result<Vec<EquipmentTexture>, AssetError> {
    let mut identifiers = bindings
        .iter()
        .map(|binding| binding.texture.identifier.as_ref())
        .collect::<Vec<_>>();
    identifiers.extend(
        bindings
            .iter()
            .flat_map(|binding| binding.resources.textures.values().map(AsRef::as_ref)),
    );
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
    let mut priority_storage = priorities.to_vec();
    priority_storage.extend(
        bindings
            .iter()
            .filter(|binding| {
                matches!(
                    binding.category,
                    EquipmentCategory::Armor { .. } | EquipmentCategory::ArmorAndHeld { .. }
                )
            })
            .flat_map(|binding| {
                std::iter::once(binding.texture.identifier.as_ref())
                    .chain(binding.resources.textures.values().map(Box::as_ref))
            }),
    );
    compile_texture_identifiers_with_priorities(
        &assets.sources,
        identifiers,
        &priority_storage,
        read,
    )
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

    #[test]
    fn priority_texture_survives_the_sorted_texture_limit() {
        let raster = png(1, 1);
        let mut sources = Vec::new();
        for index in 0..=MAX_EQUIPMENT_TEXTURES {
            let path: Box<str> = format!("textures/items/{index:03}.png").into();
            sources.push(EntityAssetSource {
                path,
                source_bytes: raster.len() as u32,
                source_sha256: [0; 32],
            });
        }
        let identifiers = sources
            .iter()
            .map(|source| source.path.trim_end_matches(".png"))
            .collect::<Vec<_>>();
        let ordinary = compile_texture_identifiers_with(&sources, identifiers.clone(), &mut |_| {
            Ok(raster.clone())
        })
        .unwrap();
        assert!(
            !ordinary
                .iter()
                .any(|texture| texture.identifier.as_ref() == "textures/items/256")
        );
        let prioritized = compile_texture_identifiers_with_priorities(
            &sources,
            identifiers,
            &["textures/items/256"],
            &mut |_| Ok(raster.clone()),
        )
        .unwrap();
        assert!(
            prioritized
                .iter()
                .any(|texture| texture.identifier.as_ref() == "textures/items/256")
        );
        let mut sorted = prioritized
            .iter()
            .map(|texture| texture.identifier.as_ref())
            .collect::<Vec<_>>();
        let expected = sorted.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, expected);
    }
}
