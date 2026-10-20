//! The gameplay HUD through the JSON-UI engine: the frame's [`HudModel`] binds to
//! `hud.hud_screen` and the crosshair overlay over the session's pack stack (the built-in Java
//! HUD pack at the bottom). The bound and laid-out screen is reused until the model, catalog,
//! viewport, or scale changes; each frame only repaints it, evaluating fades and the native
//! renderers against the live state.

use json_ui::vanilla::Reference;
use std::{collections::BTreeMap, num::NonZeroU64, sync::Arc};

use json_ui::{Catalog, Context, DataSource, HudBindings, HudChanges, HudModel, hud_clocks};
use ui::{FONT_DESIGN_PIXEL_TEXELS, IconRef};
use ui_engine::{
    cached_screen::CachedScreen,
    engine::{
        FormEngine, PaintBounds, PaintObserver, ScreenArt,
        item_renderer::ItemRenderer,
        pack_catalog::CarrierRestore,
        screen::{CustomRenderer, PaintError, PaintFrame, ScreenRegistry},
    },
    frame::EngineFrame,
};

use crate::hud_renderers::{HudPaint, HudRenderer, HungerAnimation};

mod chat;
#[cfg(test)]
mod color_tests;
#[cfg(test)]
mod crisp_tests;
#[cfg(test)]
mod image_tests;
pub mod prepared;
#[cfg(test)]
mod retention_tests;
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

/// The built-in Java HUD pack's original images, by extensionless pack path.
pub const JAVA_HUD_IMAGES: [(&str, &[u8]); 3] = [
    (
        crate::player_list::CINNABAR_BADGE,
        crate::player_list::CINNABAR_BADGE_PNG,
    ),
    (
        "textures/ui/hotbar_offhand_left",
        include_bytes!("../../../assets/java-hud/textures/ui/hotbar_offhand_left.png"),
    ),
    (
        "textures/ui/hotbar_offhand_right",
        include_bytes!("../../../assets/java-hud/textures/ui/hotbar_offhand_right.png"),
    ),
];

/// Layers the built-in Java HUD pack and the rules server HUD layers follow over it.
pub fn register(registry: &mut ScreenRegistry) {
    for (path, bytes) in JAVA_HUD_IMAGES {
        registry.builtin_image(path, bytes);
    }
    let catalog = registry.catalog_mut();
    catalog.layer_builtin(install_java_hud);
    catalog.restore_before(CarrierRestore {
        authored: titles::authored,
        restore: titles::restore,
    });
    catalog.after_layers(chat::prefer_builtin);
    catalog.after_layers(mark_bottom_region);
}

/// Installs the original HUD artwork and marks its resolved bottom subtree.
fn install_java_hud(catalog: &mut Catalog) {
    catalog.apply_pack(JAVA_HUD_PACK.iter().map(|(path, _, bytes)| (*path, *bytes)));
    mark_bottom_region(catalog);
}

/// Tags the native root after pack layering so aliases and inherited children retain its scope.
fn mark_bottom_region(catalog: &mut Catalog) {
    if let Some(root) = catalog.lookup_mut("hud", BOTTOM_HUD_ROOT) {
        root.properties.insert(
            json_ui::transform::PAINT_REGION_PROPERTY.into(),
            serde_json::json!(BOTTOM_HUD_REGION.get()),
        );
    }
}

/// The HUD and crosshair screens, carried across frames.
#[derive(Default)]
pub struct HudScreens {
    pub hud: CachedScreen,
    /// The last native bottom-HUD paint bounds, `[x, y, width, height]` in shared GUI pixels.
    pub editor_keep_out: Option<[f64; 4]>,
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
    opacity: Option<i32>,
    bindings: HudBindings,
    /// Presentation color kept separate from the factory and collection message inputs.
    chat_color: Option<char>,
    crosshair_data: Arc<DataSource>,
    model_clocks: BTreeMap<String, f64>,
    hunger_animation: HungerAnimation,
    /// The HUD context, kept while the engine context and chat placement stay.
    context: crate::chat_position::PlacedContext,
}

