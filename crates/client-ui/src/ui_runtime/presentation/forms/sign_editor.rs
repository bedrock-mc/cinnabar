//! The sign editor through vanilla `sign.sign_screen`: the wood's sign art
//! behind a multiline edit box holding the four lines with a caret, over an
//! input panel that closes the editor when pressed outside the sign.

use std::sync::Arc;

use json_ui::{DataSource, HitRegion, Scalar, ViewState};
use serde_json::Value;
use ui::{UiNode, UiPoint, UiRect};

use super::engine::{EngineInputs, EngineOutput, ScreenArt};
use super::hud::CachedScreen;
use super::menus::window_rect;
use crate::ui_runtime::{UiRuntime, sign_editor::SignEdit};
use {
    super::super::{TextMetrics, UiPresentationError, UiPresentationRuntime},
    ui::FONT_DESIGN_PIXEL_TEXELS,
};

pub const SIGN_SCREEN: &str = "sign.sign_screen";
/// The edit box caret's on and off time.
const CARET_BLINK_MILLIS: u64 = 500;

/// The sign screen's cached layout and last frame's pressable regions.
#[derive(Default)]
pub(super) struct SignScreen {
    screen: CachedScreen,
    /// Window-logical rects, bottom to top, with where each routes when pressed.
    hits: Vec<(UiRect, Option<String>)>,
}

impl UiPresentationRuntime {
    /// Whether a press at `point` lands outside the sign, which closes the editor.
    pub fn sign_editor_exit_hit(&self, point: UiPoint) -> bool {
        self.form_presentation
            .sign
            .hits
            .iter()
            .rev()
            .find(|(bounds, _)| bounds.contains(point))
            .is_some_and(|(_, pressed)| pressed.as_deref() == Some("button.menu_exit"))
    }

    /// Drops the hidden editor's hit regions.
    pub(in crate::ui_runtime::presentation) fn hide_sign_editor(&mut self) {
        self.form_presentation.sign.hits.clear();
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::ui_runtime::presentation) fn append_sign_editor(
        &mut self,
        runtime: &UiRuntime,
        nodes: &mut Vec<UiNode>,
        next: &mut u32,
        metrics: TextMetrics,
        width: f32,
        height: f32,
        now_millis: u64,
    ) -> Result<(), UiPresentationError> {
        self.form_presentation.sign.hits.clear();
        let Some(edit) = runtime.sign_editor().active() else {
            return Ok(());
        };
        let Some(renderer) = self.form_presentation.engine.as_deref() else {
            return Ok(());
        };
        let look = edit.look();
        let multiline = if look.hanging {
            "sign.hanging_sign_text_multiline"
        } else {
            "sign.regular_sign_text_multiline"
        };
        // Vanilla sign screen variables: the wood's art and edit box.
        let context = renderer
            .context()
            .clone()
            .with_var("sign_texture", Value::from(look.texture.clone()))
            .with_var("sign_text_multiline", Value::from(multiline));
        let data = sign_data(edit, now_millis);
        let catalog = Arc::clone(renderer.catalog());
        let px = metrics.scale.get() * FONT_DESIGN_PIXEL_TEXELS as f32;
        let translate = |key: &str| runtime.translation(key);
        let inputs = EngineInputs {
            layouts: &mut self.layouts,
            font: &self.font,
            metrics,
            solid_page: self.solid_texture_page,
            safe_area: self.safe_area,
            content: [width, height],
            translate: &translate,
            language: runtime.text_generation(),
        };
        let out = EngineOutput {
            nodes,
            next,
            overlay: &[],
        };
        let sign = &mut self.form_presentation.sign;
        let view = ViewState::default();
        let frame = renderer.draw(
            ScreenArt {
                now: self.menu_seconds,
                clocks: Some(&self.scene_clock),
                ..ScreenArt::default()
            },
            inputs,
            out,
            |env, root| {
                sign.screen.render_with(
                    SIGN_SCREEN,
                    &catalog,
                    &context,
                    data,
                    (root, px, runtime.text_generation()),
                    env,
                    &view,
                )
            },
        )?;
        if let Some(frame) = frame {
            sign.hits = frame
                .hits
                .iter()
                .filter_map(|region: &HitRegion| {
                    let bounds = window_rect(region, frame.scale, frame.origin)?;
                    Some((bounds, region.pressed.clone()))
                })
                .collect();
        }
        Ok(())
    }
}

/// What the sign controller binds: the lines with a caret, in the sign's colour.
fn sign_data(edit: &SignEdit, now_millis: u64) -> DataSource {
    let mut data = DataSource::new();
    data.set_strict(true);
    let (cursor_line, cursor_column) = edit.cursor();
    let caret = (now_millis / CARET_BLINK_MILLIS).is_multiple_of(2);
    let lines: Vec<String> = edit
        .lines()
        .iter()
        .enumerate()
        .map(|(index, line)| {
            if index != cursor_line || !caret {
                return line.clone();
            }
            let at = line
                .char_indices()
                .nth(cursor_column)
                .map_or(line.len(), |(at, _)| at);
            format!("{}|{}", &line[..at], &line[at..])
        })
        .collect();
    data.set_global("#sign_text", Scalar::Text(lines.join("\n")));
    let [r, g, b, _] = edit.color();
    data.set_global(
        "#edit_box_text_color",
        Scalar::Text(format!("#{r:02x}{g:02x}{b:02x}")),
    );
    data.set_global("#close_button_visible", Scalar::Bool(true));
    data
}

#[cfg(test)]
impl UiPresentationRuntime {
    /// The sign screen's last laid-out draw nodes, in virtual px.
    pub fn sign_draw_nodes(&self) -> &[json_ui::DrawNode] {
        self.form_presentation.sign.screen.nodes()
    }
}
