use std::{
    fs::{self, File},
    io::{self, Read},
    path::{Component, Path, PathBuf},
};

use asset_compiler::{
    AnimationInventory, AtmosphereCompileOptions, CompileReferenceOutcome, FontCompileError,
    OutlineFontConfig, compile_atmosphere_assets_with_options, compile_entity_assets_with_report,
    compile_fonts, compile_hud_assets, compile_outline_font, compile_pack_with_biomes,
    inspect_animation_inventory,
};
use assets::{
    AssetError, AtmosphereRole, ItemVisualDefinitionRoute, MATERIAL_FLAG_ALPHA_CUTOUT,
    MAX_FONT_SOURCE_BYTES, encode_atmosphere_blob, encode_blob, encode_entity_blob,
    read_biome_registry, read_light_registry, read_registry, write_blob_atomic,
};
use clap::{Parser, Subcommand};
use serde::Serialize;
use sha2::{Digest, Sha256};

#[path = "assetc/entity_report.rs"]
mod entity_report;
#[path = "assetc/hud_command.rs"]
mod hud_command;

use entity_report::{EntityAssetCounts, EntityAssetsReport};
use hud_command::{HudAssetCounts, HudAssetsReport};

const MAX_REGISTRY_FILE_BYTES: usize = 128 * 1024 * 1024;
const MAX_SOURCE_MANIFEST_BYTES: usize = 1024 * 1024;
#[derive(Debug, Parser)]
#[command(
    about = "Compile verified local Bedrock resource-pack assets",
    after_help = "Compile inputs:\n  assetc compile --pack <RESOURCE_PACK> --registry <BLOCK_REGISTRY_BIN> --light-registry <LIGHT_REGISTRY_BIN> --biome-registry <BIOME_REGISTRY_BIN> --out <IGNORED_DIR>/vanilla-v1001.mcbea\n\nAtmosphere inputs:\n  assetc atmosphere --pack <RESOURCE_PACK> --source-manifest <VANILLA_SOURCE_JSON> --out <IGNORED_DIR>/vanilla-v1.mcbeatm --report <IGNORED_DIR>/atmosphere-assets.json\n\nEntity catalog and geometry payloads:\n  assetc entity-assets --pack <RESOURCE_PACK> --source-manifest <VANILLA_SOURCE_JSON> --out <IGNORED_DIR>/vanilla-v1.mcbeent --report <IGNORED_DIR>/entity-assets.json\n\nBitmap font payloads:\n  assetc font-assets --pack <RESOURCE_PACK> --source-manifest <VANILLA_SOURCE_JSON> --out <IGNORED_DIR>/vanilla-v1.mcbefont --report <IGNORED_DIR>/font-assets.json\n\nPinned official Mojang sample HUD sprites:\n  assetc hud-assets --pack <RESOURCE_PACK> --source-manifest assets/hud-source-v1001.json --out <IGNORED_DIR>/vanilla-v1.mcbehud --report <IGNORED_DIR>/hud-assets.json\n\nAnimation inventory:\n  assetc animation-inventory --pack <RESOURCE_PACK> --source-manifest <VANILLA_SOURCE_JSON> --max-layers-per-page 2048 --max-pages 2 --out <IGNORED_DIR>/animation-inventory.json"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Compile the fixed vanilla sun, moon-phase, and cloud textures.
    Atmosphere {
        /// Root of the pinned vanilla resource pack.
        #[arg(long)]
        pack: PathBuf,
        /// Tracked manifest that pins the local resource-pack source.
        #[arg(long)]
        source_manifest: PathBuf,
        #[arg(long)]
        clouds_override: Option<PathBuf>,
        /// Ignored/local MCBEATM2 output path.
        #[arg(long)]
        out: PathBuf,
        /// Ignored/local deterministic JSON provenance report path.
        #[arg(long)]
        report: PathBuf,
    },
    /// Compile bounded entity geometry, animation, controller, and texture metadata.
    EntityAssets {
        /// Root of the pinned vanilla resource pack.
        #[arg(long)]
        pack: PathBuf,
        /// Tracked manifest that pins the local resource-pack source.
        #[arg(long)]
        source_manifest: PathBuf,
        /// Ignored/local MCBEENT3 output path.
        #[arg(long)]
        out: PathBuf,
        /// Ignored/local deterministic JSON provenance report path.
        #[arg(long)]
        report: PathBuf,
    },
    /// Compile bounded bitmap-font metrics and raw RGBA8 texture pages.
    FontAssets {
        /// Root of the pinned vanilla resource pack.
        #[arg(long)]
        pack: PathBuf,
        /// Tracked manifest that pins the local resource-pack source.
        #[arg(long)]
        source_manifest: PathBuf,
        /// Ignored/local MCBEFONT1 output path.
        #[arg(long)]
        out: PathBuf,
        /// Ignored/local deterministic JSON provenance report path.
        #[arg(long)]
        report: PathBuf,
    },
    HudAssets {
        #[arg(long)]
        pack: PathBuf,
        #[arg(long)]
        source_manifest: PathBuf,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        report: PathBuf,
    },
    /// Rasterize a pinned open-licensed outline font into a bounded bitmap carrier.
    OutlineFontAssets {
        /// Exact hash-verified local TTF/OTF source.
        #[arg(long)]
        font: PathBuf,
        /// Tracked manifest pinning font URL, hash, license, and raster settings.
        #[arg(long)]
        source_manifest: PathBuf,
        /// Ignored/local MCBEFONT1 output path.
        #[arg(long)]
        out: PathBuf,
        /// Ignored/local deterministic JSON provenance report path.
        #[arg(long)]
        report: PathBuf,
    },
    /// Compile a resource pack and Dragonfly registry into a runtime blob.
    Compile {
        /// Root containing blocks.json and the textures directory.
        #[arg(long)]
        pack: PathBuf,
        /// BREG1003 registry exported by tools/registrygen.
        #[arg(long)]
        registry: PathBuf,
        /// LREG1001 state light metadata bound to the exact BREG1003 input.
        #[arg(long)]
        light_registry: PathBuf,
        /// BIOREG01 registry exported by tools/registrygen.
        #[arg(long)]
        biome_registry: PathBuf,
        /// Ignored/local output path, conventionally ending in .mcbea.
        #[arg(long)]
        out: PathBuf,
    },
    /// Compile a bounded read-only animation plan and write its deterministic inventory.
    AnimationInventory {
        /// Root containing blocks.json and the textures directory.
        #[arg(long)]
        pack: PathBuf,
        /// Pinned source manifest whose exact bytes identify the local pack source.
        #[arg(long)]
        source_manifest: PathBuf,
        /// Maximum physical array layers in each texture page (1..=2048).
        #[arg(long)]
        max_layers_per_page: u32,
        /// Maximum physical texture pages (1..=2).
        #[arg(long)]
        max_pages: u32,
        /// Ignored/local deterministic JSON report path.
        #[arg(long)]
        out: PathBuf,
    },
}

