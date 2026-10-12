//! The End credits: an engine-drawn screen whose text scrolls through its own renderer.

use std::{
    cell::Cell,
    collections::BTreeMap,
    sync::{Arc, Mutex, OnceLock},
};

use json_ui::{DataSource, Scalar, ViewState};
use ui::FONT_DESIGN_PIXEL_TEXELS;

use crate::credits_content::Content;
use crate::credits_renderer::{CreditsRenderer, MeasuredCredits};
use ui_engine::engine::{
    FormEngine, ScreenArt, rect,
    screen::{CustomRenderer, PaintError, PaintFrame, Screen, ScreenOutput, ScreenRegistry},
};

pub const CREDITS_SCREEN: &str = "credits.credits_screen";

/// The End credits' cached screen, run state and content.
#[derive(Default)]
pub struct CreditsScreen {
    screen: ui_engine::cached_screen::CachedScreen,
    finished: bool,
    identity: Option<(u64, u64)>,
    layout: Mutex<Option<Arc<MeasuredCredits>>>,
    /// The credits files, read once per applied server pack.
    content: OnceLock<Arc<Content>>,
}

/// What one credits paint reads.
pub struct CreditsInput<'a> {
    pub engine: &'a FormEngine,
    /// The applied server pack's files, which override the carrier's.
    pub pack: Option<&'a resource_pack::LayeredPackView>,
    /// The session and sequence the credits belong to.
    pub identity: (u64, u64),
    pub player_name: &'a str,
    pub scroll_pixels: f64,
    pub skip_visible: bool,
    pub cover_alpha: f32,
    pub clocks: &'a BTreeMap<String, f64>,
}

/// The credits' only pointer target.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CreditsAction {
    Skip,
}

impl CreditsScreen {
    /// Reads the credits files again after the applied server pack changes.
    pub fn forget_content(&mut self) {
        self.content = OnceLock::new();
    }

    /// Whether these are the credits of `identity`.
    pub fn shows(&self, identity: (u64, u64)) -> bool {
        self.identity == Some(identity)
    }

    /// Whether the credits of `identity` scrolled to their end.
    pub fn finished(&self, identity: (u64, u64)) -> bool {
        self.shows(identity) && self.finished
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
        let content = Arc::clone(
            self.content
                .get_or_init(|| read_content(engine, input.pack)),
        );
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
fn read_content(
    engine: &FormEngine,
    pack: Option<&resource_pack::LayeredPackView>,
) -> Arc<Content> {
    let content = Content::read(|path| {
        pack.and_then(|view| view.read_capped(path, assets::MAX_UI_FILE_BYTES as u64))
            .map(Vec::from)
            .or_else(|| engine.assets().ui_file(path).map(<[u8]>::to_vec))
    });
    if content.missing {
        tracing::warn!(
            "credits runtime files are missing from the UI carrier or pack; rebuild UI assets from a pack containing credits/end.txt, credits/credits.json and credits/quote.txt"
        );
    }
    Arc::new(content)
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
