//! Server toasts through vanilla `toast_screen.toast_screen`: the showing toast
//! is the `toast_factory`'s `popup`, as 26.30 creates one for a ToastRequest.
//! The popup's offset animation (from above the top edge down 32 px and back)
//! is evaluated here and handed over as its offset.

use std::sync::Arc;

use json_ui::{DataSource, FactoryItem, Scalar};
use serde_json::{Value, json};
use ui::UiNode;

use super::super::{
    FONT_DESIGN_PIXEL_TEXELS, TextMetrics, UiPresentationError, UiPresentationRuntime,
    bounded_visible_text,
};
use super::engine::{EngineInputs, EngineOutput, ScreenArt};
use crate::ui_runtime::UiRuntime;

pub const TOAST_SCREEN: &str = "toast_screen.toast_screen";
/// How far the popup slides down from above the top edge.
const TOAST_DISTANCE: f64 = 32.0;

impl UiPresentationRuntime {
    /// Draw the showing server toast, if any.
    pub(in super::super) fn append_toast_screen(
        &mut self,
        runtime: &UiRuntime,
        nodes: &mut Vec<UiNode>,
        next: &mut u32,
        metrics: TextMetrics,
        content: [f32; 2],
        now_millis: u64,
    ) -> Result<(), UiPresentationError> {
        let Some(renderer) = self.form_presentation.engine.as_deref() else {
            return Ok(());
        };
        let Some(toast) = runtime
            .hud()
            .toasts()
            .iter()
            .find(|toast| toast.visible_at(now_millis))
        else {
            return Ok(());
        };
        let data = toast_data(toast, now_millis);
        // Vanilla toast screen variables.
        let context = renderer
            .context()
            .clone()
            .with_var("popup_size", json!(["100% - 50px", 32]));
        let catalog = Arc::clone(renderer.catalog());
        let px = metrics.scale.get() * FONT_DESIGN_PIXEL_TEXELS as f32;
        let translate = |key: &str| runtime.translation(key);
        let inputs = EngineInputs {
            layouts: &mut self.layouts,
            font: &self.font,
            metrics,
            solid_page: self.solid_texture_page,
            safe_area: self.safe_area,
            content,
            translate: &translate,
            language: runtime.text_generation(),
        };
        let out = EngineOutput {
            nodes,
            next,
            overlay: &[],
        };
        let screen = &mut self.form_presentation.hud.toast;
        renderer.draw(
            ScreenArt {
                now: self.menu_seconds,
                ..ScreenArt::default()
            },
            inputs,
            out,
            |env, root| {
                screen.render_with(
                    TOAST_SCREEN,
                    &catalog,
                    &context,
                    data,
                    (root, px, runtime.text_generation()),
                    env,
                    &json_ui::ViewState::default(),
                )
            },
        )?;
        Ok(())
    }
}

/// What the toast controller binds for `toast` at `now_millis`.
fn toast_data(toast: &ui::Toast, now_millis: u64) -> DataSource {
    let mut data = DataSource::new();
    data.set_strict(true);
    let title = bounded_visible_text(&toast.title).to_owned();
    let subtitle = bounded_visible_text(&toast.message).to_owned();
    data.set_global(
        "#toast_subtitle_visible",
        Scalar::Bool(!subtitle.is_empty()),
    );
    data.set_global("#toast_title", Scalar::Text(title));
    data.set_global("#toast_subtitle", Scalar::Text(subtitle));
    data.set_global("#toast_icon_section_content", Scalar::Num(0.0));
    let offset = TOAST_DISTANCE * f64::from(toast.slide(now_millis));
    data.set_factory(
        "toast_factory",
        vec![
            FactoryItem::new("popup", toast.started_millis as f64 / 1_000.0)
                .named("popup")
                .var("toast_offset", json!([0.0, offset]))
                .var("offset_anims", Value::Array(Vec::new())),
        ],
    );
    data
}

#[cfg(test)]
impl UiPresentationRuntime {
    /// The toast screen's last laid-out draw nodes, in virtual px.
    pub fn toast_draw_nodes(&self) -> &[json_ui::DrawNode] {
        self.form_presentation.hud.toast.nodes()
    }
}