#[derive(Serialize)]
struct AnimationInventoryReport {
    schema: u32,
    source_manifest_sha256: Box<str>,
    canonical_pack_path: Box<str>,
    limits: AnimationInventoryLimits,
    inventory: AnimationInventory,
}

#[derive(Serialize)]
struct AnimationInventoryLimits {
    max_layers_per_page: u32,
    max_pages: u32,
}

#[derive(Serialize)]
struct AtmosphereReport {
    schema: u32,
    source: serde_json::Value,
    source_manifest_sha256: Box<str>,
    blob_sha256: Box<str>,
    textures: Box<[AtmosphereTextureReport]>,
}

#[derive(Serialize)]
struct AtmosphereTextureReport {
    role: &'static str,
    source_path: Box<str>,
    width: u32,
    height: u32,
    source_bytes: usize,
    decoded_rgba8_bytes: usize,
    source_sha256: Box<str>,
    pixels_sha256: Box<str>,
}

#[derive(Serialize)]
struct FontAssetsReport {
    schema: u32,
    source: serde_json::Value,
    source_manifest_sha256: Box<str>,
    carrier_sha256: Box<str>,
    counts: FontAssetCounts,
}

#[derive(Serialize)]
struct FontAssetCounts {
    glyphs: usize,
    pages: usize,
    source_bytes: u64,
    decoded_bytes: u64,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    match Cli::parse().command {
        Command::Atmosphere {
            pack,
            source_manifest,
            clouds_override,
            out,
            report,
        } => {
            compile_atmosphere_command(
                &pack,
                &source_manifest,
                clouds_override.as_deref(),
                &out,
                &report,
                compile_atmosphere_assets_with_options,
            )?;
        }
        Command::EntityAssets {
            pack,
            source_manifest,
            out,
            report,
        } => {
            compile_entity_assets_command(&pack, &source_manifest, &out, &report)?;
        }
        Command::FontAssets {
            pack,
            source_manifest,
            out,
            report,
        } => {
            compile_font_assets_command(&pack, &source_manifest, &out, &report)?;
        }
        Command::HudAssets {
            pack,
            source_manifest,
            out,
            report,
        } => {
            compile_hud_assets_command(&pack, &source_manifest, &out, &report)?;
        }
        Command::OutlineFontAssets {
            font,
            source_manifest,
            out,
            report,
        } => {
            compile_outline_font_assets_command(&font, &source_manifest, &out, &report)?;
        }
        Command::Compile {
            pack,
            registry,
            light_registry,
            biome_registry,
            out,
        } => {
            let registry_bytes = read_bounded(&registry)?;
            let records = read_registry(&registry_bytes)?;
            let light_registry_bytes = read_bounded(&light_registry)?;
            let light_properties =
                read_light_registry(&light_registry_bytes, &registry_bytes, records.len())?;
            let biome_registry_bytes = read_bounded(&biome_registry)?;
            let biome_records = read_biome_registry(&biome_registry_bytes)?;
            let behavior_pack = pack
                .parent()
                .ok_or("resource-pack path has no parent for behavior_pack")?
                .join("behavior_pack");
            let compiled = compile_pack_with_biomes(
                &pack,
                &behavior_pack,
                &records,
                &biome_records,
                &light_properties,
            )?;
            let blob = encode_blob(&compiled)?;
            write_blob_atomic(&out, &blob)?;
            let cutout_materials = compiled
                .materials
                .iter()
                .filter(|material| material.flags & MATERIAL_FLAG_ALPHA_CUTOUT != 0)
                .count();
            println!(
                "compiled {} visuals, {} materials ({} alpha cutout), {} texture layers, and {} biome rules to {}",
                compiled.visuals.len(),
                compiled.materials.len(),
                cutout_materials,
                compiled
                    .texture_pages
                    .iter()
                    .map(|page| page.texture.layers)
                    .sum::<u32>(),
                compiled.biomes.rules.len(),
                out.display()
            );
        }
        Command::AnimationInventory {
            pack,
            source_manifest,
            max_layers_per_page,
            max_pages,
            out,
        } => {
            let canonical_pack = fs::canonicalize(&pack).map_err(|source| AssetError::Io {
                path: pack.clone(),
                source,
            })?;
            let manifest_bytes = read_bounded_with_limit(
                &source_manifest,
                MAX_SOURCE_MANIFEST_BYTES,
                "source manifest",
            )?;
            serde_json::from_slice::<serde_json::Value>(&manifest_bytes).map_err(|source| {
                AssetError::Json {
                    path: source_manifest.clone(),
                    source,
                }
            })?;
            let source_manifest_sha256 = format!("{:x}", Sha256::digest(&manifest_bytes));
            let inventory =
                inspect_animation_inventory(&canonical_pack, max_layers_per_page, max_pages)?;
            let report = AnimationInventoryReport {
                schema: 1,
                source_manifest_sha256: source_manifest_sha256.into_boxed_str(),
                canonical_pack_path: canonical_pack
                    .to_string_lossy()
                    .into_owned()
                    .into_boxed_str(),
                limits: AnimationInventoryLimits {
                    max_layers_per_page,
                    max_pages,
                },
                inventory,
            };
            let mut bytes =
                serde_json::to_vec_pretty(&report).map_err(|source| AssetError::Json {
                    path: out.clone(),
                    source,
                })?;
            bytes.push(b'\n');
            write_blob_atomic(&out, &bytes)?;
            println!(
                "inspected {} reachable animations, {} physical frames, {} deduplicated layers across {} pages to {}",
                report.inventory.reachable_animations,
                report.inventory.physical_animation_frames,
                report.inventory.deduplicated_layers,
                report.inventory.pages,
                out.display()
            );
        }
    }
    Ok(())
}

