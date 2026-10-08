use std::{
    fs::{self, File},
    io::{self, Read},
    path::Path,
};

use assets::{
    AssetError, AtmosphereRole, BlobProvenance, EntityAssetSource, EntityAssetSymbol,
    ItemVisualDefinitionRoute, MATERIAL_FLAG_ALPHA_CUTOUT, encode_atmosphere_blob, encode_blob,
    encode_entity_blob, read_biome_registry, write_blob_atomic,
};
use clap::Parser;
use pack_compiler::{
    AnimationInventory, AtmosphereCompileOptions, CompileReferenceOutcome, FontCompileError,
    compile_atmosphere_assets_with_options, compile_entity_assets_with_report, compile_fonts,
    compile_pack_with_material_keys, compile_vanilla_entity_refs, inspect_animation_inventory,
};
use serde::Serialize;
use sha2::{Digest, Sha256};

#[path = "assetc/actor_command.rs"]
mod actor_command;
#[path = "assetc/audio_bank_command.rs"]
mod audio_bank_command;
#[path = "assetc/audio_command.rs"]
mod audio_command;
#[path = "assetc/audio_pcm_command.rs"]
mod audio_pcm_command;
#[path = "assetc/block_entity_command.rs"]
mod block_entity_command;
#[path = "assetc/cli.rs"]
mod cli;
#[path = "assetc/command_outputs.rs"]
mod command_outputs;
#[path = "assetc/equipment_command.rs"]
mod equipment_command;
#[path = "assetc/font_command.rs"]
mod font_command;
#[path = "assetc/hud_command.rs"]
mod hud_command;
#[path = "assetc/icon_command.rs"]
mod icon_command;
#[path = "assetc/lang_command.rs"]
mod lang_command;
#[path = "assetc/output_bundle.rs"]
mod output_bundle;
use output_bundle::write_output_bundle;
#[path = "assetc/output_validation.rs"]
mod output_validation;
#[path = "assetc/particle_command.rs"]
mod particle_command;
#[path = "assetc/prepare.rs"]
mod prepare;
#[path = "assetc/prepare_plan.rs"]
mod prepare_plan;
#[path = "assetc/registry_version.rs"]
mod registry_version;
#[path = "assetc/ui_command.rs"]
mod ui_command;
#[path = "assetc/vanilla_pack_command.rs"]
mod vanilla_pack_command;

use audio_bank_command::compile_audio_bank_command;
use audio_command::compile_audio_assets_command;
use audio_pcm_command::compile_audio_pcm_command;
use cli::{Cli, Command};
use equipment_command::compile_equipment_assets_command;
use hud_command::compile_hud_assets_command;
use icon_command::compile_icon_assets_command;
use lang_command::{compile_lang_assets_command, compile_languages_command};
use output_validation::validate_output_bundle;
use particle_command::compile_particle_assets_command;
use ui_command::compile_ui_assets_command;

const MAX_REGISTRY_FILE_BYTES: usize = 128 * 1024 * 1024;
const MAX_SOURCE_MANIFEST_BYTES: usize = 1024 * 1024;

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
struct EntityAssetsReport<'a> {
    schema: u32,
    source: serde_json::Value,
    source_manifest_sha256: Box<str>,
    blob_sha256: Box<str>,
    counts: EntityAssetCounts,
    sources: &'a [EntityAssetSource],
    symbols: &'a [EntityAssetSymbol],
    reference_outcomes: &'a [CompileReferenceOutcome<u32>],
}

#[derive(Serialize)]
struct EntityAssetCounts {
    sources: usize,
    symbols: usize,
    dependencies: usize,
    geometries: usize,
    bones: usize,
    cubes: usize,
    animation_clips: usize,
    animation_channels: usize,
    animation_keyframes: usize,
    molang_symbols: usize,
    molang_expressions: usize,
    molang_ops: usize,
    molang_collections: usize,
    molang_collection_items: usize,
    controllers: usize,
    controller_states: usize,
    controller_animations: usize,
    controller_transitions: usize,
    rig_bindings: usize,
    rig_geometry_candidates: usize,
    rig_animations: usize,
    rig_controllers: usize,
    rig_geometry_selections: usize,
    item_visuals: usize,
    item_visual_aliases: usize,
    item_sprite_routes: usize,
    item_block_routes: usize,
    item_empty_hand_routes: usize,
    item_missing_routes: usize,
    block_visuals: usize,
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
    run(Cli::parse().command)
}

