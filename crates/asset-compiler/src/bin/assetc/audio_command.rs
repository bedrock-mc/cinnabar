//! `assetc audio-assets`: compiles the dormant pinned sound-definition lookup.

use std::path::Path;

use asset_compiler::{AUDIO_SOUND_DEFINITIONS_RELATIVE_PATH, compile_audio_assets};
use serde::Serialize;

use super::{
    MAX_SOURCE_MANIFEST_BYTES, hex, read_bounded_with_limit, validate_output_bundle,
    write_blob_atomic,
};

#[derive(Serialize)]
struct AudioAssetsReport {
    schema: u32,
    source_relative_path: &'static str,
    source_manifest_sha256: Box<str>,
    sound_definitions_sha256: Box<str>,
    carrier_sha256: Box<str>,
    counts: AudioAssetCounts,
}

#[derive(Serialize)]
struct AudioAssetCounts {
    definitions: usize,
    alternatives: usize,
    scalar_alternatives: usize,
    object_alternatives: usize,
}

pub(super) fn compile_audio_assets_command(
    pack: &Path,
    source_manifest: &Path,
    out: &Path,
    report: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let manifest_bytes = read_bounded_with_limit(
        source_manifest,
        MAX_SOURCE_MANIFEST_BYTES,
        "audio source manifest",
    )?;
    let compiled = compile_audio_assets(pack, &manifest_bytes)?;
    let report_data = AudioAssetsReport {
        schema: 1,
        source_relative_path: AUDIO_SOUND_DEFINITIONS_RELATIVE_PATH,
        source_manifest_sha256: hex(&compiled.report.source_manifest_sha256).into_boxed_str(),
        sound_definitions_sha256: hex(&compiled.report.sound_definitions_sha256).into_boxed_str(),
        carrier_sha256: hex(&compiled.report.carrier_sha256).into_boxed_str(),
        counts: AudioAssetCounts {
            definitions: compiled.report.definition_count,
            alternatives: compiled.report.alternative_count,
            scalar_alternatives: compiled.report.scalar_alternative_count,
            object_alternatives: compiled.report.object_alternative_count,
        },
    };
    let mut report_bytes = serde_json::to_vec_pretty(&report_data)?;
    report_bytes.push(b'\n');
    validate_output_bundle(out, report)?;
    write_blob_atomic(out, &compiled.bytes)?;
    write_blob_atomic(report, &report_bytes)?;
    println!(
        "compiled {} definitions and {} alternatives to {} and {}",
        report_data.counts.definitions,
        report_data.counts.alternatives,
        out.display(),
        report.display()
    );
    Ok(())
}
