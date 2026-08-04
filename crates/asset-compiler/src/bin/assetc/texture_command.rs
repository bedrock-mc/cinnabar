use std::path::Path;

use asset_compiler::compile_texture_catalog;
use assets::{AssetError, RuntimeTextureCatalog, write_blob_atomic};
use serde::Serialize;

use super::{
    MAX_SOURCE_MANIFEST_BYTES, canonical_source_manifest_sha256, hex, read_bounded_with_limit,
    validate_output_bundle,
};

#[derive(Serialize)]
struct TextureAssetsReport {
    schema: u32,
    source: serde_json::Value,
    source_manifest_sha256: Box<str>,
    carrier_sha256: Box<str>,
    routes: usize,
}

pub(super) fn compile_texture_assets_command(
    pack: &Path,
    source_manifest: &Path,
    out: &Path,
    report: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let canonical_pack = std::fs::canonicalize(pack).map_err(|source| AssetError::Io {
        path: pack.to_path_buf(),
        source,
    })?;
    let manifest_bytes = read_bounded_with_limit(
        source_manifest,
        MAX_SOURCE_MANIFEST_BYTES,
        "texture source manifest",
    )?;
    let source =
        serde_json::from_slice::<serde_json::Value>(&manifest_bytes).map_err(|source| {
            AssetError::Json {
                path: source_manifest.to_path_buf(),
                source,
            }
        })?;
    let source_manifest_sha256 = canonical_source_manifest_sha256(&manifest_bytes);
    let bytes = compile_texture_catalog(&canonical_pack, source_manifest_sha256)?;
    let catalog =
        RuntimeTextureCatalog::decode(&bytes, source_manifest_sha256).map_err(|error| {
            AssetError::InvalidCompiledAssets {
                detail: format!("decode compiled texture catalog: {error}").into_boxed_str(),
            }
        })?;
    let identity = catalog.identity();
    let report_data = TextureAssetsReport {
        schema: identity.schema,
        source,
        source_manifest_sha256: hex(&identity.source_manifest_sha256).into_boxed_str(),
        carrier_sha256: hex(&identity.carrier_sha256).into_boxed_str(),
        routes: catalog.routes().len(),
    };
    let mut report_bytes = serde_json::to_vec_pretty(&report_data)?;
    report_bytes.push(b'\n');
    validate_output_bundle(out, report)?;
    write_blob_atomic(out, &bytes)?;
    write_blob_atomic(report, &report_bytes)?;
    println!(
        "compiled {} terrain texture routes from {} to {} and {}",
        report_data.routes,
        canonical_pack.display(),
        out.display(),
        report.display()
    );
    Ok(())
}