fn compile_hud_assets_command(
    pack: &Path,
    source_manifest: &Path,
    out: &Path,
    report: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let canonical_pack = fs::canonicalize(pack).map_err(|source| AssetError::Io {
        path: pack.to_path_buf(),
        source,
    })?;
    let manifest_bytes = read_bounded_with_limit(
        source_manifest,
        MAX_SOURCE_MANIFEST_BYTES,
        "HUD source manifest",
    )?;
    let compiled = compile_hud_assets(&canonical_pack, &manifest_bytes)?;
    let report_data = HudAssetsReport {
        schema: 1,
        canonical_pack_path: canonical_pack
            .to_string_lossy()
            .into_owned()
            .into_boxed_str(),
        source_manifest_sha256: hex(&compiled.report.source_manifest_sha256).into_boxed_str(),
        carrier_sha256: hex(&compiled.report.carrier_sha256).into_boxed_str(),
        counts: HudAssetCounts {
            textures: compiled.report.textures,
            source_bytes: compiled.report.source_bytes,
            decoded_bytes: compiled.report.decoded_bytes,
        },
    };
    let mut report_bytes = serde_json::to_vec_pretty(&report_data)?;
    report_bytes.push(b'\n');
    validate_output_bundle(out, report)?;
    write_blob_atomic(out, &compiled.bytes)?;
    write_blob_atomic(report, &report_bytes)?;
    println!(
        "compiled {} pinned official Mojang sample HUD textures to {} and {}",
        report_data.counts.textures,
        out.display(),
        report.display()
    );
    Ok(())
}

