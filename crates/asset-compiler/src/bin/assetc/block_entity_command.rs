use std::path::Path;

use assets::AssetError;
use pack_compiler::compile_block_entity_assets;
use serde::Serialize;

use super::{
    MAX_SOURCE_MANIFEST_BYTES, hex, read_bounded_with_limit, validate_output_bundle,
    write_output_bundle,
};

#[derive(Serialize)]
struct BlockEntityAssetsReport {
    schema: u32,
    source: serde_json::Value,
    source_manifest_sha256: Box<str>,
    carrier_sha256: Box<str>,
    atlas_size: [u32; 2],
    textures_packed: usize,
    textures_skipped_oversized: usize,
    textures_skipped_undecodable: usize,
}

pub(super) fn compile_block_entity_assets_command(
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
    let compiled = compile_block_entity_assets(pack, &manifest_bytes)?;
    let report_data = BlockEntityAssetsReport {
        schema: 1,
        source,
        source_manifest_sha256: hex(&compiled.report.source_manifest_sha256).into_boxed_str(),
        carrier_sha256: hex(&compiled.report.carrier_sha256).into_boxed_str(),
        atlas_size: compiled.report.atlas_size,
        textures_packed: compiled.report.textures_packed,
        textures_skipped_oversized: compiled.report.textures_skipped_oversized,
        textures_skipped_undecodable: compiled.report.textures_skipped_undecodable,
    };
    let mut report_bytes =
        serde_json::to_vec_pretty(&report_data).map_err(|source| AssetError::Json {
            path: report.to_path_buf(),
            source,
        })?;
    report_bytes.push(b'\n');
    validate_output_bundle(out, report)?;
    write_output_bundle(&[(out, &compiled.bytes), (report, &report_bytes)])?;
    println!(
        "compiled {} block-entity textures into a {}x{} atlas ({} oversized skipped) to {} and {}",
        report_data.textures_packed,
        report_data.atlas_size[0],
        report_data.atlas_size[1],
        report_data.textures_skipped_oversized,
        out.display(),
        report.display()
    );
    Ok(())
}
