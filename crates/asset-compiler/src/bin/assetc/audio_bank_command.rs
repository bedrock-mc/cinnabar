//! `assetc audio-bank`: packs the sound routing JSON and FSB sound files into MCBESND1.

use std::path::Path;

use super::{validate_output_bundle, write_output_bundle};

pub(super) fn compile_audio_bank_command(
    pack: &Path,
    out: &Path,
    report: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    validate_output_bundle(out, report)?;
    let compiled = pack_compiler::compile_audio_bank(pack)?;
    let mut report_bytes = serde_json::to_vec_pretty(&compiled.report)?;
    report_bytes.push(b'\n');
    write_output_bundle(&[(out, &compiled.bytes), (report, &report_bytes)])?;
    println!(
        "packed {} sound files ({} skipped) to {} and {}",
        compiled.report.files,
        compiled.report.skipped_files,
        out.display(),
        report.display()
    );
    Ok(())
}
