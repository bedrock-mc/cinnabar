//! Installed OreUI outline faces remain runtime assets, separate from the HUD carrier.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::Read,
    path::{Path, PathBuf},
    sync::Arc,
};

use assets::{FontRendering, RuntimeFontCatalog};
use client_ui::ui_runtime::{oreui_assets::bundle_dir, oreui_fonts::OreUiFont};
#[cfg(test)]
use pack_compiler::compile_native_outline_font;
use pack_compiler::{
    NATIVE_SDF_EM_PIXELS, NATIVE_SDF_MIN_PIXELS, OutlineFontConfig,
    compile_native_outline_font_sizes,
};
use sha2::{Digest, Sha256};

const MAX_SOURCE_BYTES: usize = 2 * 1024 * 1024;
const RASTER_EM: u32 = 32;
const ATLAS_SIDE: u32 = 1024;
mod fallback;

fn profile(face: OreUiFont) -> (FontRendering, u32) {
    match face {
        OreUiFont::SevenPixel | OreUiFont::FivePixel => (FontRendering::NativeCoverage, RASTER_EM),
        _ => (FontRendering::NativeSdf, NATIVE_SDF_EM_PIXELS),
    }
}

pub(crate) fn install(
    base: Arc<RuntimeFontCatalog>,
    resource_root: &Path,
) -> Arc<RuntimeFontCatalog> {
    let Some(bundle) = bundle_dir() else {
        return base;
    };
    let mut combined = (*base).clone().with_coverage_pages();
    for face in OreUiFont::ALL {
        match load(&bundle, face).and_then(|(font, sizes)| {
            combined
                .with_named_font_sizes(face.name(), &font, &sizes)
                .map_err(|error| error.to_string())
        }) {
            Ok(font) => combined = font,
            Err(reason) => eprintln!("OreUI font {} unavailable ({reason})", face.name()),
        }
    }
    match fallback::install(combined.clone(), &bundle, resource_root) {
        Ok(font) => Arc::new(font),
        Err(reason) => {
            eprintln!("OreUI Unicode fallback unavailable ({reason})");
            Arc::new(combined)
        }
    }
}

fn source_path(bundle: &Path, face: OreUiFont) -> Result<PathBuf, String> {
    let directory = bundle.join("fonts");
    std::fs::read_dir(&directory)
        .map_err(|error| format!("{}: {error}", directory.display()))?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .and_then(|name| name.strip_prefix(face.source_prefix()))
                .and_then(|name| name.strip_suffix(face.source_extension()))
                .is_some_and(|hash| {
                    !hash.is_empty() && hash.bytes().all(|byte| byte.is_ascii_hexdigit())
                })
        })
        .ok_or_else(|| {
            format!(
                "{} has no {} outline face",
                directory.display(),
                face.name()
            )
        })
}

fn raster_sizes(face: OreUiFont) -> Vec<u32> {
    let mut sizes: BTreeSet<_> = (1..NATIVE_SDF_MIN_PIXELS).collect();
    if let Some(pixels) = face.raster_gui_pixels() {
        let maximum_scale = ui::gui_scale([u32::MAX; 2], None);
        sizes.extend((1..=maximum_scale).map(|scale| scale * pixels));
    }
    sizes.into_iter().collect()
}

fn load(
    bundle: &Path,
    face: OreUiFont,
) -> Result<(RuntimeFontCatalog, BTreeMap<u32, RuntimeFontCatalog>), String> {
    let path = source_path(bundle, face)?;
    let bytes = read_source(&path, MAX_SOURCE_BYTES)?;
    let identity = Sha256::digest(&bytes).into();
    let (rendering, pixel_height) = profile(face);
    compile_native_outline_font_sizes(
        &path,
        &bytes,
        identity,
        OutlineFontConfig {
            pixel_height,
            atlas_side: ATLAS_SIDE,
            ..OutlineFontConfig::default()
        },
        rendering,
        &raster_sizes(face),
    )
    .map_err(|error| error.to_string())
}

