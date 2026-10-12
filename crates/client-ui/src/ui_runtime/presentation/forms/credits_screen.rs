//! The End credits: an engine-drawn screen whose text scrolls through its own renderer.

use std::{
    cell::Cell,
    collections::BTreeMap,
    sync::{Arc, Mutex, OnceLock},
};

use json_ui::{DataSource, Scalar, ViewState};
use ui::UiNode;

use super::credits_content::Content;
use super::credits_renderer::{CreditsRenderer, MeasuredCredits};
use super::engine::{
    FormEngine, ScreenArt, rect,
    screen::{CustomRenderer, PaintError, PaintFrame, Screen, ScreenOutput, ScreenRegistry},
};
use crate::ui_runtime::{UiRuntime, credits::CREDITS_SCREEN};
use {
    super::super::{TextMetrics, UiPresentationError, UiPresentationRuntime},
    ui::FONT_DESIGN_PIXEL_TEXELS,
};

#[derive(Default)]
pub(super) struct CreditsScreen {
    screen: super::hud::CachedScreen,
    finished: bool,
    identity: Option<(u64, u64)>,
    layout: Mutex<Option<Arc<MeasuredCredits>>>,
    /// The credits files, read once per applied server pack.
    content: OnceLock<Arc<Content>>,
}

/// What one credits paint reads.
pub(super) struct CreditsInput<'a> {
    pub(super) engine: &'a FormEngine,
    /// The session and sequence the credits belong to.
    pub(super) identity: (u64, u64),
    pub(super) player_name: &'a str,
    pub(super) scroll_pixels: f64,
    pub(super) skip_visible: bool,
    pub(super) cover_alpha: f32,
    pub(super) clocks: &'a BTreeMap<String, f64>,
}

/// The credits' only pointer target.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum CreditsAction {
    Skip,
}

impl CreditsScreen {
    /// Reads the credits files again after the applied server pack changes.
    pub(super) fn forget_content(&mut self) {
        self.content = OnceLock::new();
    }
}

impl Screen for CreditsScreen {
    type Input<'a> = CreditsInput<'a>;
    type Action = CreditsAction;

    fn register(registry: &mut ScreenRegistry) {
        registry.catalog_mut().extend(extend_catalog);
    }

    fn paint(
        &mut self,
        input: CreditsInput<'_>,
        frame: &mut PaintFrame<'_>,
    ) -> Result<ScreenOutput<CreditsAction>, PaintError> {
        if self.identity != Some(input.identity) {
            *self = CreditsScreen {
                identity: Some(input.identity),
                content: std::mem::take(&mut self.content),
                ..Default::default()
            };
        }
        let engine = input.engine;
        let content = Arc::clone(self.content.get_or_init(|| read_content(engine)));
        let finished = Cell::new(false);
        let mut data = DataSource::new();
        data.set_strict(true);
        data.set_global("#show_end_poem", Scalar::Bool(true));
        data.set_global("#show_edu_icon", Scalar::Bool(false));
        data.set_global("#player_name", Scalar::Text(input.player_name.to_owned()));
        data.set_global("#scroll_faster", Scalar::Bool(false));
        data.set_global("#skip_button_visible", Scalar::Bool(input.skip_visible));
        data.set_global(
            "#credits_cover_alpha",
            Scalar::Num(f64::from(input.cover_alpha)),
        );
        let context = engine.context().clone();
        let catalog = Arc::clone(engine.catalog());
        let px = frame.metrics.scale.get() * FONT_DESIGN_PIXEL_TEXELS as f32;
        let language = frame.language;
        let view = ViewState::default();
        let mut renderer = CreditsRenderer {
            content,
            player_name: input.player_name,
            scroll_pixels: input.scroll_pixels,
            finished: &finished,
            layout: &self.layout,
        };
        let mut renderers: [&mut dyn CustomRenderer; 1] = [&mut renderer];
        let screen = &mut self.screen;
        let drawn = engine.draw(
            ScreenArt {
                clocks: Some(input.clocks),
                ..ScreenArt::default()
            },
            &mut frame.with_renderers(&mut renderers),
            |env, root| {
                screen.render_with(
                    CREDITS_SCREEN,
                    &catalog,
                    &context,
                    data,
                    (root, px, language),
                    env,
                    &view,
                )
            },
        )?;
        self.finished = finished.get();
        let mut hits = Vec::new();
        if let Some(drawn) = &drawn {
            for region in drawn
                .hits
                .iter()
                .filter(|hit| hit.pressed.as_deref() == Some("button.menu_exit"))
            {
                hits.push((
                    CreditsAction::Skip,
                    rect(
                        drawn.origin[0] + region.rect.x as f32 * px,
                        drawn.origin[1] + region.rect.y as f32 * px,
                        drawn.origin[0] + (region.rect.x + region.rect.w) as f32 * px,
                        drawn.origin[1] + (region.rect.y + region.rect.h) as f32 * px,
                    )?,
                ));
            }
        }
        Ok(ScreenOutput {
            hits,
            focus: None,
            frame: drawn,
        })
    }
}

