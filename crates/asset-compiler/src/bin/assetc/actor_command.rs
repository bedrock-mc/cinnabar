use super::{
    MAX_SOURCE_MANIFEST_BYTES, read_bounded_with_limit, validate_output_bundle, write_output_bundle,
};
use assets::AssetError;
use pack_compiler::{
    compile_actor_assets_for_assets, compile_entity_assets_with_behavior_report,
    compile_entity_assets_with_report,
};
use std::{fs, path::Path};

/// Builds actor artwork from the same resource and optional seat inputs as the entity recipe.
pub(super) fn compile_actor_assets_command(
    pack: &Path,
    manifest: &Path,
    out: &Path,
    report: &Path,
    behavior_pack: Option<&Path>,
) -> Result<(), Box<dyn std::error::Error>> {
    let root = fs::canonicalize(pack).map_err(|source| AssetError::Io {
        path: pack.into(),
        source,
    })?;
    let manifest =
        read_bounded_with_limit(manifest, MAX_SOURCE_MANIFEST_BYTES, "actor source manifest")?;
    let entities = match behavior_pack {
        Some(behavior) if behavior.join("entities").is_dir() => {
            compile_entity_assets_with_behavior_report(&root, behavior, &manifest)?
        }
        _ => compile_entity_assets_with_report(&root, &manifest)?,
    };
    let compiled = compile_actor_assets_for_assets(&root, &entities.assets)?;
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
