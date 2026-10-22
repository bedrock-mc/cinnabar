//! Projects the frame into the engine HUD: the [`json_ui::HudModel`] the templates bind, the native
//! renderers' captured paint, and the mod and preview renderers drawn ahead of the HUD's own.

use json_ui::vanilla::Reference;
use launcher::menu::settings_options::SettingId;
use std::{collections::BTreeSet, sync::Arc};

use json_ui::{Catalog, hud_context};
use mod_ui::crosshair::CrosshairRenderer;
use ui::UiNode;
use ui_engine::{
    engine::screen::{CustomRenderer, PaintFrame},
    text_metrics::TextMetrics,
};
use ui_screens::hud::HudInput;

use super::super::{UiPresentationError, UiPresentationRuntime, hud_layout};
use super::preview_renderer::PreviewRenderer;
use crate::ui_runtime::UiRuntime;

mod geometry;
mod semantic;
pub(in crate::ui_runtime::presentation) use semantic::SemanticHud;
#[cfg(test)]
#[path = "hud/reference.rs"]
mod reference;
#[cfg(test)]
use super::super::HudFrame;
#[cfg(test)]
use semantic::world_text_lines_for_frame as world_text_lines;

#[cfg(test)]
mod cooldown_tests;
#[cfg(test)]
mod crosshair_tests;
#[cfg(test)]
mod hunger_control_tests;
#[cfg(test)]
mod mob_effects_tests;

/// Java's per-line chat background opacity.
#[cfg(test)]
const CHAT_BACKGROUND_OPACITY: f64 = 0.5;
/// Newest chat lines the controller keeps alive.
const MAX_CHAT_LINES: usize = 50;
/// Java sidebar background opacities: 0.3 for rows, 0.4 for the title.
const SIDEBAR_OPACITY: f64 = 0.3;
const SIDEBAR_TITLE_OPACITY: f64 = 0.4;
/// The selected-item label shows for two seconds after the selection changes.
const ITEM_NAME_MILLIS: u64 = 2_000;
/// Display cap for stacked boss bars; the retained store holds more.
const MAX_BOSS_BARS: usize = 8;

/// Resolves the HUD screens of `catalog` under both chat placements on the calling thread, and
/// returns the textures their controls name.
pub(in crate::ui_runtime::presentation) fn prepare(catalog: &Arc<Catalog>) -> BTreeSet<String> {
    let base = hud_context(&ui_screens::menu_screens::retail_context());
    ui_screens::hud::prepared::prepare(
        catalog,
        [false, true].map(|top| ui_screens::chat_position::placed(base.clone(), top)),
    )
}

