use std::{fs, path::Path};

use asset_compiler::compile_hud_assets;
use assets::AssetError;
use serde::Serialize;

use super::{
    MAX_SOURCE_MANIFEST_BYTES, hex, read_bounded_with_limit, validate_output_bundle,
    write_blob_atomic,
};

#[derive(Serialize)]
pub(super) struct HudAssetsReport {
    pub(super) schema: u32,
    pub(super) canonical_pack_path: Box<str>,
    pub(super) source_manifest_sha256: Box<str>,
    pub(super) carrier_sha256: Box<str>,
    pub(super) counts: HudAssetCounts,
}

#[derive(Serialize)]
pub(super) struct HudAssetCounts {
    pub(super) textures: usize,
    pub(super) source_bytes: usize,
    pub(super) decoded_bytes: usize,
}

pub(super) fn compile_hud_assets_command(
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
