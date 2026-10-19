//! The player preview raster, posed and framed as each vanilla player renderer asks.

use std::cell::Cell;

use serde_json::Value;
use ui::{IconRef, UiVisual};

use super::super::player_preview::{PreviewView, controller::PreviewControl};
use ui_engine::engine::{
    Canvas,
    screen::{CustomDraw, CustomRenderer, PaintError},
};

/// The preview raster, posed and framed as each vanilla player renderer asks.
#[derive(Default)]
pub(in crate::ui_runtime::presentation) struct PreviewRenderer<'a> {
    pub(super) preview: Option<IconRef>,
    /// Where a drawn player renderer records how it wants the model posed.
    pub(super) view: Option<&'a Cell<Option<PreviewView>>>,
    pub(super) control: Option<&'a Cell<Option<PreviewControl>>>,
    pub(super) rotation: f32,
}

impl CustomRenderer for PreviewRenderer<'_> {
    fn paint(
        &mut self,
        node: &CustomDraw<'_>,
        canvas: &mut Canvas<'_>,
    ) -> Result<bool, PaintError> {
        if !Self::handles(node.renderer) {
            return Ok(false);
        }
        if let Some((visual, bounds)) = self.player_preview(canvas, node) {
            canvas.push(visual, bounds)?;
        }
        Ok(true)
    }
}

impl PreviewRenderer<'_> {
    /// The player-model custom controls handled by this shared renderer.
    pub(in crate::ui_runtime::presentation) fn handles(renderer: &str) -> bool {
        matches!(
            renderer,
            "live_player_renderer" | "paper_doll_renderer" | "hud_player_renderer"
        )
    }
    /// The preview raster for `node`; the pose is requested for the next raster.
    fn player_preview(
        &self,
        canvas: &Canvas<'_>,
        node: &CustomDraw<'_>,
    ) -> Option<(UiVisual, [f32; 4])> {
        let preview = self.preview?;
        let (data, dest) = (node.data, node.dest);
        let (view, frame) = crate::ui_runtime::presentation::player_preview::renderer_frame(
            node.renderer,
            data,
            dest,
            canvas.px(),
            canvas.pointer(),
        );
        let view = view.with_menu_rotation(self.rotation);
        if let Some(request) = self.view {
            request.set(Some(view));
        }
        if let Some(control) = self.control
            && data.get("rotation").and_then(Value::as_str) == Some("gesture_x")
        {
            control.set(Some(PreviewControl {
                bounds: crate::ui_runtime::presentation::rect(dest[0], dest[1], dest[2], dest[3])
                    .ok()?,
                gui_pixel: canvas.px(),
            }));
        }
        Some((
            UiVisual::Sprite {
                texture_page: preview.page,
                uv: preview.uv,
                color: node.alpha([255; 4]),
            },
            frame,
        ))
    }
}
