//! `assetc icon-assets`: compiles the pinned item-icon carrier and its JSON
//! report. Split from the CLI root to honor the production line budget.

use std::{fs, path::Path};

use assets::AssetError;
use pack_compiler::{compile_icon_assets, compile_icon_assets_with_blocks};
use serde::Serialize;
use sha2::{Digest, Sha256};

use super::{
    MAX_SOURCE_MANIFEST_BYTES, hex, read_bounded_with_limit, validate_output_bundle,
    write_output_bundle,
};

#[derive(Serialize)]
pub(super) struct IconAssetsReport {
    pub(super) schema: u32,
    pub(super) canonical_pack_path: Box<str>,
    pub(super) source_manifest_sha256: Box<str>,
    pub(super) carrier_sha256: Box<str>,
    pub(super) counts: IconAssetCounts,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) block_assets_sha256: Option<Box<str>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) block_registry_sha256: Option<Box<str>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) block_policy: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) block_shading: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) unresolved_block_items: Option<Vec<Box<str>>>,
}

#[derive(Serialize)]
pub(super) struct IconAssetCounts {
    pub(super) sprites: usize,
    pub(super) entries: usize,
    pub(super) sprite_visuals: usize,
    pub(super) model_item_visuals: usize,
    pub(super) alias_entries: usize,
    pub(super) animation_strips: usize,
    pub(super) skipped_oversized: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) block_visuals: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) flat_block_visuals: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) carried_block_sheets: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) skipped_blocks: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) block_refusals: Option<[usize; 4]>,
}

pub(super) fn compile_icon_assets_command(
    pack: &Path,
    source_manifest: &Path,
    block_assets: Option<&Path>,
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
        "icon source manifest",
    )?;
    let world_bytes = block_assets
        .map(|path| read_bounded_with_limit(path, 128 * 1024 * 1024, "block icon world carrier"))
        .transpose()?;
    let world = world_bytes
        .as_deref()
        .map(assets::RuntimeAssets::decode)
        .transpose()?;
    let compiled = match world.as_ref() {
        Some(world) => compile_icon_assets_with_blocks(&canonical_pack, &manifest_bytes, world)?,
        None => compile_icon_assets(&canonical_pack, &manifest_bytes)?,
    };
    let report_data = IconAssetsReport {
        schema: 1,
        canonical_pack_path: canonical_pack
            .to_string_lossy()
            .into_owned()
            .into_boxed_str(),
        source_manifest_sha256: hex(&compiled.report.source_manifest_sha256).into_boxed_str(),
        carrier_sha256: hex(&compiled.report.carrier_sha256).into_boxed_str(),
        block_assets_sha256: world_bytes
            .as_ref()
            .map(|bytes| hex(&Sha256::digest(bytes)).into_boxed_str()),
        block_registry_sha256: compiled
            .report
            .block_registry_sha256
            .map(|hash| hex(&hash).into_boxed_str()),
        block_policy: compiled.report.block_policy,
        unresolved_block_items: world
            .as_ref()
            .map(|_| compiled.report.unresolved_block_items.clone()),
        block_shading: world
            .as_ref()
            .map(|_| "provisional authored side shading; retail comparison pending"),
        counts: IconAssetCounts {
            sprites: compiled.report.sprites,
            entries: compiled.report.entries,
            sprite_visuals: compiled.report.sprite_visuals,
            model_item_visuals: compiled.report.model_item_visuals,
            alias_entries: compiled.report.alias_entries,
            animation_strips: compiled.report.animation_strips,
            skipped_oversized: compiled.report.skipped_oversized,
            block_visuals: world.as_ref().map(|_| compiled.report.block_visuals),
            flat_block_visuals: world.as_ref().map(|_| compiled.report.flat_block_visuals),
            carried_block_sheets: world.as_ref().map(|_| compiled.report.carried_block_sheets),
            skipped_blocks: world.as_ref().map(|_| compiled.report.skipped_blocks),
            block_refusals: world.as_ref().map(|_| compiled.report.block_refusals),
        },
    };
    let mut report_bytes = serde_json::to_vec_pretty(&report_data)?;
    report_bytes.push(b'\n');
    validate_output_bundle(out, report)?;
    write_output_bundle(&[(out, &compiled.bytes), (report, &report_bytes)])?;
    if world.is_some() {
        println!(
            "compiled {} sprite and provisional ordinary-cube thumbnails ({} entries) to {} and {}",
            report_data.counts.sprites,
            report_data.counts.entries,
            out.display(),
            report.display()
        );
    } else {
        println!(
            "compiled {} pinned official Mojang sample item icons ({} entries) to {} and {}",
            report_data.counts.sprites,
            report_data.counts.entries,
            out.display(),
            report.display()
        );
    }
    Ok(())
}
