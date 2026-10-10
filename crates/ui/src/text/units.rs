//! Measures in exact device units when font texels span whole half device pixels.
//! Rounds only finished glyph edges into output units, preventing cumulative pen drift.

use super::TextError;

/// One device pixel per texel in `scale_1024 * device_65536` units, halved.
const HALF_PIXEL_PER_TEXEL: i64 = 1024 * 65_536 / 2;

/// How a layout scales font texel values into the 1/64 pixels it measures in.
#[derive(Clone, Copy, Debug)]
pub(super) enum Units {
    /// Output 1/64 pixels, scaling texel values by `scale_1024 / 1024`.
    Output { scale_1024: i64 },
    /// Device 1/64 pixels: each texel spans `half_pixels / 2` device pixels and each output
    /// pixel spans `device_65536 / 65536` device pixels.
    Device { half_pixels: i64, device_65536: i64 },
}

impl Units {
    /// Selects device units when fixed-point scales put texels on whole half device pixels.
    /// Fonts with separate em metrics retain output units for their rescaled fallback glyphs.
    pub(super) fn select(scale_1024: i64, device_65536: u32, own_em_metrics: bool) -> Self {
        let device_65536 = i64::from(device_65536);
        if device_65536 > 0 && !own_em_metrics {
            let product = scale_1024 * device_65536;
            let half_pixels = (product + HALF_PIXEL_PER_TEXEL / 2) / HALF_PIXEL_PER_TEXEL;
            let tolerance = (device_65536 + scale_1024) / 2 + 1;
            if half_pixels > 0 && (product - half_pixels * HALF_PIXEL_PER_TEXEL).abs() <= tolerance
            {
                return Self::Device {
                    half_pixels,
                    device_65536,
                };
            }
        }
        Self::Output { scale_1024 }
    }

    /// A font value in 1/64 texels, in layout units, truncated toward zero.
    pub(super) fn texels(self, value_64: i64) -> Result<i64, TextError> {
        let (numerator, denominator) = match self {
            Self::Output { scale_1024 } => (scale_1024, crate::UiScale::SCALE_DENOMINATOR),
            Self::Device { half_pixels, .. } => (half_pixels, 2),
        };
        value_64
            .checked_mul(numerator)
            .and_then(|scaled| scaled.checked_div(denominator))
            .ok_or(TextError::FixedPointOverflow)
    }

    /// A request value in output 1/64 pixels, in layout units, rounded to nearest.
    pub(super) fn from_output(self, value_64: i64) -> Result<i64, TextError> {
        match self {
            Self::Output { .. } => Ok(value_64),
            Self::Device { device_65536, .. } => divide_rounded(value_64, device_65536, 65_536),
        }
    }

    /// A layout value in output 1/64 pixels, rounded to nearest.
    pub(super) fn to_output(self, value_64: i64) -> Result<i64, TextError> {
        match self {
            Self::Output { .. } => Ok(value_64),
            Self::Device { device_65536, .. } => divide_rounded(value_64, 65_536, device_65536),
        }
    }
}

/// `value * numerator / denominator`, rounded half away from zero; `denominator` is positive.
fn divide_rounded(value: i64, numerator: i64, denominator: i64) -> Result<i64, TextError> {
    let product = value
        .checked_mul(numerator)
        .ok_or(TextError::FixedPointOverflow)?;
    let half = denominator / 2;
    let biased = if product < 0 {
        product.checked_sub(half)
    } else {
        product.checked_add(half)
    };
    biased
        .map(|biased| biased / denominator)
        .ok_or(TextError::FixedPointOverflow)
}
