//! Composes the world-loading screen with the native progress components drawn over it.

use ui::{LoadingStage, UiNode};

use super::super::{UiPresentationError, UiPresentationRuntime};
use crate::ui_runtime::UiRuntime;
use ui_engine::engine::screen::PaintFrame;
use ui_engine::text_metrics::TextMetrics;
use ui_screens::loading_screen::{self, LoadingInput};

impl UiPresentationRuntime {
    /// Draw the world-loading screen for `stage`; `Ok(false)` when nothing drew.
    pub(in super::super) fn append_loading_screen(
        &mut self,
        runtime: &UiRuntime,
        stage: LoadingStage,
        nodes: &mut Vec<UiNode>,
        next: &mut u32,
        metrics: TextMetrics,
        content: [f32; 2],
    ) -> Result<bool, UiPresentationError> {
        let Some(renderer) = self.engine.as_deref() else {
            return Ok(false);
        };
        let translate = |key: &str| runtime.translation(key);
        let mut paint_frame = PaintFrame {
            layouts: &mut self.layouts,
            font: &self.font,
            metrics,
            solid_page: self.solid_texture_page,
            safe_area: self.safe_area,
            content,
            translate: &translate,
            language: runtime.text_generation(),
            now: self.menu_seconds,
            nodes: &mut *nodes,
            next: &mut *next,
            renderers: &mut [],
        };
        let text = loading_screen::paint(
            &mut self.screens.hud.loading,
            renderer,
            LoadingInput {
                stage,
                dimension: usize::try_from(self.hud_frame.dimension).ok(),
                clocks: &self.scene_clock,
                images: &self.menu_artwork.refs,
            },
            &mut paint_frame,
        )?;
        let Some([title, detail]) = text else {
            return Ok(false);
        };
        self.append_oreui_loading(stage, [&title, &detail], nodes, next, metrics, content)?;
        Ok(true)
    }
}

#[cfg(test)]
impl UiPresentationRuntime {
    pub(in super::super) fn loading_texture_sprite(
        &self,
        texture: &str,
    ) -> Option<(u16, [u16; 4])> {
        let engine = self.engine.as_deref()?;
        let atlas = engine.textures.lock();
        let textures = ui_engine::textures::Textures {
            assets: engine.assets(),
            set: &engine.textures,
            atlas: &atlas,
            images: Some(&self.menu_artwork.refs),
        };
        let (page, [x, y, width, height]) = textures.sprite(texture)?;
        Some((
            page,
            [x, y, x + width, y + height].map(|value| value as u16),
        ))
    }

    /// The loading screen's last laid-out draw nodes, in virtual px.
    pub fn loading_draw_nodes(&self) -> &[json_ui::DrawNode] {
        self.screens.hud.loading.nodes()
    }
}
