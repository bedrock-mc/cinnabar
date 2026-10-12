//! Projects runtime authority into `mod-ui`'s surfaces and paints them in the scene order the
//! frame builder chooses.

use std::collections::BTreeSet;

use mod_ui::{ModUi, experience_modal::ExperienceModal, mod_screens::ModScreensInput};
use ui::{UiNode, mod_hud::Hud};
use ui_engine::{engine::screen::PaintFrame, pages, text_metrics::TextMetrics};

use super::super::UiPresentationRuntime;
use crate::ui_runtime::UiRuntime;

/// A paint frame over the runtime's text caches, painting at time zero with no renderers.
macro_rules! paint_frame {
    ($runtime:ident, $ui:expr, $nodes:expr, $next:expr, $metrics:expr, $content:expr, $translate:expr) => {
        PaintFrame {
            layouts: &mut $runtime.layouts,
            font: &$runtime.font,
            metrics: $metrics,
            solid_page: $runtime.solid_texture_page,
            safe_area: $runtime.safe_area,
            content: $content,
            translate: $translate,
            language: $ui.text_generation(),
            now: 0.0,
            nodes: $nodes,
            next: $next,
            renderers: &mut [],
        }
    };
}

#[cfg(test)]
mod tests;

impl UiPresentationRuntime {
    /// Every mod- and experience-owned surface.
    pub fn mod_ui(&self) -> &ModUi {
        &self.screens.mod_ui
    }

    pub fn mod_ui_mut(&mut self) -> &mut ModUi {
        &mut self.screens.mod_ui
    }

    /// Replaces the personal HUD cards, first revoking chat links bound to chat actions the
    /// new content no longer publishes.
    pub fn set_mod_hud(&mut self, content: Option<&Hud>) -> Result<(), String> {
        if self.screens.mod_ui.mod_chat_actions()
            != content.map_or(&[][..], |hud| hud.chat_actions.as_slice())
        {
            let chat = &mut self.screens.chat;
            chat.link_generation = None;
            chat.link_hits.clear();
            chat.extension_events.clear();
        }
        self.screens.mod_ui.set_mod_hud(content)
    }

    /// Follows the client part's modal in the dynamic pages reserved to its atlas.
    pub fn set_experience_modal(&mut self, modal: Option<ExperienceModal<'_>>) {
        let page = (self.textures.dynamic_start() + pages::MODAL_UI_PAGE) as u16;
        self.screens.mod_ui.set_experience_modal(modal, page);
    }

    /// Follows the player mod's screens in the dynamic pages reserved to their atlas.
    pub fn set_mod_screens(&mut self, input: Option<ModScreensInput<'_>>) {
        let page = (self.textures.dynamic_start() + pages::MOD_UI_PAGE) as u16;
        self.screens.mod_ui.set_mod_screens(input, page);
    }

    /// All personal HUD content follows gameplay focus and the player's HUD preference.
    pub(in super::super) fn mod_hud_visible(
        &self,
        player: &player_state::PlayerState,
        runtime: &UiRuntime,
    ) -> bool {
        !runtime.ui_focused(player)
            && self.menu_view.is_none()
            && self.loading_stage.is_none()
            && self.screens.chat.settings.options.value("hide_hud") == 0
            && !self.screens.mod_ui.mod_panel_open()
    }

    /// Draws the extension label through JSON-UI only while the gameplay HUD owns focus.
    pub(in super::super) fn append_mod_hud(
        &mut self,
        player_runtime: &player_state::PlayerState,
        runtime: &UiRuntime,
        nodes: &mut Vec<UiNode>,
        next: &mut u32,
        metrics: TextMetrics,
        content: [f32; 2],
    ) {
        if !self.mod_hud_visible(player_runtime, runtime)
            || runtime.ui_focused(player_runtime)
            || self.loading_stage.is_some()
            || !self.screens.hud.hud.has_visible_content()
        {
            return;
        }
        let Some(renderer) = self.engine.as_deref() else {
            return;
        };
        let mut paint = paint_frame!(self, runtime, nodes, next, metrics, content, &|_| None);
        self.screens.mod_ui.paint_mod_hud(renderer, &mut paint);
    }

    /// Draws the personal HUD cards while the HUD shows content.
    pub(in super::super) fn append_mod_widgets(
        &mut self,
        player: &player_state::PlayerState,
        runtime: &UiRuntime,
        nodes: &mut Vec<UiNode>,
        next: &mut u32,
        metrics: TextMetrics,
        content: [f32; 2],
    ) {
        if !self.mod_hud_visible(player, runtime) || !self.screens.hud.hud.has_visible_content() {
            return;
        }
        let Some(resolved) = self
            .screens
            .mod_ui
            .mod_widget_icons(|id, metadata| self.item_icon(id, metadata))
        else {
            return;
        };
        let mut paint = paint_frame!(self, runtime, nodes, next, metrics, content, &|_| None);
        self.screens
            .mod_ui
            .paint_mod_widgets(self.engine.as_deref(), &mut paint, &resolved);
    }

