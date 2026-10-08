use std::path::Path;

use assets::AssetError;
use pack_compiler::compile_particle_assets;
use serde::Serialize;

use super::{
    MAX_SOURCE_MANIFEST_BYTES, hex, read_bounded_with_limit, validate_output_bundle,
    write_output_bundle,
};

#[derive(Serialize)]
struct ParticleAssetsReport {
    schema: u32,
    source: serde_json::Value,
    source_manifest_sha256: Box<str>,
    carrier_sha256: Box<str>,
    counts: ParticleCounts,
}

#[derive(Serialize)]
struct ParticleCounts {
    textures_packed: usize,
    textures_skipped: usize,
    effects: usize,
    effects_skipped: usize,
    texture_pixel_bytes: usize,
}

pub(super) fn compile_particle_assets_command(
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
    let compiled = compile_particle_assets(pack, &manifest_bytes)?;
    let report_data = ParticleAssetsReport {
        schema: 1,
        source,
        source_manifest_sha256: hex(&compiled.report.source_manifest_sha256).into_boxed_str(),
        carrier_sha256: hex(&compiled.report.carrier_sha256).into_boxed_str(),
        counts: ParticleCounts {
            textures_packed: compiled.report.textures_packed,
            textures_skipped: compiled.report.textures_skipped,
            effects: compiled.report.effects,
            effects_skipped: compiled.report.effects_skipped,
            texture_pixel_bytes: compiled.report.texture_pixel_bytes,
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
        "compiled {} particle effects and {} textures ({} textures skipped) to {} and {}",
        report_data.counts.effects,
        report_data.counts.textures_packed,
        report_data.counts.textures_skipped,
        out.display(),
        report.display()
    );
    Ok(())
}
