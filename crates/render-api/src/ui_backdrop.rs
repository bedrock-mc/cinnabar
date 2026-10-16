//! Backdrop geometry: UI shapes that show a processed copy of the 3-D scene behind them.
//!
//! A backdrop batch draws nothing into the UI layers. Before any UI layer composites, the
//! renderer replaces the scene under the batch's geometry with the scene blurred, saturated and
//! brightened by the vertex's [`UiBackdropPaint`]. Vertex UV `y` holds the shape's antialiased
//! area coverage and tint alpha its opacity; tint RGB is unused.

/// Largest blur a backdrop vertex may request, as a Gaussian standard deviation in physical
/// pixels. The shared blur pyramid reaches this radius at its smallest level.
pub const UI_BACKDROP_MAX_BLUR_PIXELS: f32 = 128.0;
/// Upper bound for backdrop saturation and brightness factors.
pub const UI_BACKDROP_MAX_FACTOR: f32 = 2.0;

/// How a backdrop processes the scene under it, packed into a vertex's overlay field.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UiBackdropPaint {
    /// Gaussian standard deviation in physical pixels, like CSS `blur()`.
    pub blur: f32,
    /// CSS `saturate()` factor applied to sRGB-encoded values; 1 leaves colour unchanged.
    pub saturation: f32,
    /// CSS `brightness()` factor applied to sRGB-encoded values; 1 leaves colour unchanged.
    pub brightness: f32,
}

impl Default for UiBackdropPaint {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl UiBackdropPaint {
    /// Shows the scene unchanged.
    pub const IDENTITY: Self = Self {
        blur: 0.0,
        saturation: 1.0,
        brightness: 1.0,
    };

    /// Clamps every factor to its renderable bound; non-finite values become the identity.
    #[must_use]
    pub fn bounded(self) -> Self {
        let clamp = |value: f32, identity: f32, max: f32| {
            if value.is_finite() {
                value.clamp(0.0, max)
            } else {
                identity
            }
        };
        Self {
            blur: clamp(self.blur, 0.0, UI_BACKDROP_MAX_BLUR_PIXELS),
            saturation: clamp(self.saturation, 1.0, UI_BACKDROP_MAX_FACTOR),
            brightness: clamp(self.brightness, 1.0, UI_BACKDROP_MAX_FACTOR),
        }
    }

    /// Applies `self` to a scene already processed by `under`, as a CSS backdrop filter
    /// sees what an earlier backdrop drew. Gaussian blurs add in quadrature; saturation and
    /// brightness multiply.
    #[must_use]
    pub fn over(self, under: Self) -> Self {
        Self {
            blur: self.blur.hypot(under.blur),
            saturation: self.saturation * under.saturation,
            brightness: self.brightness * under.brightness,
        }
        .bounded()
    }

    /// Moves `self` toward the identity by `opacity`, for a partly faded backdrop seen through
    /// another.
    #[must_use]
    pub fn faded(self, opacity: f32) -> Self {
        let opacity = if opacity.is_finite() {
            opacity.clamp(0.0, 1.0)
        } else {
            1.0
        };
        let mix = |value: f32, identity: f32| identity + (value - identity) * opacity;
        Self {
            blur: mix(self.blur, 0.0),
            saturation: mix(self.saturation, 1.0),
            brightness: mix(self.brightness, 1.0),
        }
    }

    /// The vertex overlay field carrying this paint.
    #[must_use]
    pub fn overlay(self) -> [f32; 4] {
        let bounded = self.bounded();
        [bounded.blur, bounded.saturation, bounded.brightness, 0.0]
    }

    /// Recovers the paint a vertex overlay field carries, bounded for rendering.
    #[must_use]
    pub fn from_overlay(overlay: [f32; 4]) -> Self {
        Self {
            blur: overlay[0],
            saturation: overlay[1],
            brightness: overlay[2],
        }
        .bounded()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paint_round_trips_through_the_vertex_overlay_within_its_bounds() {
        let paint = UiBackdropPaint {
            blur: 26.0,
            saturation: 1.12,
            brightness: 0.62,
        };
        assert_eq!(UiBackdropPaint::from_overlay(paint.overlay()), paint);
        let wild = UiBackdropPaint {
            blur: 1e9,
            saturation: -3.0,
            brightness: f32::NAN,
        };
        assert_eq!(
            UiBackdropPaint::from_overlay(wild.overlay()),
            UiBackdropPaint {
                blur: UI_BACKDROP_MAX_BLUR_PIXELS,
                saturation: 0.0,
                brightness: 1.0,
            }
        );
    }

    #[test]
    fn stacked_backdrops_compose_like_successive_css_filters() {
        let world = UiBackdropPaint {
            blur: 3.0,
            saturation: 1.0,
            brightness: 0.62,
        };
        let glass = UiBackdropPaint {
            blur: 4.0,
            saturation: 1.12,
            brightness: 1.0,
        };
        let seen = glass.over(world);
        assert_eq!(seen.blur, 5.0);
        assert!((seen.saturation - 1.12).abs() < 1e-6);
        assert!((seen.brightness - 0.62).abs() < 1e-6);
        assert_eq!(world.faded(0.0), UiBackdropPaint::IDENTITY);
        assert_eq!(world.faded(1.0), world);
        assert_eq!(world.faded(0.5).brightness, 0.81);
    }
}
