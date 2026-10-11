//! Shipped compiled faces used by OreUI's semantic text styles.

use std::{fs::File, io::Read, path::Path, sync::Arc};

use assets::RuntimeFontCatalog;

use ui::oreui_theme::OreUiFont;

/// Attaches valid optional carriers, retaining the default font for unavailable faces.
pub fn install(base: Arc<RuntimeFontCatalog>, compiled_dir: &Path) -> Arc<RuntimeFontCatalog> {
    let mut combined = (*base).clone();
    for face in OreUiFont::ALL {
        let path = compiled_dir.join(face.carrier().output);
        match load(face, &path).and_then(|font| {
            combined
                .with_named_font(face.name(), &font)
                .map_err(|e| e.to_string())
        }) {
            Ok(font) => combined = font,
            Err(reason) => eprintln!(
                "{} unavailable at {} ({reason}); using default font; rebuild with make {}-assets",
                face.name(),
                path.display(),
                face.carrier().name
            ),
        }
    }
    if combined.identity() == base.identity() {
        base
    } else {
        Arc::new(combined)
    }
}

/// Bounds the read and validates both the carrier payload and its pinned source manifest.
fn load(face: OreUiFont, path: &Path) -> Result<RuntimeFontCatalog, String> {
    let file = File::open(path).map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    file.take(assets::MAX_FONT_CARRIER_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > assets::MAX_FONT_CARRIER_BYTES {
        return Err("font carrier exceeds its byte bound".into());
    }
    let profile = face.carrier().font_face.unwrap();
    let manifest = assets::canonical_source_manifest_sha256(profile.manifest);
    RuntimeFontCatalog::decode(&bytes, manifest)
        .and_then(|font| font.with_line_metrics(profile.line_metrics()))
        .map(|font| font.with_coverage_pages())
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests;