fn compile_font_assets_command(
    pack: &Path,
    source_manifest: &Path,
    out: &Path,
    report: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let manifest_bytes = read_bounded_with_limit(
        source_manifest,
        MAX_SOURCE_MANIFEST_BYTES,
        "source manifest",
    )?;
    let source =
        serde_json::from_slice::<serde_json::Value>(&manifest_bytes).map_err(|source| {
            AssetError::Json {
                path: source_manifest.to_path_buf(),
                source,
            }
        })?;
    let source_manifest_sha256 = canonical_source_manifest_sha256(&manifest_bytes);
    let compiled = compile_fonts(pack)?;
    if compiled.report.source_manifest_sha256 != source_manifest_sha256 {
        return Err(FontCompileError::SourceManifestMismatch.into());
    }
    write_compiled_font_assets(source, source_manifest_sha256, compiled, out, report)
}

fn compile_outline_font_assets_command(
    font: &Path,
    source_manifest: &Path,
    out: &Path,
    report: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let manifest_bytes = read_bounded_with_limit(
        source_manifest,
        MAX_SOURCE_MANIFEST_BYTES,
        "source manifest",
    )?;
    let source =
        serde_json::from_slice::<serde_json::Value>(&manifest_bytes).map_err(|source| {
            AssetError::Json {
                path: source_manifest.to_path_buf(),
                source,
            }
        })?;
    let raster = source
        .get("rasterization")
        .ok_or("font source manifest is missing rasterization")?;
    let pixel_height = required_u32(raster, "pixel_height")?;
    let atlas_side = required_u32(raster, "atlas_side")?;
    let replacement = char::from_u32(required_u32(raster, "replacement_codepoint")?)
        .ok_or("font replacement_codepoint is not a Unicode scalar")?;
    let expected_font_size = source
        .get("font_size_bytes")
        .and_then(serde_json::Value::as_u64)
        .ok_or("font source manifest has invalid font_size_bytes")?;
    let expected_font_sha256 = source
        .get("font_sha256")
        .and_then(serde_json::Value::as_str)
        .filter(|value| value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .ok_or("font source manifest has invalid font_sha256")?;
    let source_manifest_sha256 = canonical_source_manifest_sha256(&manifest_bytes);
    let font_bytes = read_bounded_with_limit(
        font,
        usize::try_from(MAX_FONT_SOURCE_BYTES).expect("font source bound fits usize"),
        "outline font",
    )?;
    if font_bytes.len() as u64 != expected_font_size {
        return Err("outline font size does not match the source manifest".into());
    }
    if format!("{:x}", Sha256::digest(&font_bytes)) != expected_font_sha256.to_ascii_lowercase() {
        return Err("outline font SHA-256 does not match the source manifest".into());
    }
    let compiled = compile_outline_font(
        font,
        &font_bytes,
        source_manifest_sha256,
        OutlineFontConfig {
            pixel_height,
            atlas_side,
            replacement_codepoint: replacement,
        },
    )?;
    write_compiled_font_assets(source, source_manifest_sha256, compiled, out, report)
}

fn required_u32(value: &serde_json::Value, field: &str) -> Result<u32, Box<dyn std::error::Error>> {
    value
        .get(field)
        .and_then(serde_json::Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .ok_or_else(|| format!("font rasterization field '{field}' is invalid").into())
}

fn canonical_source_manifest_sha256(source: &[u8]) -> [u8; 32] {
    if !source.contains(&b'\r') {
        return Sha256::digest(source).into();
    }
    let mut canonical = Vec::with_capacity(source.len());
    let mut index = 0;
    while index < source.len() {
        match source[index] {
            b'\r' if source.get(index + 1) == Some(&b'\n') => {
                canonical.push(b'\n');
                index += 2;
            }
            b'\r' | b'\n' => return Sha256::digest(source).into(),
            byte => {
                canonical.push(byte);
                index += 1;
            }
        }
    }
    Sha256::digest(canonical).into()
}

fn write_compiled_font_assets(
    source: serde_json::Value,
    source_manifest_sha256: [u8; 32],
    compiled: asset_compiler::CompiledFontCarrier,
    out: &Path,
    report: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    if compiled.report.source_manifest_sha256 != source_manifest_sha256 {
        return Err(FontCompileError::SourceManifestMismatch.into());
    }
    let report_data = FontAssetsReport {
        schema: compiled.report.schema,
        source,
        source_manifest_sha256: hex(&compiled.report.source_manifest_sha256).into_boxed_str(),
        carrier_sha256: hex(&compiled.report.carrier_sha256).into_boxed_str(),
        counts: FontAssetCounts {
            glyphs: compiled.report.glyphs,
            pages: compiled.report.pages,
            source_bytes: compiled.report.source_bytes,
            decoded_bytes: compiled.report.decoded_bytes,
        },
    };
    let mut report_bytes =
        serde_json::to_vec_pretty(&report_data).map_err(|source| AssetError::Json {
            path: report.to_path_buf(),
            source,
        })?;
    report_bytes.push(b'\n');
    validate_output_bundle(out, report)?;
    write_blob_atomic(out, &compiled.bytes)?;
    write_blob_atomic(report, &report_bytes)?;
    println!(
        "compiled {} bitmap-font glyphs across {} pages to {} and {}",
        report_data.counts.glyphs,
        report_data.counts.pages,
        out.display(),
        report.display()
    );
    Ok(())
}

fn compile_entity_assets_command(
    pack: &Path,
    source_manifest: &Path,
    out: &Path,
    report: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let manifest_bytes = read_bounded_with_limit(
        source_manifest,
        MAX_SOURCE_MANIFEST_BYTES,
        "source manifest",
    )?;
    let source =
        serde_json::from_slice::<serde_json::Value>(&manifest_bytes).map_err(|source| {
            AssetError::Json {
                path: source_manifest.to_path_buf(),
                source,
            }
        })?;
    let mut compilation = compile_entity_assets_with_report(pack, &manifest_bytes)?;
    compilation.reference_outcomes.sort_by_key(outcome_sort_key);
    let compiled = &compilation.assets;
    let blob = encode_entity_blob(compiled)?;
    let report_data = EntityAssetsReport {
        schema: 5,
        source,
        source_manifest_sha256: hex(&compiled.source_manifest_sha256).into_boxed_str(),
        blob_sha256: format!("{:x}", Sha256::digest(&blob)).into_boxed_str(),
        counts: EntityAssetCounts {
            sources: compiled.sources.len(),
            symbols: compiled.symbols.len(),
            dependencies: compiled
                .symbols
                .iter()
                .map(|symbol| symbol.dependencies.len())
                .sum(),
            geometries: compiled.geometries.len(),
            bones: compiled
                .geometries
                .iter()
                .map(|geometry| geometry.bones.len())
                .sum(),
            cubes: compiled
                .geometries
                .iter()
                .flat_map(|geometry| geometry.bones.iter())
                .map(|bone| bone.cubes.len())
                .sum(),
            animation_clips: compiled.animation_clips.len(),
            animation_channels: compiled.animation_channels.len(),
            animation_keyframes: compiled.animation_keyframes.len(),
            molang_symbols: compiled.molang_symbols.len(),
            molang_expressions: compiled.molang_expressions.len(),
            molang_ops: compiled.molang_ops.len(),
            molang_collections: compiled.molang_collections.len(),
            molang_collection_items: compiled.molang_collection_items.len(),
            controllers: compiled.controllers.len(),
            controller_states: compiled.controller_states.len(),
            controller_animations: compiled.controller_animations.len(),
            controller_transitions: compiled.controller_transitions.len(),
            rig_bindings: compiled.rig_bindings.len(),
            rig_geometry_candidates: compiled.rig_geometries.len(),
            rig_animations: compiled.rig_animations.len(),
            rig_controllers: compiled.rig_controllers.len(),
            rig_textures: compiled.rig_textures.len(),
            rig_texture_bytes: compiled
                .rig_textures
                .iter()
                .map(|texture| texture.rgba8.len())
                .sum(),
            rig_geometry_selections: compiled
                .rig_geometries
                .iter()
                .filter(|candidate| candidate.condition.is_some())
                .count(),
            item_visuals: compiled.item_visuals.len(),
            item_visual_aliases: compiled.item_visual_aliases.len(),
            item_sprite_routes: compiled
                .item_visuals
                .iter()
                .filter(|visual| matches!(visual.route, ItemVisualDefinitionRoute::Sprite { .. }))
                .count(),
            item_block_routes: compiled
                .item_visuals
                .iter()
                .filter(|visual| {
                    matches!(visual.route, ItemVisualDefinitionRoute::BlockItem { .. })
                })
                .count(),
            item_empty_hand_routes: compiled
                .item_visuals
                .iter()
                .filter(|visual| matches!(visual.route, ItemVisualDefinitionRoute::EmptyHand))
                .count(),
            item_missing_routes: compiled
                .item_visuals
                .iter()
                .filter(|visual| matches!(visual.route, ItemVisualDefinitionRoute::Missing))
                .count(),
            block_visuals: compiled.block_visual_count as usize,
        },
        sources: &compiled.sources,
        symbols: &compiled.symbols,
        reference_outcomes: &compilation.reference_outcomes,
    };
    let mut report_bytes =
        serde_json::to_vec_pretty(&report_data).map_err(|source| AssetError::Json {
            path: report.to_path_buf(),
            source,
        })?;
    report_bytes.push(b'\n');
    validate_output_bundle(out, report)?;
    write_blob_atomic(out, &blob)?;
    write_blob_atomic(report, &report_bytes)?;
    println!(
        "compiled {} entity authority sources, {} symbols, {} dependencies, {} geometries, {} bones, {} cubes, and {} rig textures ({} bytes) to {} and {}",
        report_data.counts.sources,
        report_data.counts.symbols,
        report_data.counts.dependencies,
        report_data.counts.geometries,
        report_data.counts.bones,
        report_data.counts.cubes,
        report_data.counts.rig_textures,
        report_data.counts.rig_texture_bytes,
        out.display(),
        report.display()
    );
    Ok(())
}

fn outcome_sort_key(outcome: &CompileReferenceOutcome<u32>) -> (u32, u32, u8, u8) {
    match outcome {
        CompileReferenceOutcome::Resolved(index) => (u32::MAX, *index, 0, 0),
        CompileReferenceOutcome::OptionalStaticFallback {
            source,
            symbol,
            reason,
        } => (*source, *symbol, 1, *reason as u8),
        CompileReferenceOutcome::RequiredRigRejected {
            source,
            symbol,
            reason,
        } => (*source, *symbol, 2, *reason as u8),
    }
}

fn compile_atmosphere_command<F>(
    pack: &Path,
    source_manifest: &Path,
    clouds_override: Option<&Path>,
    out: &Path,
    report: &Path,
    compile: F,
) -> Result<(), Box<dyn std::error::Error>>
where
    F: for<'a> FnOnce(
        &Path,
        &[u8],
        AtmosphereCompileOptions<'a>,
    ) -> Result<assets::CompiledAtmosphereAssets, AssetError>,
{
    let manifest_bytes = read_bounded_with_limit(
        source_manifest,
        MAX_SOURCE_MANIFEST_BYTES,
        "source manifest",
    )?;
    let source =
        serde_json::from_slice::<serde_json::Value>(&manifest_bytes).map_err(|source| {
            AssetError::Json {
                path: source_manifest.to_path_buf(),
                source,
            }
        })?;
    let compiled = compile(
        pack,
        &manifest_bytes,
        AtmosphereCompileOptions { clouds_override },
    )?;
    let blob = encode_atmosphere_blob(&compiled)?;
    let report_data = build_atmosphere_report(source, &compiled, &blob);
    let mut report_bytes =
        serde_json::to_vec_pretty(&report_data).map_err(|source| AssetError::Json {
            path: report.to_path_buf(),
            source,
        })?;
    report_bytes.push(b'\n');
    validate_output_bundle(out, report)?;
    write_blob_atomic(out, &blob)?;
    write_blob_atomic(report, &report_bytes)?;
    println!(
        "compiled {} pinned atmosphere textures to {} and {}",
        report_data.textures.len(),
        out.display(),
        report.display()
    );
    Ok(())
}

fn build_atmosphere_report(
    source: serde_json::Value,
    compiled: &assets::CompiledAtmosphereAssets,
    blob: &[u8],
) -> AtmosphereReport {
    let textures = compiled
        .textures
        .iter()
        .map(|texture| AtmosphereTextureReport {
            role: match texture.role {
                AtmosphereRole::Sun => "sun",
                AtmosphereRole::MoonPhases => "moon_phases",
                AtmosphereRole::Clouds => "clouds",
            },
            source_path: texture.source_path.clone(),
            width: texture.width,
            height: texture.height,
            source_bytes: texture.source_bytes as usize,
            decoded_rgba8_bytes: texture.rgba8.len(),
            source_sha256: hex(&texture.source_sha256).into_boxed_str(),
            pixels_sha256: hex(&texture.pixels_sha256).into_boxed_str(),
        })
        .collect::<Vec<_>>()
        .into_boxed_slice();
    AtmosphereReport {
        schema: 1,
        source,
        source_manifest_sha256: hex(&compiled.source_manifest_sha256).into_boxed_str(),
        blob_sha256: format!("{:x}", Sha256::digest(blob)).into_boxed_str(),
        textures,
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn validate_output_bundle(blob: &Path, report: &Path) -> Result<(), AssetError> {
    let normalized_blob = normalized_absolute(blob)?;
    let normalized_report = normalized_absolute(report)?;
    if paths_alias(&normalized_blob, &normalized_report)
        || paths_alias(
            &canonicalized_location(&normalized_blob)?,
            &canonicalized_location(&normalized_report)?,
        )
    {
        return Err(output_alias_error(blob));
    }

    let mut both_exist = true;
    for path in [blob, report] {
        match fs::metadata(path) {
            Ok(metadata) if !metadata.is_file() => {
                return Err(AssetError::Io {
                    path: path.to_path_buf(),
                    source: io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "atmosphere output destination is not a regular file",
                    ),
                });
            }
            Ok(_) => {}
            Err(source) if source.kind() == io::ErrorKind::NotFound => both_exist = false,
            Err(source) => {
                return Err(AssetError::Io {
                    path: path.to_path_buf(),
                    source,
                });
            }
        }
    }
    if both_exist {
        match same_file::is_same_file(blob, report) {
            Ok(true) => return Err(output_alias_error(blob)),
            Ok(false) => {}
            Err(source) => {
                return Err(AssetError::Io {
                    path: blob.to_path_buf(),
                    source,
                });
            }
        }
    }
    Ok(())
}

fn output_alias_error(path: &Path) -> AssetError {
    AssetError::Io {
        path: path.to_path_buf(),
        source: io::Error::new(
            io::ErrorKind::InvalidInput,
            "atmosphere blob and report paths must identify distinct files",
        ),
    }
}

fn normalized_absolute(path: &Path) -> Result<PathBuf, AssetError> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|source| AssetError::Io {
                path: path.to_path_buf(),
                source,
            })?
            .join(path)
    };
    let mut normalized = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() {
                    return Err(AssetError::Io {
                        path: path.to_path_buf(),
                        source: io::Error::new(
                            io::ErrorKind::InvalidInput,
                            "output path escapes its filesystem root",
                        ),
                    });
                }
            }
            Component::Prefix(_) | Component::RootDir | Component::Normal(_) => {
                normalized.push(component.as_os_str());
            }
        }
    }
    Ok(normalized)
}

