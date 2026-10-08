//! `assetc lang-assets`: compiles the pinned localization carrier and its
//! JSON report. Split from the CLI root to honor the production line budget.

use std::{fs, path::Path};

use assets::AssetError;
use pack_compiler::{compile_lang_assets, compile_language, vanilla_language_codes};
use serde::Serialize;

use super::{
    MAX_SOURCE_MANIFEST_BYTES, hex, read_bounded_with_limit, validate_output_bundle,
    write_blob_atomic, write_output_bundle,
};

#[derive(Serialize)]
pub(super) struct LangAssetsReport {
    pub(super) schema: u32,
    pub(super) canonical_pack_path: Box<str>,
    pub(super) source_manifest_sha256: Box<str>,
    pub(super) lang_source_sha256: Box<str>,
    pub(super) carrier_sha256: Box<str>,
    pub(super) counts: LangAssetCounts,
}

#[derive(Serialize)]
pub(super) struct LangAssetCounts {
    pub(super) entries: usize,
    pub(super) duplicate_keys: usize,
    pub(super) skipped_oversized: usize,
    pub(super) source_bytes: usize,
}

pub(super) fn compile_lang_assets_command(
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
        "language source manifest",
    )?;
    // Production compiles refuse any source that is not the pinned official
    // sample `texts/en_US.lang` bytes.
    let compiled = compile_lang_assets(
        &canonical_pack,
        &manifest_bytes,
        Some(assets::VANILLA_EN_US_LANG_SHA256),
    )?;
    let report_data = LangAssetsReport {
        schema: 2,
        canonical_pack_path: canonical_pack
            .to_string_lossy()
            .into_owned()
            .into_boxed_str(),
        source_manifest_sha256: hex(&compiled.report.source_manifest_sha256).into_boxed_str(),
        lang_source_sha256: hex(&compiled.report.lang_source_sha256).into_boxed_str(),
        carrier_sha256: hex(&compiled.report.carrier_sha256).into_boxed_str(),
        counts: LangAssetCounts {
            entries: compiled.report.entries,
            duplicate_keys: compiled.report.duplicate_keys,
            skipped_oversized: compiled.report.skipped_oversized,
            source_bytes: compiled.report.source_bytes,
        },
    };
    let mut report_bytes = serde_json::to_vec_pretty(&report_data)?;
    report_bytes.push(b'\n');
    validate_output_bundle(out, report)?;
    write_output_bundle(&[(out, &compiled.bytes), (report, &report_bytes)])?;
    println!(
        "compiled {} pinned official Mojang sample language entries to {} and {}",
        report_data.counts.entries,
        out.display(),
        report.display()
    );
    Ok(())
}

/// Writes every other language the pack lists as `<dir>/<code>.mcbelang`, then
/// the `.compiled` stamp make tracks; a listed language without a file is skipped.
pub(super) fn compile_languages_command(
    pack: &Path,
    source_manifest: &Path,
    dir: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let stamp = dir.join(".compiled");
    validate_output_bundle(&stamp, source_manifest)?;
    let manifest_bytes = read_bounded_with_limit(
        source_manifest,
        MAX_SOURCE_MANIFEST_BYTES,
        "language source manifest",
    )?;
    fs::create_dir_all(dir)?;
    let mut written = 0usize;
    for code in vanilla_language_codes(pack)? {
        if !pack.join(format!("texts/{code}.lang")).is_file() {
            continue;
        }
        let out = dir.join(format!("{code}.mcbelang"));
        validate_output_bundle(&out, source_manifest)?;
        let compiled = compile_language(pack, &code, &manifest_bytes)?;
        write_blob_atomic(&out, &compiled.bytes)?;
        written += 1;
    }
    write_blob_atomic(&stamp, format!("{written}\n").as_bytes())?;
    println!(
        "compiled {written} optional language carriers to {}",
        dir.display()
    );
    Ok(())
}