/// What one HUD paint reads.
pub struct HudInput<'a> {
    /// The crosshair overlay rather than the HUD.
    pub crosshair: bool,
    /// Observe actual native bottom-HUD painting while the host's layout editor is open.
    pub capture_keep_out: bool,
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

    /// The HUD screens' context: `base` with the HUD flags and the chat at the top or bottom.
    /// It is rebuilt only when `base` or the placement changes.
    pub fn context(&mut self, base: &Context, chat_top: bool) -> Context {
        self.context.get(base, chat_top, |base, top| {
            crate::chat_position::placed(json_ui::hud_context(&base), top)
        })
    }

    /// Binds retained semantic components while advancing the live scene and paint clocks.
    pub fn begin(
        &mut self,
        model: Option<(&HudModel, HudChanges, i32)>,
        scene_clock: &BTreeMap<String, f64>,
        session: u64,
        catalog: &Arc<Catalog>,
    ) -> Arc<DataSource> {
        let data = match model {
            None => Arc::clone(&self.crosshair_data),
            Some((model, changes, opacity)) => {
                self.chat_color = model.chat_color;
                if changes.text {
                    self.model_clocks = hud_clocks(model);
                }
                self.clocks.clone_from(&self.model_clocks);
                let mut data = self.bindings.update(model, changes);
                if changes.status || self.opacity != Some(opacity) {
                    // A status refresh restores the default alpha binding.
                    drop(data);
                    data = self.bindings.set_opacity(opacity);
                    self.opacity = Some(opacity);
                }
                data
            }
        };
        self.clocks.extend(
            scene_clock
                .iter()
                .map(|(clock, born)| (clock.clone(), *born)),
        );
        self.hunger_animation.begin(session, catalog);
        data
    }

    /// Paints the HUD screen, or its crosshair overlay, returning its laid-out frame; `frame`'s
    /// renderers draw ahead of the HUD's native and item renderers.
    pub fn paint(
        &mut self,
        engine: &FormEngine,
        input: HudInput<'_>,
        frame: &mut PaintFrame<'_>,
    ) -> Result<Option<EngineFrame>, PaintError> {
        let HudInput {
            crosshair,
            capture_keep_out,
            catalog,
            context,
            data,
            paint,
            icons,
        } = input;
        let reference = if crosshair {
            Reference::HudCrosshairHudCrosshairScreen.as_str()
        } else {
            Reference::HudHudScreen.as_str()
        };
        let px = frame.metrics.scale.get() * FONT_DESIGN_PIXEL_TEXELS as f32;
        let language = frame.language;
        let hunger_animation = std::cell::RefCell::new(&mut self.hunger_animation);
        let advance_hunger =
            |key: &str, instance| hunger_animation.borrow_mut().advance(key, instance);
        let color = self.chat_color;
        let history = Reference::HudChatGridItem
            .as_str()
            .rsplit('.')
            .next()
            .unwrap_or_default();
        let text_color = |node: &json_ui::DrawNode| {
            crate::settings_chat::message_color(&node.key, history, color)
        };
        let bounds = PaintBounds::default();
        let art = ScreenArt {
            paint_bounds: (capture_keep_out && !crosshair).then_some(PaintObserver {
                bounds: &bounds,
                region: BOTTOM_HUD_REGION,
            }),
            text_color: Some(&text_color),
            nearest_sampling: true,
            clocks: Some(&self.clocks),
            ..ScreenArt::default()
        };
        let mut items = ItemRenderer { icons, id_aux: &[] };
        let screen = if crosshair {
            &mut self.crosshair
        } else {
            &mut self.hud
        };
        let caller = std::mem::take(&mut frame.renderers);
        let drawn = {
            let mut hud_renderer = HudRenderer {
                hud: paint,
                hunger_update: Some(&advance_hunger),
                bounds: (capture_keep_out && !crosshair).then_some(&bounds),
            };
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
        if !crosshair {
            self.editor_keep_out = drawn
                .as_ref()
                .ok()
                .and_then(|frame| frame.as_ref())
                .and(bounds.get())
                .map(|b| [b[0], b[1], b[2] - b[0], b[3] - b[1]].map(|v| f64::from(v / px)));
        }
        drawn
    }

    /// This frame's fade clocks.
    #[cfg(any(test, feature = "test-support"))]
    pub fn clocks(&self) -> &BTreeMap<String, f64> {
        &self.clocks
    }
}

/// The built-in bottom HUD root owns image frames, item icons, labels and native controls.
pub const BOTTOM_HUD_ROOT: &str = "centered_gui_elements_at_bottom_middle";
const BOTTOM_HUD_REGION: NonZeroU64 = NonZeroU64::new(1).unwrap();

#[cfg(test)]
mod keep_out_contract_tests {
    use super::*;

    #[test]
    fn bottom_keep_out_root_matches_the_built_in_layout() {
        let mut catalog = Catalog::from_files([
            ("ui/_global_variables.json", b"{}".as_slice()),
            (
                "ui/_ui_defs.json",
                br#"{"ui_defs":["ui/hud_screen.json"]}"#.as_slice(),
            ),
            ("ui/hud_screen.json", br#"{"namespace":"hud"}"#.as_slice()),
        ])
        .unwrap();
        install_java_hud(&mut catalog);
        assert!(catalog.lookup("hud", BOTTOM_HUD_ROOT).is_some());
    }
}
