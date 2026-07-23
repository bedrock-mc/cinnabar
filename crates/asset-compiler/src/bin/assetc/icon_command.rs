//! `assetc icon-assets`: compiles the pinned item-icon carrier and its JSON
//! report. Split from the CLI root to honor the production line budget.

use std::{fs, path::Path};

use asset_compiler::compile_icon_assets;
use assets::AssetError;
use serde::Serialize;

use super::{
    MAX_SOURCE_MANIFEST_BYTES, hex, read_bounded_with_limit, validate_output_bundle,
    write_blob_atomic,
};

#[derive(Serialize)]
pub(super) struct IconAssetsReport {
    pub(super) schema: u32,
    pub(super) canonical_pack_path: Box<str>,
    pub(super) source_manifest_sha256: Box<str>,
    pub(super) carrier_sha256: Box<str>,
    pub(super) counts: IconAssetCounts,
}

#[derive(Serialize)]
pub(super) struct IconAssetCounts {
    pub(super) sprites: usize,
    pub(super) entries: usize,
    pub(super) sprite_visuals: usize,
    pub(super) alias_entries: usize,
    pub(super) animation_strips: usize,
    pub(super) skipped_oversized: usize,
}

pub(super) fn compile_icon_assets_command(
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
        "icon source manifest",
    )?;
    let compiled = compile_icon_assets(&canonical_pack, &manifest_bytes)?;
    let report_data = IconAssetsReport {
        schema: 1,
        canonical_pack_path: canonical_pack
            .to_string_lossy()
            .into_owned()
            .into_boxed_str(),
        source_manifest_sha256: hex(&compiled.report.source_manifest_sha256).into_boxed_str(),
        carrier_sha256: hex(&compiled.report.carrier_sha256).into_boxed_str(),
        counts: IconAssetCounts {
            sprites: compiled.report.sprites,
            entries: compiled.report.entries,
            sprite_visuals: compiled.report.sprite_visuals,
            alias_entries: compiled.report.alias_entries,
            animation_strips: compiled.report.animation_strips,
            skipped_oversized: compiled.report.skipped_oversized,
        },
    };
    let mut report_bytes = serde_json::to_vec_pretty(&report_data)?;
    report_bytes.push(b'\n');
    validate_output_bundle(out, report)?;
    write_blob_atomic(out, &compiled.bytes)?;
    write_blob_atomic(report, &report_bytes)?;
    println!(
        "compiled {} pinned official Mojang sample item icons ({} entries) to {} and {}",
        report_data.counts.sprites,
        report_data.counts.entries,
        out.display(),
        report.display()
    );
    Ok(())
}
