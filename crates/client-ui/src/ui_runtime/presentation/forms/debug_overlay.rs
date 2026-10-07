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
        let metrics = debug_overlay::fitted_metrics(metrics, content[1]);
        let scale = metrics.scale.get();
        self.debug_overlay.retain_font(&self.font);
        let cache = &mut self.debug_overlay;
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
            nodes,
            next,
            overlay: &[],
        };
        engine.draw(ScreenArt::default(), inputs, out, |env, root| {
            Some(debug_overlay::render(cache, lines, (root, scale), env))
        })?;
        Ok(())
    }
}