impl UiPresentationRuntime {
    /// Draw the gameplay HUD screen, or its crosshair overlay screen, through
    /// the engine; `Ok(false)` when the engine is not loaded.
    #[allow(clippy::too_many_arguments)]
    pub(in super::super) fn append_engine_hud(
        &mut self,
        player_runtime: &player_state::PlayerState,
        runtime: &UiRuntime,
        nodes: &mut Vec<UiNode>,
        next: &mut u32,
        metrics: TextMetrics,
        content: [f32; 2],
        now_millis: u64,
        crosshair: bool,
    ) -> Result<bool, UiPresentationError> {
        let Some(renderer) = self.engine.as_deref() else {
            return Ok(false);
        };
        if self
            .screens
            .chat
            .settings
            .options()
            .value(SettingId::HideHud)
            != 0
            || self.hud_frame.death_screen
            || self.screens.mod_ui.mod_panel_open()
                && (crosshair || !self.screens.mod_ui.mod_panel_shows_gameplay_hud())
        {
            return Ok(true);
        }
        let context = self.screens.hud.context(
            renderer.context(),
            self.screens.chat.settings.options().chat_at_top(),
        );
        let reference = if crosshair {
            Reference::HudCrosshairHudCrosshairScreen.as_str()
        } else {
            Reference::HudHudScreen.as_str()
        };
        if !renderer
            .scene_settings(reference, &context)
            .renders(crosshair || !runtime.chat_focused())
        {
            return Ok(true);
        }
        // Project at the caller clock, then restore the published frame before fallible painting.
        let captured_clock = std::mem::replace(&mut self.hud_frame.now_millis, now_millis);
        let frame = &self.hud_frame;
        let catalog = self.mod_hud_bindings.catalog(renderer.catalog());
        let options = self.screens.chat.settings.options();
        let model = if crosshair {
            None
        } else {
            let sidebar_key = (
                runtime.scoreboards().revision(),
                self.scoreboard_owner_names.revision(),
                self.screens.mod_ui.mod_scoreboard_hidden(),
            );
            let sidebar = self
                .scoreboard
                .refresh(runtime.scoreboards(), &self.scoreboard_owner_names)
                .filter(|_| !sidebar_key.2);
            let mut changes = self.hud_cache.refresh(
                player_runtime,
                runtime,
                frame,
                sidebar,
                sidebar_key,
                options,
                self.screens.mod_ui.mod_effect_icons_hidden(),
                renderer.catalog(),
            );
            let touch_inventory = self.screens.touch.enabled();
            if self.hud_cache.model.touch_inventory != touch_inventory {
                self.hud_cache.model.touch_inventory = touch_inventory;
                changes.status = true;
            }
            Some((
                &self.hud_cache.model,
                changes,
                options.value(SettingId::InterfaceOpacity),
            ))
        };
        let data = self
            .screens
            .hud
            .begin(model, &self.scene_clock, runtime.session_id(), &catalog);
        let data = self.mod_hud_bindings.merge(data, crosshair);
        let mut paint = hud_layout::capture_hud_paint(
            player_runtime,
            runtime,
            frame,
            self.hud_textures.as_ref(),
            self.screens.chat.settings.options(),
        );
        if let Some(style) = self.screens.mod_ui.mod_hunger_overlay() {
            paint.hunger.set_overlay(
                style,
                runtime.gameplay_hud().saturation(),
                frame.selected_food.as_ref(),
            );
        }
        let player_preview = frame.player_preview;
        self.hud_frame.now_millis = captured_clock;
        // Packs may replace the native cursor renderer with ordinary image controls.
        // Gate the whole overlay so these controls obey the same camera and HUD settings.
        if crosshair && paint.crosshair.is_none() {
            return Ok(true);
        }
        if !crosshair {
            // Legacy HUDs embed this renderer; the separate overlay already draws it.
            // Inverting the same pixels twice restores the world behind the cursor.
            paint.crosshair = None;
        }
        if self.screens.mod_ui.mod_effect_icons_hidden() {
            paint.effects.clear();
            paint.java_effects.clear();
        }
        let crosshair_spec = if self.mod_hud_visible(player_runtime, runtime) {
            self.screens.mod_ui.mod_crosshair().cloned()
        } else {
            None
        };
        let drew = &self.hud_renderers[usize::from(crosshair)];
        let renderers_unchanged =
            drew.preview == player_preview && drew.crosshair == crosshair_spec;
        let translate = |key: &str| runtime.translation(key);
        let preview_view = std::cell::Cell::new(None);
        let mut preview = PreviewRenderer {
            preview: player_preview,
            view: Some(&preview_view),
            ..PreviewRenderer::default()
        };
        let mut custom_crosshair = CrosshairRenderer {
            spec: crosshair_spec.as_ref(),
            visible: paint.crosshair.is_some(),
        };
        let mut renderers: [&mut dyn CustomRenderer; 2] = [&mut custom_crosshair, &mut preview];
        let mut paint_frame = PaintFrame {
            layouts: &mut self.layouts,
            font: &self.font,
            metrics,
            solid_page: self.solid_texture_page,
            safe_area: self.safe_area,
            content,
            translate: &translate,
            language: runtime.text_generation(),
            now: now_millis as f64 / 1_000.0,
            nodes: &mut *nodes,
            next: &mut *next,
            renderers: &mut renderers,
        };
        let drawn = self.screens.hud.paint(
            renderer,
            HudInput {
                capture_keep_out: self.screens.mod_ui.mod_hud_editor_open(),
                renderers_unchanged,
                crosshair,
                catalog: &catalog,
                context: &context,
                data,
                paint: &paint,
                icons: if crosshair {
                    &[]
                } else {
                    &self.hud_cache.icons
                },
            },
            &mut paint_frame,
        )?;
        if !crosshair {
            // Touch selects hotbar slots through the HUD's own laid-out cells.
            self.screens.touch.hud_session = drawn.as_ref().map(|_| runtime.session_id());
            self.screens.touch.hud_keep_out = self.screens.hud.editor_keep_out;
            self.screens.touch.hud = drawn;
        }
        // A reused paint ran no player renderer; the pose it asked for still holds.
        let reused = self.screens.hud.reused(crosshair);
        let drew = &mut self.hud_renderers[usize::from(crosshair)];
        if !reused {
            drew.view = preview_view.get();
        }
        drew.preview = player_preview;
        drew.crosshair = crosshair_spec;
        if let Some(view) = drew.view {
            self.player_preview_view = view;
        }
        Ok(true)
    }
}

