use super::*;
use pack_compiler::{
    CompiledFontCarrier, GlyphAdvances, OutlineFontConfig, compact_font_pages,
    compile_outline_font, compile_outline_font_with_fallback, overlay_font_glyph_sheets,
};

const MAX_MANIFEST: usize = 64 * 1024;
const MAX_LICENSE: usize = 16 * 1024;

pub(super) struct PostprocessOptions<'a> {
    pub glyph_pack: Option<&'a Path>,
    pub compact_pages: bool,
}

pub(super) fn postprocess(
    mut compiled: CompiledFontCarrier,
    options: PostprocessOptions<'_>,
) -> Result<CompiledFontCarrier, FontCompileError> {
    if let Some(pack) = options.glyph_pack {
        compiled = overlay_font_glyph_sheets(compiled, pack)?;
    }
    if options.compact_pages {
        compiled = compact_font_pages(compiled)?;
    }
    Ok(compiled)
}

pub(super) fn compile(
    font: &Path,
    fallback: Option<&Path>,
    primary_only: bool,
    options: PostprocessOptions<'_>,
    manifest_path: &Path,
    out: &Path,
    report: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let manifest = read_bounded_with_limit(manifest_path, MAX_MANIFEST, "font manifest")?;
    let manifest_hash = assets::canonical_source_manifest_sha256(&manifest);
    let source: serde_json::Value = serde_json::from_slice(&manifest)?;
    let primary = verified(font, &source, "font", 32 * 1024 * 1024)?;
    let primary_license = license(font, &source, "")?;
    let raster = source
        .get("rasterization")
        .ok_or("missing font rasterization policy")?;
    let config = OutlineFontConfig {
        pixel_height: required_u32(raster, "pixel_height")?,
        atlas_side: required_u32(raster, "atlas_side")?,
        replacement_codepoint: char::from_u32(required_u32(raster, "replacement_codepoint")?)
            .ok_or("invalid replacement scalar")?,
        advances: if raster.get("proportional_advance_gap_px").is_none() {
            GlyphAdvances::Source
        } else {
            GlyphAdvances::InkPlusGap {
                gap_px: required_u32(raster, "proportional_advance_gap_px")?,
                blank_advance_px: raster
                    .get("blank_advance_px")
                    .map(|_| required_u32(raster, "blank_advance_px"))
                    .transpose()?,
            }
        },
    };
    let declared = source.get("fallback_font_sha256").is_some();
    if primary_only && fallback.is_some() {
        return Err("--primary-only takes no fallback source".into());
    }
    if declared != fallback.is_some() && !primary_only {
        return Err("declared fallback source must be supplied exactly once".into());
    }
    let secondary = if let Some(path) = fallback {
        if source.get("fallback_ranges")
            != Some(&serde_json::json!([
                [8592, 9215],
                [9312, 10175],
                [12288, 12543],
                [13312, 19903],
                [19968, 40959]
            ]))
            || required_u32(&source, "fallback_pixel_height")? != 18
            || required_u32(&source, "fallback_atlas_side")? != 2048
            || required_u32(&source, "fallback_max_pages")? != 3
        {
            return Err("unsupported fallback rasterization policy".into());
        }
        Some((
            verified(
                path,
                &source,
                "fallback_font",
                32 * 1024 * 1024 - primary.len(),
            )?,
            license(path, &source, "fallback_")?,
        ))
    } else {
        None
    };
    let mut notices = format!(
        "Converted font carrier notices\nSource manifest SHA-256: {}\n\n",
        hex(&manifest_hash)
    )
    .into_bytes();
    append_notice(&mut notices, &source, "", &primary_license)?;
    if let Some((_, license)) = &secondary {
        append_notice(&mut notices, &source, "fallback_", license)?;
    }
    if notices.len() > 2 * MAX_LICENSE + MAX_MANIFEST {
        return Err("font notices exceed policy".into());
    }
    let notices_path = out.with_file_name("ui-font-notices.txt");
    validate_output_bundle(out, report)?;
    validate_output_bundle(out, &notices_path)?;
    validate_output_bundle(report, &notices_path)?;
    for input in [Some(font), fallback, Some(manifest_path)]
        .into_iter()
        .flatten()
    {
        for output in [out, report, notices_path.as_path()] {
            validate_output_bundle(input, output)?;
        }
    }
    for (input, prefix) in [(Some(font), ""), (fallback, "fallback_")] {
        if let Some(input) = input {
            let license_path = input
                .parent()
                .ok_or("font has no source directory")?
                .join(text(&source, &format!("{prefix}license_file"))?);
            for output in [out, report, notices_path.as_path()] {
                validate_output_bundle(&license_path, output)?;
            }
        }
    }
    let compiled = match (&secondary, fallback) {
        (Some((bytes, _)), Some(path)) => {
            compile_outline_font_with_fallback(font, &primary, path, bytes, manifest_hash, config)?
        }
        _ => compile_outline_font(font, &primary, manifest_hash, config)?,
    };
    let compiled = postprocess(compiled, options)?;
    write_compiled_font_assets(
        source,
        manifest_hash,
        compiled,
        out,
        report,
        &[(&notices_path, &notices)],
    )
}

