//! Shared geometry identity for retained screen and form layouts.

use crate::LayoutEnv;

/// Layout geometry depends on both logical size and the physical pixel grid.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LayoutCacheKey {
    root: [f64; 2],
    logical_scale: f32,
    physical_pixel: f64,
}

impl LayoutCacheKey {
    /// Keys a layout using the same physical pixel size that its measurer and culling use.
    pub fn new(root: [f64; 2], logical_scale: f32, env: &LayoutEnv) -> Self {
        Self {
            root,
            logical_scale,
            physical_pixel: env.text.physical_pixel_size(),
        }
    }

    /// Keys a layout from display metrics when a measurement environment is not yet available.
    pub fn from_dpi(root: [f64; 2], logical_scale: f32, dpi_scale: f32) -> Self {
        Self {
            root,
            logical_scale,
            physical_pixel: 1.0 / f64::from(logical_scale * dpi_scale),
        }
    }

    /// The virtual root size used to measure and place the retained layout.
    pub const fn root(self) -> [f64; 2] {
        self.root
    }
}