fn canonicalized_location(path: &Path) -> Result<PathBuf, AssetError> {
    let mut ancestor = path;
    let mut suffix = Vec::new();
    loop {
        match fs::canonicalize(ancestor) {
            Ok(mut canonical) => {
                for component in suffix.iter().rev() {
                    canonical.push(component);
                }
                return Ok(canonical);
            }
            Err(source) if source.kind() == io::ErrorKind::NotFound => {
                let Some(file_name) = ancestor.file_name() else {
                    return Err(AssetError::Io {
                        path: path.to_path_buf(),
                        source,
                    });
                };
                suffix.push(file_name.to_os_string());
                let Some(parent) = ancestor.parent() else {
                    return Err(AssetError::Io {
                        path: path.to_path_buf(),
                        source,
                    });
                };
                ancestor = parent;
            }
            Err(source) => {
                return Err(AssetError::Io {
                    path: path.to_path_buf(),
                    source,
                });
            }
        }
    }
}

fn paths_alias(left: &Path, right: &Path) -> bool {
    left.to_string_lossy()
        .to_lowercase()
        .eq(&right.to_string_lossy().to_lowercase())
}

fn read_bounded(path: &Path) -> Result<Vec<u8>, AssetError> {
    read_bounded_with_limit(path, MAX_REGISTRY_FILE_BYTES, "registry")
}

fn read_bounded_with_limit(
    path: &Path,
    max_bytes: usize,
    label: &'static str,
) -> Result<Vec<u8>, AssetError> {
    let file = File::open(path).map_err(|source| AssetError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let mut bytes = Vec::new();
    file.take((max_bytes + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|source| AssetError::Io {
            path: path.to_path_buf(),
            source,
        })?;
    if bytes.len() > max_bytes {
        return Err(AssetError::Io {
            path: path.to_path_buf(),
            source: io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{label} exceeds the {max_bytes}-byte compiler input limit"),
            ),
        });
    }
    Ok(bytes)
}

#[cfg(test)]
#[path = "assetc/tests.rs"]
mod tests;
