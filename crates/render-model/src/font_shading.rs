//! Font coverage parameters shared by the CPU target and the UI shader.

pub const SDF_DISTANCE_SCALE: f32 = 7.96875;
pub const SDF_DISTANCE_BIAS: f32 = SDF_DISTANCE_SCALE * 0.5;
pub const SDF_SMOOTHING_SCALE: f32 = 128.0 / 255.0;
pub const SDF_MIN_THRESHOLD: f32 = 0.000001;
pub const COVERAGE_GAMMA_BASE: f32 = 1.45;
pub const COVERAGE_LUMINANCE: [f32; 3] = [0.2126, 0.7152, 0.0722];

/// Converts a filtered distance sample to glyph coverage at the drawn texel scale.
pub fn sdf_coverage(sample: f32, texels_per_pixel: f32) -> f32 {
    let distance = sample * SDF_DISTANCE_SCALE - SDF_DISTANCE_BIAS;
    let threshold = (SDF_SMOOTHING_SCALE * texels_per_pixel).max(SDF_MIN_THRESHOLD);
    let t = ((distance + threshold) / (2.0 * threshold)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Applies native text coverage gamma using the unpremultiplied text colour.
pub fn coverage_gamma(coverage: f32, color: [f32; 3]) -> f32 {
    let luminance: f32 = color
        .into_iter()
        .zip(COVERAGE_LUMINANCE)
        .map(|(channel, weight)| channel * weight)
        .sum();
    coverage.powf(COVERAGE_GAMMA_BASE - luminance)
}
