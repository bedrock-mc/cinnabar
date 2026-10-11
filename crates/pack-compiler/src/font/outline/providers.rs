//! Optional outline providers and shared raster admission.

use super::*;

const MAX_ALPHA_BYTES: usize = 64 * 1024 * 1024;

/// Compiles both complete character maps, preferring the primary face on overlap.
pub fn compile_outline_font_with_fallback(
    primary_path: &Path,
    primary_bytes: &[u8],
    fallback_path: &Path,
    fallback_bytes: &[u8],
    manifest: [u8; 32],
    config: OutlineFontConfig,
) -> Result<CompiledFontCarrier, FontCompileError> {
    validate_config(primary_bytes, manifest, config)?;
    validate_config(fallback_bytes, manifest, config)?;
    if primary_bytes
        .len()
        .checked_add(fallback_bytes.len())
        .is_none_or(|bytes| bytes as u64 > MAX_FONT_SOURCE_BYTES)
    {
        return Err(invalid("outline providers exceed source byte bound"));
    }
    let primary = source::parse(primary_path, primary_bytes)?;
    let fallback = source::parse(fallback_path, fallback_bytes)?;
    complete::compile(
        &[
            (primary_path, primary_bytes, &primary),
            (fallback_path, fallback_bytes, &fallback),
        ],
        manifest,
        config,
    )
}

/// Admits raster scratch before allocating any coverage buffers.
pub(super) fn add_charge(previous: usize, amount: usize) -> Result<usize, FontCompileError> {
    previous
        .checked_add(amount)
        .filter(|n| *n <= MAX_ALPHA_BYTES)
        .ok_or_else(|| invalid("font alpha scratch exceeds policy"))
}

/// Validates one glyph and charges both the raster and its trimmed copy.
pub(super) fn metric_charge(
    codepoint: char,
    metrics: &fontdue::Metrics,
) -> Result<usize, FontCompileError> {
    let pixels = metrics
        .width
        .checked_mul(metrics.height)
        .filter(|n| *n <= 4_096)
        .ok_or_else(|| metric_error(codepoint, "bitmap"))?;
    if metrics.width > 64
        || metrics.height > 64
        || !metrics.advance_width.is_finite()
        || !metrics.advance_height.is_finite()
        || !metrics.bounds.xmin.is_finite()
        || !metrics.bounds.ymin.is_finite()
        || !metrics.bounds.width.is_finite()
        || !metrics.bounds.height.is_finite()
        || !(f64::from(metrics.advance_width).round() * 64.0).is_finite()
        || (f64::from(metrics.advance_width).round() * 64.0) < f64::from(i16::MIN)
        || (f64::from(metrics.advance_width).round() * 64.0) > f64::from(i16::MAX)
        || i16::try_from(metrics.xmin).is_err()
        || metrics
            .xmin
            .checked_add(metrics.width as i32)
            .is_none_or(|x| i16::try_from(x).is_err())
        || metrics
            .ymin
            .checked_neg()
            .is_none_or(|y| i16::try_from(y).is_err())
        || metrics
            .ymin
            .checked_add(metrics.height as i32)
            .and_then(i32::checked_neg)
            .is_none_or(|y| i16::try_from(y).is_err())
    {
        return Err(metric_error(codepoint, "preflight"));
    }
    pixels
        .checked_add(pixels.max(1))
        .ok_or_else(|| invalid("alpha accounting overflows"))
}

/// Builds a hash-bound coverage page for an outline source.
pub(super) fn page(
    path: &str,
    source: &[u8],
    coverage: Box<[u8]>,
    side: u32,
) -> Result<FontTexturePage, FontCompileError> {
    Ok(FontTexturePage {
        source_path: path.into(),
        source_bytes: u32::try_from(source.len())
            .map_err(|_| invalid("font source length overflows"))?,
        source_sha256: Sha256::digest(source).into(),
        pixels_sha256: Sha256::digest(&coverage).into(),
        width: side,
        height: side,
        pixels: FontPixels::Coverage(coverage),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn alpha_charge_checks_cumulative_overflow_before_raster() {
        assert!(add_charge(MAX_ALPHA_BYTES, 1).is_err());
        assert!(add_charge(usize::MAX, 1).is_err());
        assert_eq!(add_charge(MAX_ALPHA_BYTES - 1, 1).unwrap(), MAX_ALPHA_BYTES);
    }
    #[test]
    fn blank_metrics_and_hostile_dimensions_have_explicit_preflight_bounds() {
        let mut metrics = fontdue::Metrics::default();
        assert_eq!(metric_charge(' ', &metrics).unwrap(), 1);
        metrics.width = 65;
        assert!(metric_charge('X', &metrics).is_err());
        metrics.width = usize::MAX;
        metrics.height = 2;
        assert!(metric_charge('X', &metrics).is_err());
        metrics = fontdue::Metrics::default();
        metrics.bounds.width = f32::NAN;
        assert!(metric_charge('X', &metrics).is_err());
    }
}
