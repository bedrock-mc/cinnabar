//! Shipped compiled faces used by OreUI's semantic text styles.

use std::{collections::BTreeMap, fs::File, io::Read, path::Path, sync::Arc};

use assets::{FontRendering, RuntimeFontCatalog};

use assets::carriers;
use ui::oreui_theme::OreUiFont;

/// Attaches valid optional face bundles, retaining the default font for unavailable faces.
/// Faces map only their own ranges, so none attach without the shared Sans fallback.
pub fn install(base: Arc<RuntimeFontCatalog>, compiled_dir: &Path) -> Arc<RuntimeFontCatalog> {
    let mut combined = (*base).clone();
    let fallback = match shared_fallback(&mut combined, compiled_dir) {
        Ok(fallback) => fallback,
        Err(reason) => {
            eprintln!("OreUI faces unavailable without their Sans fallback ({reason}); using default font");
            return base;
        }
    };
    let mut installed = 0;
    for face in OreUiFont::ALL {
        let path = compiled_dir.join(face.carrier().output);
        match load(face, &path).and_then(|(font, sizes)| {
            combined
                .with_named_face(face.name(), &font, &sizes, Some(&fallback))
                .map_err(|e| e.to_string())
        }) {
            Ok(font) => {
                combined = font;
                installed += 1;
            }
            Err(reason) => eprintln!(
                "{} unavailable at {} ({reason}); using default font; rebuild with make {}-assets",
                face.name(),
                path.display(),
                face.carrier().name
            ),
        }
    }
    if installed == 0 {
        base
    } else {
        Arc::new(combined)
    }
}

/// Returns the shipped Sans catalog that draws characters outside the faces' native ranges,
/// reusing the default font's pages when it is that carrier.
fn shared_fallback(
    combined: &mut RuntimeFontCatalog,
    compiled_dir: &Path,
) -> Result<Arc<RuntimeFontCatalog>, String> {
    let sans = carriers::FONT.font_face.unwrap();
    let manifest = assets::canonical_source_manifest_sha256(sans.manifest);
    let font = if combined.identity().source_manifest_sha256 == manifest {
        combined.clone()
    } else {
        let decoded = RuntimeFontCatalog::decode(
            &read_bounded(&compiled_dir.join(carriers::FONT.output))?,
            manifest,
        )
        .map_err(|e| e.to_string())?
        .with_coverage_pages();
        *combined = combined
            .with_named_font(sans.name, &decoded)
            .map_err(|e| e.to_string())?;
        combined.font_named(sans.name).clone()
    };
    font.with_line_metrics(sans.line_metrics())
        .map(Arc::new)
        .map_err(|e| e.to_string())
}

fn read_bounded(path: &Path) -> Result<Vec<u8>, String> {
    let file = File::open(path).map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    file.take(assets::MAX_FONT_CARRIER_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > assets::MAX_FONT_CARRIER_BYTES {
        return Err("font carrier exceeds its byte bound".into());
    }
    Ok(bytes)
}

/// Returns the face's distance field and its exact small sizes, validated against the pinned
/// source manifest.
fn load(
    face: OreUiFont,
    path: &Path,
) -> Result<(RuntimeFontCatalog, BTreeMap<u32, RuntimeFontCatalog>), String> {
    let bytes = read_bounded(path)?;
    let profile = face.carrier().font_face.unwrap();
    let manifest = assets::canonical_source_manifest_sha256(profile.manifest);
    let spacing = shipped_spacing(profile)?;
    let (mut font, mut sizes) = (None, BTreeMap::new());
    for (em, rendering, raster) in
        assets::decode_font_face_bundle(&bytes, manifest).map_err(|e| e.to_string())?
    {
        let raster = raster
            .with_line_metrics(profile.line_metrics_at(em))
            .map_err(|e| e.to_string())?
            .with_rendering(rendering)
            .with_coverage_pages();
        let raster = with_spacing(&spacing, em, raster)?;
        match rendering {
            FontRendering::NativeSdf => font = Some(raster),
            FontRendering::NativeCoverage => {
                sizes.insert(em, raster);
            }
            FontRendering::Coverage => return Err("face bundle holds a pixel-grid raster".into()),
        }
    }
    Ok((font.ok_or("face bundle has no distance-field raster")?, sizes))
}

/// Reads the generated pair spacing, in thousandths of an em, from the pinned manifest.
fn shipped_spacing(profile: assets::carriers::FontFace) -> Result<Vec<(char, char, i32)>, String> {
    #[derive(serde::Deserialize)]
    struct Spacing {
        #[serde(default)]
        kerning_em_1000: Vec<(char, char, i32)>,
    }
    serde_json::from_slice::<Spacing>(profile.manifest)
        .map(|spacing| spacing.kerning_em_1000)
        .map_err(|e| e.to_string())
}

/// Applies pair spacing in the fixed-point pen units of a raster with an `em`-pixel em.
fn with_spacing(
    spacing: &[(char, char, i32)],
    em: u32,
    font: RuntimeFontCatalog,
) -> Result<RuntimeFontCatalog, String> {
    let pairs = spacing
        .iter()
        .filter_map(|&(left, right, value)| {
            if font.glyph(left).is_none() || font.glyph(right).is_none() {
                return None;
            }
            let scaled = i64::from(value) * i64::from(em) * 64;
            let fixed = scaled.signum() * ((scaled.abs() + 500) / 1000);
            Some(((left, right), fixed as i32))
        })
        .collect();
    font.with_kerning(pairs).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests;
