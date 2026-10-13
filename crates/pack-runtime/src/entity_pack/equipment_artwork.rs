//! Texture-only packs replace startup artwork without introducing attachable bindings.

use assets::{EquipmentTexture, RuntimeEquipmentCatalog};
use resource_pack::LayeredPackView;

/// Retains startup-path raster sources for compilation without allocating decoded pixels.
pub(super) fn collect(
    view: &LayeredPackView,
    base: Option<&RuntimeEquipmentCatalog>,
    files: &mut Vec<(Box<str>, Vec<u8>)>,
) -> Vec<(Box<str>, image::ImageFormat)> {
    let mut sources = Vec::new();
    for texture in base.into_iter().flat_map(RuntimeEquipmentCatalog::textures) {
        for (extension, format) in [
            ("png", image::ImageFormat::Png),
            ("tga", image::ImageFormat::Tga),
        ] {
            let path = format!("{}.{extension}", texture.identifier);
            let Some(bytes) = view.read_capped(&path, resource_pack::MAX_FILE_BYTES) else {
                continue;
            };
            sources.push((Box::<str>::from(path.as_str()), format));
            if !files.iter().any(|(existing, _)| existing.as_ref() == path) {
                files.push((path.into(), bytes.into_vec()));
            }
            break;
        }
    }
    sources
}

/// Reuses compiler rasters and decodes only missing startup paths within the remaining budgets.
pub(super) fn merge(
    view: &LayeredPackView,
    textures: &mut Vec<EquipmentTexture>,
    sources: Vec<(Box<str>, image::ImageFormat)>,
) {
    let mut retained: usize = textures.iter().map(|texture| texture.rgba8.len()).sum();
    for (path, format) in sources {
        let Some((identifier, _)) = path.rsplit_once('.') else {
            continue;
        };
        if textures
            .iter()
            .any(|texture| texture.identifier.as_ref() == identifier)
            || textures.len() == assets::MAX_EQUIPMENT_TEXTURES
        {
            continue;
        }
        let Some(bytes) = view.read_capped(&path, resource_pack::MAX_FILE_BYTES) else {
            continue;
        };
        let Some((width, height, rgba8)) = pack_compiler::decode_equipment_raster(
            &bytes,
            format,
            assets::MAX_EQUIPMENT_PIXEL_BYTES - retained,
        ) else {
            continue;
        };
        retained += rgba8.len();
        textures.push(EquipmentTexture {
            identifier: identifier.into(),
            width,
            height,
            rgba8,
        });
    }
    textures.sort_by(|a, b| a.identifier.cmp(&b.identifier));
}

#[cfg(test)]
mod tests;
