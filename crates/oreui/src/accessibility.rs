//! New-menu sizing leaves the gameplay GUI preference unchanged.

use launcher::menu::settings_options::SettingsOptions;
use ui_engine::text_metrics::TextMetrics;

use crate::OreUiScreen;

impl OreUiScreen {
    /// Refreshes the menu preference, including while a loading screen hides the menu.
    pub fn configure_accessibility(&mut self, settings: &SettingsOptions) {
        self.extra_large_ui = settings.extra_large_ui();
    }

    /// Adds one physical GUI pixel per design pixel before the canvas derives rem and text size.
    pub fn accessibility_metrics(&self, mut metrics: TextMetrics) -> TextMetrics {
        if self.extra_large_ui {
            metrics.gui_scale += 1.0;
            metrics.scale = ui::UiScale::new_display(
                metrics.gui_scale / (ui::FONT_DESIGN_PIXEL_TEXELS as f32 * metrics.dpi_scale.get()),
            )
            .expect("supported GUI scale and DPI produce a valid accessibility scale");
        }
        metrics
    }
}
