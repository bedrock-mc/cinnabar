use super::{
    MAX_SOURCE_MANIFEST_BYTES, read_bounded_with_limit, validate_output_bundle, write_output_bundle,
};
use assets::AssetError;
use pack_compiler::compile_actor_assets;
use std::{fs, path::Path};

pub(super) fn compile_actor_assets_command(
    pack: &Path,
    manifest: &Path,
    out: &Path,
    report: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let root = fs::canonicalize(pack).map_err(|source| AssetError::Io {
        path: pack.into(),
        source,
    })?;
    let manifest =
        read_bounded_with_limit(manifest, MAX_SOURCE_MANIFEST_BYTES, "actor source manifest")?;
    let compiled = compile_actor_assets(&root, &manifest)?;
    let mut report_bytes = serde_json::to_vec_pretty(&compiled.report)?;
    report_bytes.push(b'\n');
    validate_output_bundle(out, report)?;
    write_output_bundle(&[(out, &compiled.bytes), (report, &report_bytes)])?;
    println!(
        "compiled {} neutral actor bindings; {} observable fallbacks",
        compiled.report.bindings,
        compiled.report.fallbacks.len()
    );
    Ok(())
}