/// The credits files from the applied server pack, else the carrier.
fn read_content(engine: &FormEngine) -> Arc<Content> {
    let content = Content::read(|path| {
        engine
            .server_source()
            .and_then(|pack| pack.view.as_ref())
            .and_then(|view| view.read_capped(path, assets::MAX_UI_FILE_BYTES as u64))
            .map(Vec::from)
            .or_else(|| engine.assets().ui_file(path).map(<[u8]>::to_vec))
    });
    if content.missing {
        bevy::log::warn!(
            "credits runtime files are missing from the UI carrier or pack; rebuild UI assets from a pack containing credits/end.txt, credits/credits.json and credits/quote.txt"
        );
    }
    Arc::new(content)
}

impl UiPresentationRuntime {
    pub fn credits_finished(&self, session: u64, sequence: u64) -> bool {
        let screen = &self.screens.credits;
        screen.identity == Some((session, sequence)) && screen.finished
    }

    pub fn credits_skip_contains(&self, session: u64, sequence: u64, point: [f32; 2]) -> bool {
        self.screens.credits.identity == Some((session, sequence))
            && self.screens.credits_hits.iter().any(|(_, rect)| {
                point[0] >= rect.min().x()
                    && point[0] <= rect.max().x()
                    && point[1] >= rect.min().y()
                    && point[1] <= rect.max().y()
            })
    }

    /// Paints the active credits; `false` when none are active or the engine drew nothing.
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
        let Some(engine) = self.engine.as_deref() else {
            return Ok(false);
        };
        let translate = |key: &str| runtime.translation(key);
        let input = CreditsInput {
            engine,
            identity: (runtime.session_id(), active.sequence),
            player_name: runtime.credits_player_name(),
            scroll_pixels: active.scroll_pixels(),
            skip_visible: active.skip_visible(now),
            cover_alpha: active.cover_alpha(now),
            clocks: &self.scene_clock,
        };
        let mut frame = PaintFrame {
            layouts: &mut self.layouts,
            font: &self.font,
            metrics,
            solid_page: self.solid_texture_page,
            safe_area: self.safe_area,
            content,
            translate: &translate,
            language: runtime.text_generation(),
            now: self.menu_seconds,
            nodes,
            next,
            renderers: &mut [],
        };
        // New credits start without the previous run's skip regions, even if painting fails.
        if self.screens.credits.identity != Some(input.identity) {
            self.screens.credits_hits.clear();
        }
        let output = self.screens.credits.paint(input, &mut frame)?;
        self.screens.credits_hits = output.hits;
        Ok(output.frame.is_some())
    }
}

/// Supplies the native factory controls from the host's lifecycle bindings.
fn extend_catalog(catalog: &mut json_ui::Catalog) {
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
