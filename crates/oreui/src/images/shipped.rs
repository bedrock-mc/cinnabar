//! Shipped original artwork, compiled by `make oreui-icons-assets`.

use std::path::Path;

use assets::{
    canonical_source_manifest_sha256,
    carriers::OREUI_ICONS,
    oreui_icons::{
        MAX_OREUI_ICONS_BYTES, OREUI_ICONS_SOURCE_MANIFEST, OreUiIcon, decode_oreui_icons,
    },
};

use super::{Art, read_bounded};

/// Reads the carrier in `compiled_dir`; the error names its path and rebuild command.
pub(super) fn read(compiled_dir: &Path) -> Result<Vec<(String, Art)>, String> {
    let path = compiled_dir.join(OREUI_ICONS.output);
    read_bounded(&path, MAX_OREUI_ICONS_BYTES as u64)
        .and_then(|bytes| decode(&bytes))
        .map_err(|reason| {
            format!(
                "OreUI shipped artwork unavailable at {} ({reason}); rebuild with make {}-assets",
                path.display(),
                OREUI_ICONS.name
            )
        })
}

/// Decodes a carrier compiled from the checked-in source manifest, tallest images first.
pub(super) fn decode(bytes: &[u8]) -> Result<Vec<(String, Art)>, String> {
    let manifest = canonical_source_manifest_sha256(OREUI_ICONS_SOURCE_MANIFEST);
    let mut icons = decode_oreui_icons(bytes, manifest).map_err(|error| error.to_string())?;
    icons.sort_by(|a, b| {
        b.height
            .cmp(&a.height)
            .then(b.width.cmp(&a.width))
            .then(a.key.cmp(&b.key))
    });
    Ok(icons.into_iter().map(art).collect())
}

fn art(icon: OreUiIcon) -> (String, Art) {
    let OreUiIcon {
        key,
        width,
        height,
        mut frames,
    } = icon;
    let art = if frames.len() == 1 {
        Art::Still(width, height, frames.remove(0).rgba8.into_vec())
    } else {
        Art::Animation(
            frames
                .into_iter()
                .map(|frame| (width, height, frame.rgba8.into_vec(), frame.millis))
                .collect(),
        )
    };
    (key, art)
}