/// Dispatches a parsed command after its inputs and destinations are checked.
fn run(command: Command) -> Result<(), Box<dyn std::error::Error>> {
    command_outputs::validate_command_outputs(&command)?;
    match command {
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
        Command::EquipmentAssets {
            pack,
            source_manifest,
            out,
            report,
            behavior_pack,
        } => {
            compile_equipment_assets_command(
                &pack,
                &source_manifest,
                &out,
                &report,
                behavior_pack.as_deref(),
            )?;
        }
        Command::FontAssets {
            pack,
            font,
            glyph_pack,
            compact_pages,
            source_manifest,
            out,
            report,
        } => {
            compile_font_assets_command(
                pack.as_deref(),
                font.as_deref(),
                font_command::PostprocessOptions {
                    glyph_pack: glyph_pack.as_deref(),
                    compact_pages,
                },
                &source_manifest,
                &out,
                &report,
            )?;
        }
        Command::HudAssets {
            pack,
            source_manifest,
            out,
            report,
        } => {
            compile_hud_assets_command(&pack, &source_manifest, &out, &report)?;
        }
        Command::HudExtrasAssets { pack, out } => {
            pack_compiler::compile_hud_extras_to_file(&pack, &out)?;
            println!("compiled HUD extras to {}", out.display());
        }
        Command::WeatherAssets { pack, out } => {
            pack_compiler::compile_weather_textures_to_file(&pack, &out)?;
            println!("compiled weather textures to {}", out.display());
        }
        Command::ActorAssets {
            pack,
            source_manifest,
            out,
            report,
        } => {
            actor_command::compile_actor_assets_command(&pack, &source_manifest, &out, &report)?;
        }
        Command::BlockEntityAssets {
            pack,
            source_manifest,
            out,
            report,
        } => {
            block_entity_command::compile_block_entity_assets_command(
                &pack,
                &source_manifest,
                &out,
                &report,
            )?;
        }
        Command::UiAssets {
            pack,
            source_manifest,
            out,
            report,
        } => {
            compile_ui_assets_command(&pack, &source_manifest, &out, &report)?;
        }
        Command::ParticleAssets {
            pack,
            source_manifest,
            out,
            report,
        } => {
            compile_particle_assets_command(&pack, &source_manifest, &out, &report)?;
        }
        Command::IconAssets {
            pack,
            source_manifest,
            block_assets,
            out,
            report,
        } => {
            compile_icon_assets_command(
                &pack,
                &source_manifest,
                block_assets.as_deref(),
                &out,
                &report,
            )?;
        }
        Command::LangAssets {
            pack,
            source_manifest,
            out,
            report,
        } => {
            compile_lang_assets_command(&pack, &source_manifest, &out, &report)?;
        }
        Command::LanguageAssets {
            pack,
            source_manifest,
            out_dir,
        } => {
            compile_languages_command(&pack, &source_manifest, &out_dir)?;
        }
        Command::AudioAssets {
            pack,
            source_manifest,
            out,
            report,
        } => {
            compile_audio_assets_command(&pack, &source_manifest, &out, &report)?;
        }
        Command::AudioBank { pack, out, report } => {
            compile_audio_bank_command(&pack, &out, &report)?;
        }
        Command::AudioPcmAssets {
            pack,
            catalog,
            source_manifest,
            out,
            report,
        } => {
            compile_audio_pcm_command(&pack, &catalog, &source_manifest, &out, &report)?;
        }
        Command::OutlineFontAssets {
            font,
            fallback_font,
            primary_only,
            glyph_pack,
            compact_pages,
            source_manifest,
            out,
            report,
        } => {
            compile_outline_font_assets_command(
                &font,
                fallback_font.as_deref(),
                primary_only,
                font_command::PostprocessOptions {
                    glyph_pack: glyph_pack.as_deref(),
                    compact_pages,
                },
                &source_manifest,
                &out,
                &report,
            )?;
        }
        Command::Compile {
            pack,
            source_manifest,
            registry,
            light_registry,
            biome_registry,
            out,
        } => {
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
            let registry_bytes = read_bounded(&registry)?;
            let (records, block_registry_protocol) =
                registry_version::read_block_registry_input(&registry, &registry_bytes)?;
            let light_registry_bytes = read_bounded(&light_registry)?;
            let light_properties = registry_version::read_light_registry_input(
                &light_registry,
                &light_registry_bytes,
                &registry,
                &registry_bytes,
                block_registry_protocol,
                records.len(),
            )?;
            let biome_registry_bytes = read_bounded(&biome_registry)?;
            let biome_records = read_biome_registry(&biome_registry_bytes)?;
            let behavior_pack = pack
                .parent()
                .ok_or("resource-pack path has no parent for behavior_pack")?
                .join("behavior_pack");
            let (mut compiled, material_keys) = compile_pack_with_material_keys(
                &pack,
                &behavior_pack,
                &records,
                &biome_records,
                &light_properties,
                block_registry_protocol,
            )?;
            compiled.provenance = BlobProvenance {
                source_manifest_sha256: assets::canonical_source_manifest_sha256(&manifest_bytes),
                block_registry_sha256: Sha256::digest(&registry_bytes).into(),
                light_registry_sha256: Sha256::digest(&light_registry_bytes).into(),
                biome_registry_sha256: Sha256::digest(&biome_registry_bytes).into(),
            };
            let blob = encode_blob(&compiled)?;
            write_output_bundle(&[
                (&out, &blob),
                (
                    &command_outputs::material_keys_output(&out),
                    &material_keys.to_json(compiled.materials.len() as u32),
                ),
            ])?;
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
        Command::Prepare {
            root,
            kit,
            workspace,
            out,
            only,
            check,
            json,
            accept_eula,
            clouds_override,
        } => prepare::prepare(prepare::Options {
            root,
            kit,
            workspace,
            out,
            only,
            check,
            json,
            accept_eula,
            clouds_override,
        })?,
        Command::VanillaPack {
            source_manifest,
            accept_eula,
        } => {
            vanilla_pack_command::acquire(&source_manifest, &std::env::current_dir()?, accept_eula)?
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

fn compile_font_assets_command(
    pack: Option<&Path>,
    font: Option<&Path>,
    options: font_command::PostprocessOptions<'_>,
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
    let source_manifest_sha256 = assets::canonical_source_manifest_sha256(&manifest_bytes);
    let compiled = match (pack, font) {
        (Some(pack), None) => compile_fonts(pack)?,
        (None, Some(font)) => font_command::compile_pinned(font, &source, source_manifest_sha256)?,
        _ => return Err("font-assets takes exactly one of --pack or --font".into()),
    };
    if compiled.report.source_manifest_sha256 != source_manifest_sha256 {
        return Err(FontCompileError::SourceManifestMismatch.into());
    }
    let compiled = font_command::postprocess(compiled, options)?;
    write_compiled_font_assets(source, source_manifest_sha256, compiled, out, report, &[])
}

fn compile_outline_font_assets_command(
    font: &Path,
    fallback: Option<&Path>,
    primary_only: bool,
    options: font_command::PostprocessOptions<'_>,
    source_manifest: &Path,
    out: &Path,
    report: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    font_command::compile(
        font,
        fallback,
        primary_only,
        options,
        source_manifest,
        out,
        report,
    )
}

fn required_u32(value: &serde_json::Value, field: &str) -> Result<u32, Box<dyn std::error::Error>> {
    value
        .get(field)
        .and_then(serde_json::Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .ok_or_else(|| format!("font rasterization field '{field}' is invalid").into())
}

fn write_compiled_font_assets(
    source: serde_json::Value,
    source_manifest_sha256: [u8; 32],
    compiled: pack_compiler::CompiledFontCarrier,
    out: &Path,
    report: &Path,
    sidecars: &[(&Path, &[u8])],
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
    let mut outputs = vec![
        (out, compiled.bytes.as_ref()),
        (report, report_bytes.as_slice()),
    ];
    outputs.extend_from_slice(sidecars);
    write_output_bundle(&outputs)?;
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
        schema: 4,
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
    let refs = compile_vanilla_entity_refs(pack)?;
    write_output_bundle(&[
        (out, &blob),
        (report, &report_bytes),
        (&command_outputs::entity_refs_output(out), &refs.to_json()),
    ])?;
    println!(
        "compiled {} entity authority sources, {} symbols, {} dependencies, {} geometries, {} bones, and {} cubes to {} and {}",
        report_data.counts.sources,
        report_data.counts.symbols,
        report_data.counts.dependencies,
        report_data.counts.geometries,
        report_data.counts.bones,
        report_data.counts.cubes,
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
    write_output_bundle(&[(out, &blob), (report, &report_bytes)])?;
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
#[path = "../../build_support/lockfile.rs"]
mod lockfile;
#[cfg(test)]
#[path = "assetc/tests.rs"]
mod tests;