/// What the presentation's own renderers drew one HUD screen from, and the player pose its
/// player renderer asked for, so an unchanged paint can be reused.
#[derive(Default)]
pub(in crate::ui_runtime::presentation) struct HudRendererInputs {
    preview: Option<ui::IconRef>,
    crosshair: Option<ui::mod_hud::Crosshair>,
    view: Option<super::super::player_preview::PreviewView>,
}

#[cfg(any(test, feature = "test-support"))]
impl UiPresentationRuntime {
    /// The engine HUD's last laid-out draw nodes, in GUI px.
    pub fn hud_draw_nodes(&self) -> &[json_ui::DrawNode] {
        self.screens.hud.hud.nodes()
    }

    /// Bind+layout passes the engine HUD ran.
    pub fn hud_passes(&self) -> usize {
        self.screens.hud.hud.passes
    }

    /// Paints the engine HUD and its crosshair overlay ran rather than reused:
    /// `[hud, crosshair]`.
    pub fn hud_paints(&self) -> [usize; 2] {
        self.screens.hud.paints()
    }

    /// Drops the engine HUD's retained paints, so the next frame paints both screens in full.
    pub fn forget_hud_paints(&mut self) {
        self.screens.hud.forget_paints();
    }

    /// The engine HUD's painted sprite paths that resolve to no texture source.
    pub fn hud_unresolved_sprites(&self) -> Vec<String> {
        let Some(engine) = self.engine.as_deref() else {
            return Vec::new();
        };
        let atlas = engine.textures.lock();
        let view = ui_engine::textures::Textures {
            assets: engine.assets(),
            set: &engine.textures,
            atlas: &atlas,
            images: None,
        };
        let mut missing: Vec<String> = self
            .hud_draw_nodes()
            .iter()
            .filter(|node| node.alpha > 0.0)
            .filter_map(|node| match &node.draw {
                json_ui::Draw::Sprite { texture, .. } => Some(texture.clone()),
                _ => None,
            })
            .filter(|texture| view.sprite(texture).is_none())
            .collect();
        missing.sort();
        missing.dedup();
        missing
    }

    /// A draw node's fade multiplier at `now`, under this frame's clocks,
    /// sampled by a fresh animator so it runs from the node's creation clock.
    pub fn hud_fade(&self, node: &json_ui::DrawNode, now: f64) -> f32 {
        let clocks = Some(self.screens.hud.clocks());
        let opacity = node
            .animate(&mut json_ui::Animator::new(), now, clocks, None)
            .opacity;
        if node.alpha > 0.0 {
            opacity / node.alpha
        } else {
            opacity
        }
    }
}

#[cfg(test)]
#[path = "hud/cache_tests.rs"]
mod cache_tests;

#[cfg(test)]
#[path = "hud/texture_metadata_tests.rs"]
mod texture_metadata_tests;

#[cfg(test)]
#[path = "hud/life_state_tests.rs"]
mod life_state_tests;

#[cfg(test)]
impl UiPresentationRuntime {
    /// Semantic component constructions, ordered as `HudChanges::components`.
    pub(in crate::ui_runtime::presentation) fn hud_component_constructions(
        &self,
    ) -> [usize; json_ui::HudChanges::COUNT] {
        self.hud_cache.constructions
    }
}
