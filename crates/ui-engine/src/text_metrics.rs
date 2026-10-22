//! Per-frame text metrics shared by every HUD, chat, scoreboard and nametag run.

use assets::RuntimeFontCatalog;
use ui::{DpiScale, TextLayoutRequest, TextShadow, TextStyle, TextWrap, UiScale};

use ui::gui_scale;

use ui::{FONT_DESIGN_PIXEL_TEXELS, TEXT_BASELINE_64, TEXT_LINE_HEIGHT_64, TEXT_SHADOW_OFFSET_64};

/// Per-frame text metrics shared by every HUD, chat, and scoreboard run so a
/// single frame cannot mix scales or line pitches. Font atlas texels are two
/// texels per GUI design pixel, while sprite geometry uses one GUI pixel.
#[derive(Clone, Copy, PartialEq)]
pub struct TextMetrics {
    pub scale: UiScale,
    /// Physical pixels per GUI unit: the GUI scale, on whose pixel grid draws land.
    pub gui_scale: f32,
    pub dpi_scale: DpiScale,
    pub line_height_64: u32,
    pub baseline_64: u32,
    shadow: TextShadow,
}

impl TextMetrics {
    /// Uses the same GUI-scale choice as sprite geometry. The font atlas
    /// is authored at two texels per GUI design pixel, so its logical scale is
    /// half the sprite scale before the platform DPI is removed. Display DPI
    /// changes logical layout units, not the selected physical GUI scale.
    pub fn for_viewport(
        physical_size: [u32; 2],
        dpi_scale: DpiScale,
        preference: Option<u8>,
    ) -> Self {
        let dpi = dpi_scale.get();
        let k = gui_scale(physical_size, preference) as f32;
        let scale = k / (FONT_DESIGN_PIXEL_TEXELS as f32 * dpi);
        Self {
            scale: UiScale::new_display(scale)
                .expect("supported GUI scale and DPI produce a valid display scale"),
            gui_scale: k,
            dpi_scale,
            line_height_64: TEXT_LINE_HEIGHT_64,
            baseline_64: TEXT_BASELINE_64,
            shadow: TextShadow::Offset64(TEXT_SHADOW_OFFSET_64),
        }
    }

    pub fn request<'a>(
        &self,
        text: &'a str,
        width_64: u32,
        font: &'a RuntimeFontCatalog,
    ) -> TextLayoutRequest<'a> {
        TextLayoutRequest {
            text,
            style: TextStyle::default(),
            width_64,
            line_height_64: self.line_height_64,
            baseline_64: self.baseline_64,
            scale: self.scale,
            font,
            wrap: TextWrap {
                device_scale_65536: (self.dpi_scale.get() * 65_536.0).round() as u32,
                allow_visual_overflow: true,
                ..TextWrap::default()
            },
        }
    }

    pub const fn shadow(&self) -> TextShadow {
        self.shadow
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ui::DesktopGuiScale;

    #[test]
    fn desktop_default_scale_uses_physical_pixels_at_every_display_dpi() {
        for (physical, expected) in [
            ([1280, 720], 2),
            ([1366, 768], 3),
            ([1920, 1080], 4),
            ([2560, 1440], 5),
        ] {
            for dpi in [1.0, 1.25, 1.5] {
                let metrics =
                    TextMetrics::for_viewport(physical, DpiScale::new(dpi).unwrap(), None);
                assert_eq!(
                    metrics.gui_scale, expected as f32,
                    "{physical:?}, DPI {dpi}"
                );
                let logical_gui_pixel = metrics.scale.get() * FONT_DESIGN_PIXEL_TEXELS as f32;
                assert!(
                    (logical_gui_pixel * dpi - expected as f32).abs() < 1e-5,
                    "{physical:?}, DPI {dpi}"
                );
            }
        }
    }

    #[test]
    fn every_desktop_modifier_reaches_layout_with_one_dpi_conversion() {
        for physical in [[1280, 720], [1366, 768], [1920, 1080], [2560, 1440]] {
            let desktop = DesktopGuiScale::for_window(physical);
            let optimal = f32::from(desktop.scale_for_offset(0));
            for choice in desktop.choices() {
                let selected = desktop.scale_for_offset(choice.offset);
                for dpi in [1.0, 1.25, 1.5] {
                    let metrics = TextMetrics::for_viewport(
                        physical,
                        DpiScale::new(dpi).unwrap(),
                        Some(selected),
                    );
                    assert_eq!(metrics.gui_scale, f32::from(selected));
                    let physical_gui_pixel =
                        metrics.scale.get() * FONT_DESIGN_PIXEL_TEXELS as f32 * dpi;
                    assert!((physical_gui_pixel - f32::from(selected)).abs() < 1e-5);
                    assert_eq!(
                        (physical_gui_pixel / optimal * 100.0).round() as u16,
                        choice.percentage,
                        "{physical:?}, DPI {dpi}, modifier {}",
                        choice.offset
                    );
                }
            }
        }
    }
}
