use std::path::Path;

use assets::AssetError;
use pack_compiler::compile_ui_assets;
use serde::Serialize;

use super::{
    MAX_SOURCE_MANIFEST_BYTES, hex, read_bounded_with_limit, validate_output_bundle,
    write_output_bundle,
};

#[derive(Serialize)]
struct UiAssetsReport {
    schema: u32,
    source: serde_json::Value,
    source_manifest_sha256: Box<str>,
    carrier_sha256: Box<str>,
    counts: UiCounts,
}

#[derive(Serialize)]
struct UiCounts {
    atlas_pages: usize,
    textures_packed: usize,
    textures_skipped_oversized: usize,
    textures_skipped_undecodable: usize,
    sidecars: usize,
    sidecars_skipped: usize,
    ui_files: usize,
    ui_files_skipped: usize,
    atlas_pixel_bytes: usize,
}

pub(super) fn compile_ui_assets_command(
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
    let compiled = compile_ui_assets(pack, &manifest_bytes)?;
    let report_data = UiAssetsReport {
        schema: 1,
        source,
        source_manifest_sha256: hex(&compiled.report.source_manifest_sha256).into_boxed_str(),
        carrier_sha256: hex(&compiled.report.carrier_sha256).into_boxed_str(),
        counts: UiCounts {
            atlas_pages: compiled.report.atlas_pages,
            textures_packed: compiled.report.textures_packed,
            textures_skipped_oversized: compiled.report.textures_skipped_oversized,
            textures_skipped_undecodable: compiled.report.textures_skipped_undecodable,
            sidecars: compiled.report.sidecars,
            sidecars_skipped: compiled.report.sidecars_skipped,
            ui_files: compiled.report.ui_files,
            ui_files_skipped: compiled.report.ui_files_skipped,
            atlas_pixel_bytes: compiled.report.atlas_pixel_bytes,
        },
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
        "compiled {} ui textures into {} atlas page(s) ({} skipped), {} sidecars, {} ui json files to {} and {}",
        report_data.counts.textures_packed,
        report_data.counts.atlas_pages,
        report_data.counts.textures_skipped_oversized,
        report_data.counts.sidecars,
        report_data.counts.ui_files,
        out.display(),
        report.display()
    );
    Ok(())
}