    /// Draws the personal panel, or the HUD editor while it owns the panel surface.
    pub(in super::super) fn append_mod_panel(
        &mut self,
        runtime: &UiRuntime,
        nodes: &mut Vec<UiNode>,
        next: &mut u32,
        metrics: TextMetrics,
        content: [f32; 2],
    ) {
        if self.screens.mod_ui.mod_hud_editor_open() {
            let Some(resolved) = self
                .screens
                .mod_ui
                .mod_hud_editor_icons(|id, metadata| self.item_icon(id, metadata))
            else {
                return;
            };
            let mut paint = paint_frame!(self, runtime, nodes, next, metrics, content, &|_| None);
            self.screens
                .mod_ui
                .paint_mod_hud_editor(self.engine.as_deref(), &mut paint, &resolved);
            return;
        }
        let mut paint = paint_frame!(self, runtime, nodes, next, metrics, content, &|_| None);
        self.screens
            .mod_ui
            .paint_mod_panel(self.engine.as_deref(), &mut paint);
    }

    /// Draws the client part's modal over the gameplay scenes; trusted chrome draws after it.
    #[allow(
        clippy::too_many_arguments,
        reason = "Player authority is borrowed separately from UI state."
    )]
    pub(in super::super) fn append_experience_modal(
        &mut self,
        player_runtime: &player_state::PlayerState,
        runtime: &UiRuntime,
        nodes: &mut Vec<UiNode>,
        next: &mut u32,
        metrics: TextMetrics,
        content: [f32; 2],
        over_gameplay: bool,
    ) {
        let translate = |key: &str| runtime.translation(key);
        let hud_frame = &self.hud_frame;
        let mut paint = paint_frame!(self, runtime, nodes, next, metrics, content, &translate);
        self.screens.mod_ui.paint_experience_modal(
            self.engine.as_deref(),
            &mut paint,
            over_gameplay,
            |icons| super::containers::player_rows(player_runtime, hud_frame, icons),
        );
    }

    /// Draws trusted chrome last, including when server UI or the HUD is hidden.
    pub(in super::super) fn append_experience_chrome(
        &mut self,
        runtime: &UiRuntime,
        nodes: &mut Vec<UiNode>,
        next: &mut u32,
        metrics: TextMetrics,
        content: [f32; 2],
    ) {
        let mut paint = paint_frame!(self, runtime, nodes, next, metrics, content, &|_| None);
        self.screens
            .mod_ui
            .paint_experience_chrome(self.engine.as_deref(), &mut paint);
    }

    /// Draws the mod's view and overlay over the container screen this build drew, from node
    /// `container_start` on.
    #[allow(
        clippy::too_many_arguments,
        reason = "Player authority is borrowed separately from UI state."
    )]
    pub(in super::super) fn append_mod_screens(
        &mut self,
        player_runtime: &player_state::PlayerState,
        runtime: &UiRuntime,
        nodes: &mut Vec<UiNode>,
        next: &mut u32,
        metrics: TextMetrics,
        content: [f32; 2],
        container_start: usize,
    ) {
        let Some(keys) = self.screens.mod_ui.mod_screen_icon_keys() else {
            return;
        };
        let id_aux = item_icons(keys, player_runtime, runtime, |id, aux| {
            self.item_icon(id, aux)
        });
        let translate = |key: &str| runtime.translation(key);
        let container = self.projections.container.as_ref().map(|(frame, _)| frame);
        let mut paint = paint_frame!(self, runtime, nodes, next, metrics, content, &translate);
        self.screens.mod_ui.paint_mod_screens(
            self.engine.as_deref(),
            container,
            &mut paint,
            container_start,
            &id_aux,
            || super::container_screen_reference(player_runtime, runtime),
        );
    }
}

/// Each key's icon: vanilla's `#item_id_aux` is the network id over 16 bits of aux, which the
/// session's item registry names.
fn item_icons(
    keys: &BTreeSet<i64>,
    player_runtime: &player_state::PlayerState,
    runtime: &UiRuntime,
    icon: impl Fn(&str, u32) -> Option<ui::IconRef>,
) -> Vec<(i64, ui::IconRef)> {
    let Some(registry) = runtime
        .inventory_ledger(player_runtime)
        .negotiated_item_registry()
    else {
        return Vec::new();
    };
    keys.iter()
        .filter_map(|key| {
            let network_id = i32::try_from(key >> 16).ok()?;
            let aux = u32::try_from(key & 0xffff).ok()?;
            let entry = registry.get(&network_id)?;
            Some((*key, icon(&entry.identifier, aux)?))
        })
        .collect()
}
