use std::path::Path;

use super::{
    MAX_SOURCE_MANIFEST_BYTES, read_bounded_with_limit, validate_output_bundle, write_output_bundle,
};

pub(super) fn compile_audio_pcm_command(
    pack: &Path,
    catalog: &Path,
    manifest: &Path,
    out: &Path,
    report: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    // Reuse the same lexical, canonical and hard-link alias checks for all inputs.
    validate_output_bundle(out, report)?;
    let sample = pack.join(assets::reviewed_audio_pcm_identity().source_path());
    for output in [out, report] {
        for input in [catalog, manifest, sample.as_path()] {
            validate_output_bundle(output, input)?;
        }
    }
    let catalog_bytes =
        read_bounded_with_limit(catalog, assets::MAX_AUDIO_CARRIER_BYTES, "sound catalog")?;
    let manifest_bytes =
        read_bounded_with_limit(manifest, MAX_SOURCE_MANIFEST_BYTES, "audio source manifest")?;
    let compiled = pack_compiler::compile_audio_pcm_assets(pack, &catalog_bytes, &manifest_bytes)?;
    let mut report_bytes = serde_json::to_vec_pretty(&compiled.report)?;
    report_bytes.push(b'\n');
    write_output_bundle(&[(out, &compiled.bytes), (report, &report_bytes)])?;
    println!(
        "compiled finite no-loop PCM to {} and {}; playback remains inactive",
        out.display(),
        report.display()
    );
    Ok(())
}