fn read_source(path: &Path, maximum: usize) -> Result<Vec<u8>, String> {
    let file = File::open(path).map_err(|error| error.to_string())?;
    let mut bytes = Vec::new();
    file.take((maximum + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() > maximum {
        return Err("font source exceeds its byte bound".into());
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installed_oreui_resolves_language_names_and_server_small_caps() {
        let Some(bundle) = bundle_dir() else {
            eprintln!(
                "skipping installed_oreui_resolves_language_names_and_server_small_caps: installed OreUI font fixture missing"
            );
            return;
        };
        if source_path(&bundle, OreUiFont::Seven).is_err() {
            eprintln!(
                "skipping installed_oreui_resolves_language_names_and_server_small_caps: Seven fixture missing"
            );
            return;
        }
        let resource_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join(".local");
        if launcher::menu::settings_options::SettingsOptions::language_choices(&resource_root)
            .is_empty()
        {
            eprintln!(
                "skipping installed_oreui_resolves_language_names_and_server_small_caps: installed language-name fixture missing"
            );
            return;
        }
        let combined = install(
            client_ui::test_support::pack_harness::font(),
            &resource_root,
        );
        let world = resource_root.join("assets/compiled/fixture");
        if super::super::hud_asset_path(&world).exists()
            && super::super::icon_asset_path(&world).exists()
            && let Some(carrier) = client_ui::test_support::pack_harness::carrier()
        {
            let hud = super::super::require_hud_assets(&world).unwrap();
            let icons = super::super::require_icon_assets(
                &world,
                super::super::vanilla_source_manifest_json(),
            )
            .unwrap();
            let mut presentation =
                client_ui::ui_runtime::presentation::UiPresentationRuntime::with_hud_and_icons(
                    Arc::clone(&combined),
                    hud.into_runtime(),
                    icons.into_runtime(),
                )
                .unwrap();
            presentation.enable_json_ui(carrier).unwrap();
            let images = client_ui::ui_runtime::oreui_assets::load_optional_oreui_images().unwrap();
            presentation.enable_oreui_originals(images).unwrap();
        } else {
            eprintln!(
                "skipping installed native-art texture admission: HUD, icon or JSON-UI carrier fixture missing"
            );
        }
        let font = combined.font_named(OreUiFont::Seven.name());
        let text = "日本語 한국어 中文(简体) 中文(繁體) ʙᴇᴅᴡᴀʀѕ ★";
        let mut cache = ui::TextLayoutCache::new(4, 65536);
        let layout = cache
            .layout(ui::TextLayoutRequest {
                text,
                style: ui::TextStyle::default(),
                width_64: 655360,
                line_height_64: 64 * 64,
                baseline_64: 52 * 64,
                scale: ui::UiScale::default(),
                font,
                wrap: ui::TextWrap::default(),
            })
            .unwrap();
        for glyph in layout.glyphs() {
            assert_eq!(
                glyph.resolved_codepoint, glyph.codepoint,
                "missing {}",
                glyph.codepoint
            );
        }
        assert!(
            font.glyph_source('ไ').is_none(),
            "uncached glyph queues off-thread work"
        );
        let requests = combined.glyph_requests().unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        let ready = loop {
            if let Some(ready) = requests.take_ready() {
                break ready;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "fallback worker did not publish"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        };
        assert!(
            ready.glyph('ไ').is_some(),
            "server text outside the warm language set resolves too"
        );
    }

    #[test]
    fn installed_oreui_faces_keep_native_metrics_and_separate_hud_glyphs() {
        let Some(bundle) = bundle_dir() else {
            eprintln!(
                "skipping installed_oreui_faces_keep_native_metrics_and_separate_hud_glyphs: fixture unavailable; install a native OreUI bundle or set CINNABAR_OREUI_LOCAL_ASSETS"
            );
            return;
        };
        for face in OreUiFont::ALL {
            if let Err(reason) = source_path(&bundle, face) {
                eprintln!(
                    "skipping installed_oreui_faces_keep_native_metrics_and_separate_hud_glyphs: missing {} fixture ({reason})",
                    face.name()
                );
                return;
            }
        }
        let base = client_ui::test_support::fixture_font();
        let combined = install(Arc::clone(&base), Path::new(".local"));
        assert_eq!(combined.glyph('0'), base.glyph('0'));
        for face in OreUiFont::ALL {
            let native = combined.font_named(face.name());
            assert_ne!(
                native.identity(),
                combined.identity(),
                "{} must be installed",
                face.name()
            );
            let (rendering, pixel_height) = profile(face);
            assert_eq!(native.rendering(), rendering);
            assert_eq!(
                native.linear_sampling(),
                rendering == FontRendering::NativeSdf
            );
            assert_eq!(native.line_metrics().unwrap().em_64, pixel_height * 64);
            let glyph = native.glyph('A').unwrap();
            assert!(
                glyph.bearing[1] < 0,
                "native ascenders rise above the baseline in down-Y coordinates"
            );
            assert!(usize::from(glyph.page) >= base.pages().len());
            let page = &combined.pages()[usize::from(glyph.page)];
            assert!(u32::from(glyph.uv[2]) <= page.width);
            assert!(u32::from(glyph.uv[3]) <= page.height);
            assert!(
                (0..(page.width * page.height) as usize).all(|index| page
                    .pixels
                    .texel(index)
                    .unwrap()[..3]
                    == [255; 3]),
                "filtered glyph edges retain white RGB in transparent gutters"
            );
            if matches!(
                face,
                OreUiFont::Seven | OreUiFont::Ten | OreUiFont::Five | OreUiFont::FiveBold
            ) {
                assert_ne!(
                    glyph.advance_64 % 64,
                    0,
                    "the outline's fractional advance survives rasterization"
                );
            }
        }
        let seven = combined.font_named(OreUiFont::Seven.name());
        let ten = combined.font_named(OreUiFont::Ten.name());
        assert_ne!(seven.line_metrics(), ten.line_metrics());
        assert_ne!(seven.glyph('A').unwrap().page, ten.glyph('A').unwrap().page);
        let path = source_path(&bundle, OreUiFont::Seven).unwrap();
        let source = std::fs::read(&path).unwrap();
        let coverage = compile_native_outline_font(
            &path,
            &source,
            Sha256::digest(&source).into(),
            OutlineFontConfig {
                pixel_height: NATIVE_SDF_EM_PIXELS,
                atlas_side: ATLAS_SIDE,
                ..OutlineFontConfig::default()
            },
            FontRendering::NativeCoverage,
        )
        .unwrap();
        let sdf_glyph = seven.glyph('H').unwrap();
        let raster_glyph = coverage.glyph('H').unwrap();
        assert_eq!(sdf_glyph.advance_64, raster_glyph.advance_64);
        assert_eq!(
            sdf_glyph.uv[2] - sdf_glyph.uv[0],
            raster_glyph.uv[2] - raster_glyph.uv[0] + 8
        );
        assert_eq!(
            sdf_glyph.uv[3] - sdf_glyph.uv[1],
            raster_glyph.uv[3] - raster_glyph.uv[1] + 8
        );
        assert_eq!(
            sdf_glyph.bearing,
            [raster_glyph.bearing[0] - 4, raster_glyph.bearing[1] - 4]
        );
        assert!(matches!(
            seven.pages()[usize::from(sdf_glyph.page)].pixels,
            assets::FontPixels::Coverage(_)
        ));
        assert_eq!(
            seven.kerning_64('r', 'a'),
            -(NATIVE_SDF_EM_PIXELS as f64 * 64.0 / 10.0).round() as i32
        );
        assert_eq!(
            seven.kerning_64('F', 'a'),
            -(NATIVE_SDF_EM_PIXELS as f64 * 64.0 / 5.0).round() as i32
        );
        for word in ["General", "Fancy"] {
            let characters: Vec<_> = word.chars().collect();
            let mut cache = ui::TextLayoutCache::new(8, 65536);
            let shaped = cache
                .layout(ui::TextLayoutRequest {
                    text: word,
                    style: ui::TextStyle::default(),
                    width_64: 65536,
                    line_height_64: NATIVE_SDF_EM_PIXELS * 2 * 64,
                    baseline_64: seven.line_metrics().unwrap().ascent_64,
                    scale: ui::UiScale::default(),
                    font: seven,
                    wrap: ui::TextWrap {
                        letter_spacing_64: 26,
                        ..ui::TextWrap::default()
                    },
                })
                .unwrap();
            let advances: i32 = characters
                .iter()
                .map(|&ch| i32::from(seven.glyph(ch).unwrap().advance_64) + 26)
                .sum();
            let adjustment: i32 = characters
                .windows(2)
                .map(|pair| seven.kerning_64(pair[0], pair[1]))
                .sum();
            assert!(adjustment < 0);
            let pen = |index: usize| {
                shaped.glyphs()[index].bounds_64[0]
                    - i32::from(seven.glyph(characters[index]).unwrap().bearing[0]) * 64
            };
            let last = characters.len() - 1;
            assert_eq!(
                pen(last) - pen(0)
                    + i32::from(seven.glyph(characters[last]).unwrap().advance_64)
                    + 26,
                advances + adjustment,
                "native glyph positions retain the source pair advances"
            );
        }
    }

    #[test]
    fn installed_small_rasters_use_exact_pixel_payloads_and_preserve_sdf_boundary() {
        let Some(bundle) = bundle_dir() else {
            eprintln!(
                "skipping installed_small_rasters_use_exact_pixel_payloads_and_preserve_sdf_boundary: native OreUI font fixture unavailable"
            );
            return;
        };
        let face = OreUiFont::Seven;
        if let Err(reason) = source_path(&bundle, face) {
            eprintln!(
                "skipping installed_small_rasters_use_exact_pixel_payloads_and_preserve_sdf_boundary: {} fixture unavailable ({reason})",
                face.name()
            );
            return;
        }
        let base = client_ui::test_support::fixture_font();
        let (native, sizes) = load(&bundle, face).unwrap();
        let combined = base
            .with_named_font_sizes(face.name(), &native, &sizes)
            .unwrap();
        for pixels in 1..NATIVE_SDF_MIN_PIXELS {
            let selected = combined.font_named_at_size(face.name(), pixels as f32 + 0.75);
            assert_eq!(selected.rendering(), FontRendering::NativeCoverage);
            assert_eq!(selected.line_metrics().unwrap().em_64, pixels * 64);
            assert!(!selected.linear_sampling());
            let glyph = selected.glyph('H').unwrap();
            assert!(usize::from(glyph.page) >= base.pages().len());
            assert!(glyph.uv[3] - glyph.uv[1] <= pixels as u16 + 1);
            assert_ne!(selected.identity(), native.identity());
        }
        let threshold = combined.font_named_at_size(face.name(), NATIVE_SDF_MIN_PIXELS as f32);
        assert_eq!(threshold.rendering(), FontRendering::NativeSdf);
        assert_eq!(
            threshold.line_metrics().unwrap().em_64,
            NATIVE_SDF_EM_PIXELS * 64
        );
        assert_eq!(combined.glyph('H'), base.glyph('H'));
        let storage: usize = combined
            .pages()
            .iter()
            .map(|page| page.pixels.bytes().len())
            .sum();
        assert!(
            storage < 4 * 1024 * 1024,
            "one SDF family and its small rasters stay bounded"
        );
    }
}
