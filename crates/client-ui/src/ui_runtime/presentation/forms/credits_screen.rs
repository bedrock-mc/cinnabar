use std::{
    cell::Cell,
    sync::{Arc, Mutex},
};

use json_ui::{DataSource, Scalar, ViewState};
use ui::{UiNode, UiRect};

use super::super::{
    FONT_DESIGN_PIXEL_TEXELS, TextMetrics, UiPresentationError, UiPresentationRuntime,
};
use super::engine::{EngineInputs, EngineOutput, ScreenArt};
use crate::ui_runtime::{UiRuntime, credits::CREDITS_SCREEN};

#[derive(Default)]
pub(super) struct CreditsScreen {
    screen: super::hud::CachedScreen,
    pub(super) finished: bool,
    pub(super) hits: Vec<UiRect>,
    identity: Option<(u64, u64)>,
    layout: Arc<Mutex<Option<Arc<super::engine::credits_renderer::MeasuredCredits>>>>,
}

pub(super) struct CreditsPaint {
    pub(super) content: Arc<super::credits_content::Content>,
    pub(super) player_name: String,
    pub(super) scroll_pixels: f64,
    pub(super) finished: Cell<bool>,
    pub(super) layout: Arc<Mutex<Option<Arc<super::engine::credits_renderer::MeasuredCredits>>>>,
}

impl UiPresentationRuntime {
    pub fn credits_finished(&self, session: u64, sequence: u64) -> bool {
        let screen = &self.form_presentation.credits;
        screen.identity == Some((session, sequence)) && screen.finished
    }

    pub fn credits_skip_contains(&self, session: u64, sequence: u64, point: [f32; 2]) -> bool {
        let screen = &self.form_presentation.credits;
        screen.identity == Some((session, sequence))
            && screen.hits.iter().any(|rect| {
                point[0] >= rect.min().x()
                    && point[0] <= rect.max().x()
                    && point[1] >= rect.min().y()
                    && point[1] <= rect.max().y()
            })
    }

    pub(in super::super) fn append_credits_screen(
        &mut self,
        runtime: &UiRuntime,
        nodes: &mut Vec<UiNode>,
        next: &mut u32,
        metrics: TextMetrics,
        content: [f32; 2],
        now: u64,
    ) -> Result<bool, UiPresentationError> {
        let Some(active) = runtime.credits().active() else {
            return Ok(false);
        };
        let Some(renderer) = self.form_presentation.engine.as_deref() else {
            return Ok(false);
        };
        let screen = &mut self.form_presentation.credits;
        let identity = (runtime.session_id(), active.sequence);
        if screen.identity != Some(identity) {
            *screen = CreditsScreen {
                identity: Some(identity),
                ..Default::default()
            };
        }
        let paint = CreditsPaint {
            content: renderer.credits_content(),
            player_name: runtime.credits_player_name().to_owned(),
            scroll_pixels: active.scroll_pixels(),
            finished: Cell::new(false),
            layout: Arc::clone(&screen.layout),
        };
        let mut data = DataSource::new();
        data.set_strict(true);
        data.set_global("#show_end_poem", Scalar::Bool(true));
        data.set_global("#show_edu_icon", Scalar::Bool(false));
        data.set_global("#player_name", Scalar::Text(paint.player_name.clone()));
        data.set_global("#scroll_faster", Scalar::Bool(false));
        data.set_global(
            "#skip_button_visible",
            Scalar::Bool(active.skip_visible(now)),
        );
        data.set_global(
            "#credits_cover_alpha",
            Scalar::Num(f64::from(active.cover_alpha(now))),
        );
        let context = renderer.context().clone();
        let catalog = Arc::clone(renderer.catalog());
        let translate = |key: &str| runtime.translation(key);
        let px = metrics.scale.get() * FONT_DESIGN_PIXEL_TEXELS as f32;
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
        let view = ViewState::default();
        let frame = renderer.draw(
            ScreenArt {
                credits: Some(&paint),
                now: self.menu_seconds,
                clocks: Some(&self.scene_clock),
                ..ScreenArt::default()
            },
            inputs,
            out,
            |env, root| {
                screen.screen.render_with(
                    CREDITS_SCREEN,
                    &catalog,
                    &context,
                    data,
                    (root, px, runtime.text_generation()),
                    env,
                    &view,
                )
            },
        )?;
        screen.finished = paint.finished.get();
        screen.hits.clear();
        if let Some(frame) = &frame {
            for region in frame
                .hits
                .iter()
                .filter(|hit| hit.pressed.as_deref() == Some("button.menu_exit"))
            {
                screen.hits.push(super::super::rect(
                    frame.origin[0] + region.rect.x as f32 * px,
                    frame.origin[1] + region.rect.y as f32 * px,
                    frame.origin[0] + (region.rect.x + region.rect.w) as f32 * px,
                    frame.origin[1] + (region.rect.y + region.rect.h) as f32 * px,
                )?);
            }
        }
        Ok(frame.is_some())
    }
}

/// Supplies the native factory controls from the host's lifecycle bindings.
pub(super) fn extend_catalog(catalog: &mut json_ui::Catalog) {
    catalog.overlay_text("ui/cinnabar_credits_lifecycle.json", r##"{
      "namespace": "credits",
      "fade_in_image": {
        "anims": [],
        "bindings": [{ "binding_name": "#credits_cover_alpha", "binding_name_override": "#alpha" }]
      },
      "skip_panel": {
        "modifications": [{ "array_name": "controls", "operation": "replace", "control_name": "skip_button", "value": [{
          "skip_button@common_buttons.light_text_form_fitting_button": {
            "$pressed_button_name": "button.menu_exit", "$button_text": "credits.skip",
            "anchor_from": "bottom_right", "anchor_to": "bottom_right", "offset": [-8, -8],
            "bindings": [{ "binding_name": "#skip_button_visible", "binding_name_override": "#visible", "binding_type": "global" }]
          }
        }]}]
      },
      "credits_screen_content": {
        "modifications": [{ "array_name": "controls", "operation": "replace", "control_name": "credits_factory", "value": [{
          "credits_factory@credits.skip_panel": {}
        }]}]
      }
    }"##);
}
