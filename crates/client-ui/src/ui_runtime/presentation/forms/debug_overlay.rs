//! Paint the F3 JSON-UI screen through the ordinary screen renderer.

use ui::UiNode;

use super::super::{TextMetrics, UiPresentationError, UiPresentationRuntime, debug_overlay};
use super::engine::{EngineInputs, EngineOutput, ScreenArt};

impl UiPresentationRuntime {
    pub(in super::super) fn append_debug_overlay(
        &mut self,
        nodes: &mut Vec<UiNode>,
        next: &mut u32,
        metrics: TextMetrics,
        content: [f32; 2],
    ) -> Result<(), UiPresentationError> {
        let (Some(lines), Some(engine)) = (
            self.debug_lines.as_ref(),
            self.form_presentation.engine.as_deref(),
        ) else {
            return Ok(());
        };
        if content.iter().any(|axis| *axis <= 0.0) {
            return Ok(());
        }
        #[cfg(feature = "tracy")]
        let _span = bevy::log::info_span!("ui.f3.paint").entered();
        let metrics = debug_overlay::visible::fitted_metrics(metrics, content[1]);
        let scale = metrics.scale.get();
        self.debug_overlay.retain_font(&self.font);
        let cache = &mut self.debug_overlay;
        let key = debug_overlay::paint::PaintKey {
            content,
            scale: [scale, metrics.dpi_scale.get()],
            gui_scale: metrics.gui_scale,
            line: [metrics.line_height_64, metrics.baseline_64],
            solid_page: self.solid_texture_page,
            safe_area: self.safe_area,
        };
        if cache.matches_lines(lines)
            && cache.painted.key == Some(key)
            && cache
                .painted
                .catalog
                .as_ref()
                .is_some_and(|catalog| std::sync::Arc::ptr_eq(catalog, engine.catalog()))
        {
            cache.painted.append(nodes, next);
            return Ok(());
        }
        #[cfg(test)]
        {
            cache.paints += 1;
        }
        let mut painted = std::mem::take(&mut cache.painted);
        painted.nodes.clear();
        let mut paint_next = 1;
        let inputs = EngineInputs {
            layouts: &mut self.layouts,
            font: &self.font,
            metrics,
            solid_page: self.solid_texture_page,
            safe_area: self.safe_area,
            content,
            translate: &|_| None,
            language: [0; 3],
        };
        let out = EngineOutput {
            nodes: &mut painted.nodes,
            next: &mut paint_next,
            overlay: &[],
        };
        engine.draw(ScreenArt::default(), inputs, out, |env, root| {
            Some(debug_overlay::render(cache, lines, (root, scale), env))
        })?;
        painted.key = Some(key);
        painted.catalog = Some(std::sync::Arc::clone(engine.catalog()));
        painted.count = paint_next - 1;
        painted.append(nodes, next);
        cache.painted = painted;
        Ok(())
    }
}