/// Rasterizes an outline font whose size and hash `source` pins, keeping its own advances.
/// The font carries its own CJK pages, so it is also its own fallback provider.
pub(super) fn compile_pinned(
    font: &Path,
    source: &serde_json::Value,
    manifest_hash: [u8; 32],
) -> Result<CompiledFontCarrier, Box<dyn std::error::Error>> {
    // Both providers read these bytes, within the two-provider 32 MiB source budget.
    let bytes = verified(font, source, "font", 16 * 1024 * 1024)?;
    Ok(compile_outline_font_with_fallback(
        font,
        &bytes,
        font,
        &bytes,
        manifest_hash,
        OutlineFontConfig::default(),
    )?)
}

fn text<'a>(
    source: &'a serde_json::Value,
    key: &str,
) -> Result<&'a str, Box<dyn std::error::Error>> {
    source
        .get(key)
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.is_empty() && value.len() <= 1024)
        .ok_or_else(|| format!("invalid font source field '{key}'").into())
}

fn verified(
    path: &Path,
    source: &serde_json::Value,
    prefix: &str,
    limit: usize,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let expected = source
        .get(format!("{prefix}_size_bytes"))
        .and_then(serde_json::Value::as_u64)
        .ok_or("invalid font source length")?;
    if expected == 0 || expected > limit as u64 {
        return Err("font declared input length exceeds policy".into());
    }
    let bytes = read_bounded_with_limit(path, limit, "font input")?;
    let digest = text(source, &format!("{prefix}_sha256"))?;
    if digest.len() != 64
        || !digest.bytes().all(|byte| byte.is_ascii_hexdigit())
        || bytes.len() as u64 != expected
        || hex(&Sha256::digest(&bytes)) != digest.to_ascii_lowercase()
    {
        return Err(if prefix == "font" {
            "outline font SHA-256 does not match the source manifest"
        } else {
            "font input does not match its pinned length/hash"
        }
        .into());
    }
    Ok(bytes)
}

fn license(
    font: &Path,
    source: &serde_json::Value,
    prefix: &str,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let name = text(source, &format!("{prefix}license_file"))?;
    if Path::new(name).components().count() != 1
        || name.contains(['/', '\\', ':'])
        || name == "."
        || name == ".."
    {
        return Err("license source must be a sibling basename".into());
    }
    let path = font
        .parent()
        .ok_or("font has no source directory")?
        .join(name);
    let bytes = verified(&path, source, &format!("{prefix}license"), MAX_LICENSE)?;
    std::str::from_utf8(&bytes)?;
    Ok(bytes)
}

fn append_notice(
    output: &mut Vec<u8>,
    source: &serde_json::Value,
    prefix: &str,
    license: &[u8],
) -> Result<(), Box<dyn std::error::Error>> {
    if text(source, &format!("{prefix}license"))? != "OFL-1.1" {
        return Err("unsupported font license".into());
    }
    output.extend_from_slice(
        format!(
            "{} — {}\n{}\n\n",
            text(source, &format!("{prefix}family"))?,
            text(source, &format!("{prefix}style"))?,
            text(source, &format!("{prefix}copyright"))?
        )
        .as_bytes(),
    );
    output.extend_from_slice(license);
    output.extend_from_slice(b"\n\n");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_and_license_pins_fail_before_outline_parsing() {
        let directory = tempfile::tempdir().unwrap();
        let font = directory.path().join("source.otf");
        fs::write(&font, b"fake font").unwrap();
        let source = serde_json::json!({ "font_size_bytes": 9, "font_sha256": "00".repeat(32) });
        assert!(verified(&font, &source, "font", 9).is_err());
        assert!(verified(&font, &source, "font", 8).is_err());
        let source = serde_json::json!({ "license_file": "../LICENSE" });
        assert!(license(&font, &source, "").is_err());
    }

    #[test]
    fn delivered_notice_retains_exact_license_bytes_and_copyright() {
        let source = serde_json::json!({ "family": "Sample Font", "style": "Regular", "copyright": "Sample copyright", "license": "OFL-1.1" });
        let license = b"Exact license text\r\n";
        let mut output = Vec::new();
        append_notice(&mut output, &source, "", license).unwrap();
        assert!(
            output
                .windows(license.len())
                .any(|window| window == license)
        );
        assert!(
            String::from_utf8(output)
                .unwrap()
                .contains("Sample copyright")
        );
    }
}
