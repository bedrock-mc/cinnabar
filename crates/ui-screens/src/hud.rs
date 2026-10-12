//! The gameplay HUD through the JSON-UI engine: the frame's [`HudModel`] binds to
//! `hud.hud_screen` and the crosshair overlay over the session's pack stack (the built-in Java
//! HUD pack at the bottom). The bound and laid-out screen is reused until the model, catalog,
//! viewport, or scale changes; each frame only repaints it, evaluating fades and the native
//! renderers against the live state.

use std::{collections::BTreeMap, sync::Arc};

use json_ui::{
    CROSSHAIR_SCREEN, Catalog, Context, DataSource, HUD_SCREEN, HudModel, hud_clocks,
    hud_data_source,
};
use ui::{FONT_DESIGN_PIXEL_TEXELS, IconRef};
use ui_engine::{
    cached_screen::CachedScreen,
    engine::{
        FormEngine, ScreenArt,
        item_renderer::ItemRenderer,
        pack_catalog::CarrierRestore,
        screen::{CustomRenderer, PaintError, PaintFrame, ScreenRegistry},
    },
};

use crate::hud_renderers::{HudPaint, HudRenderer, HungerAnimation};

mod chat;
pub mod prepared;
mod titles;
#[cfg(test)]
mod visibility_tests;

/// The built-in Java-styled HUD pack: `(pack path, namespace, bytes)`, layered
/// under every server pack.
pub const JAVA_HUD_PACK: [(&str, &str, &[u8]); 4] = [
    (
        "ui/_global_variables.json",
        "",
        include_bytes!("../../../assets/java-hud/ui/_global_variables.json"),
    ),
    (
        "ui/chat_screen.json",
        "chat",
        include_bytes!("../../../assets/java-hud/ui/chat_screen.json"),
    ),
    (
        "ui/hud_screen.json",
        "hud",
        include_bytes!("../../../assets/java-hud/ui/hud_screen.json"),
    ),
    (
        "ui/scoreboards.json",
        "scoreboard",
        include_bytes!("../../../assets/java-hud/ui/scoreboards.json"),
    ),
];

/// Layers the built-in Java HUD pack and the rules server HUD layers follow over it.
pub fn register(registry: &mut ScreenRegistry) {
    let catalog = registry.catalog_mut();
    catalog.layer_builtin(install_java_hud);
    catalog.restore_before(CarrierRestore {
        authored: titles::authored,
        restore: titles::restore,
    });
    catalog.after_layers(chat::prefer_builtin);
}

fn install_java_hud(catalog: &mut Catalog) {
    catalog.apply_pack(JAVA_HUD_PACK.iter().map(|(path, _, bytes)| (*path, *bytes)));
}

/// The HUD and crosshair screens, carried across frames.
#[derive(Default)]
pub struct HudScreens {
    pub hud: CachedScreen,
    /// The crosshair overlay screen.
    pub crosshair: CachedScreen,
    /// The toast screen, drawn above everything in game.
    pub toast: CachedScreen,
    /// Where the showing toast takes presses and what they open, when it opens anything.
    pub toast_press: Option<(ui::ToastPress, ui::UiRect)>,
    /// The world-loading screen shown while joining.
    pub loading: CachedScreen,
    /// This frame's fade clocks (title, action bar, item name).
    clocks: BTreeMap<String, f64>,
    model: Option<HudModel>,
    opacity: Option<i32>,
    data: Arc<DataSource>,
    hunger_animation: HungerAnimation,
}

/// What one HUD paint reads.
pub struct HudInput<'a> {
    /// The crosshair overlay rather than the HUD.
    pub crosshair: bool,
    pub catalog: &'a Arc<Catalog>,
    /// The HUD context with the chat placement.
    pub context: &'a Context,
    /// What [`HudScreens::begin`] returned.
    pub data: Arc<DataSource>,
    pub paint: &'a HudPaint,
    /// Icons the model's item cells index.
    pub icons: &'a [IconRef],
}

impl HudScreens {
    /// Preserve bindings while relaying out screens for a changed texture pack.
    pub fn invalidate_textures(&mut self) {
        self.hud.invalidate_textures();
        self.crosshair.invalidate_textures();
        self.toast.invalidate_textures();
        self.loading.invalidate_textures();
    }

    /// Starts a frame: binds `model` at `opacity` percent (none for the crosshair overlay),
    /// starts this frame's fade clocks from `scene_clock`, and resets hunger motion for a new
    /// session or catalog. Returns the data the screen binds.
    pub fn begin(
        &mut self,
        model: Option<(HudModel, i32)>,
        scene_clock: &BTreeMap<String, f64>,
        session: u64,
        catalog: &Arc<Catalog>,
    ) -> Arc<DataSource> {
        let data = match model {
            None => Arc::default(),
            Some((model, opacity)) => {
                self.clocks = hud_clocks(&model);
                if self.model.as_ref() != Some(&model) || self.opacity != Some(opacity) {
                    let mut data = hud_data_source(&model);
                    data.set_global(
                        "#hud_alpha",
                        json_ui::Scalar::Num(f64::from(opacity) / 100.0),
                    );
                    data.set_global("#hud_propagate_alpha", json_ui::Scalar::Bool(true));
                    self.data = Arc::new(data);
                    self.opacity = Some(opacity);
                    self.model = Some(model);
                }
                Arc::clone(&self.data)
            }
        };
        self.clocks.extend(scene_clock.clone());
        self.hunger_animation.begin(session, catalog);
        data
    }

    /// Paints the HUD screen, or its crosshair overlay; `frame`'s renderers draw ahead of the
    /// HUD's native and item renderers.
    pub fn paint(
        &mut self,
        engine: &FormEngine,
        input: HudInput<'_>,
        frame: &mut PaintFrame<'_>,
    ) -> Result<(), PaintError> {
        let HudInput {
            crosshair,
            catalog,
            context,
            data,
            paint,
            icons,
        } = input;
        let reference = if crosshair {
            CROSSHAIR_SCREEN
        } else {
            HUD_SCREEN
        };
        let px = frame.metrics.scale.get() * FONT_DESIGN_PIXEL_TEXELS as f32;
        let language = frame.language;
        let hunger_animation = std::cell::RefCell::new(&mut self.hunger_animation);
        let advance_hunger =
            |key: &str, instance| hunger_animation.borrow_mut().advance(key, instance);
        let art = ScreenArt {
            clocks: Some(&self.clocks),
            ..ScreenArt::default()
        };
        let mut hud_renderer = HudRenderer {
            hud: paint,
            hunger_update: Some(&advance_hunger),
        };
        let mut items = ItemRenderer { icons, id_aux: &[] };
        let screen = if crosshair {
            &mut self.crosshair
        } else {
            &mut self.hud
        };
        let caller = std::mem::take(&mut frame.renderers);
        let drawn = {
            let mut renderers: Vec<&mut dyn CustomRenderer> = Vec::with_capacity(caller.len() + 2);
            for renderer in caller.iter_mut() {
                renderers.push(&mut **renderer);
            }
            renderers.push(&mut hud_renderer);
            renderers.push(&mut items);
            let mut paint_frame = frame.with_renderers(&mut renderers);
            engine.draw(art, &mut paint_frame, |env, root| {
                screen.render(reference, catalog, context, data, (root, px, language), env)
            })
        };
        frame.renderers = caller;
        drawn.map(|_| ())
    }

    /// This frame's fade clocks.
    #[cfg(any(test, feature = "test-support"))]
    pub fn clocks(&self) -> &BTreeMap<String, f64> {
        &self.clocks
    }
}
