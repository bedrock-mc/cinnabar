//! Scene-stack policy from the active pack's resolved screen roots.

use json_ui::ScreenSettings;

use super::menu_screens;
use crate::{
    menu::MenuView,
    ui_runtime::{UiRuntime, presentation::UiPresentationRuntime},
};

/// Synchronous launcher projection supplied by the app until launcher services move.
pub trait MenuScene {
    /// Whether the launcher currently owns a visible screen.
    fn is_visible(&self) -> bool;
    /// The current launcher view, preserving the caller's frame ordering.
    fn view(&self) -> MenuView;
    /// The visible launcher screen at this observation boundary.
    fn scene(&self) -> Option<crate::menu::MenuScreen>;
    /// Whether the menu was opened over the connected world.
    fn over_world(&self) -> bool;
    /// Whether that launcher screen draws the title panorama.
    fn uses_panorama(&self) -> bool;
}

impl UiPresentationRuntime {
    /// Input-absorbing screens stop gameplay independently of their background policy. A client
    /// part's open modal is one: like a vanilla container screen it keeps the wheel, hotbar,
    /// movement and attack/use from the player until it closes.
    pub fn absorbs_gameplay_input(
        &self,
        player_runtime: &player_state::PlayerState,
        runtime: &UiRuntime,
        menu: &dyn MenuScene,
    ) -> bool {
        self.mod_panel_open() || self.base_absorbs_gameplay_input(player_runtime, runtime, menu)
    }

    /// The ordinary scene policy before an optional personal panel takes focus.
    pub fn base_absorbs_gameplay_input(
        &self,
        player_runtime: &player_state::PlayerState,
        runtime: &UiRuntime,
        menu: &dyn MenuScene,
    ) -> bool {
        runtime.ui_focused(player_runtime)
            || (menu.is_visible() && self.menu_settings(runtime, &menu.view()).absorbs_input)
            || self.experience_modal_open()
    }

    /// Every visible scene above the world must permit drawing the game behind it.
    pub fn renders_game_behind(
        &self,
        player_runtime: &player_state::PlayerState,
        runtime: &UiRuntime,
        menu: &dyn MenuScene,
    ) -> bool {
        let engine = self.form_presentation.engine.as_deref();
        let settings = |reference: &str| {
            engine.map_or_else(ScreenSettings::default, |engine| {
                engine.scene_settings(reference, engine.context())
            })
        };
        if menu.is_visible()
            && (!self.menu_settings(runtime, &menu.view()).render_game_behind
                || menu.uses_panorama())
        {
            return false;
        }
        if runtime.chat_focused() && !settings(super::chat_screen::CHAT_SCREEN).render_game_behind {
            return false;
        }
        if runtime.emotes().is_open()
            && !settings(super::emote_screen::EMOTE_SCREEN).render_game_behind
        {
            return false;
        }
        if runtime.inventory_open()
            && let Some(layout) = super::containers::ScreenLayout::of(player_runtime, runtime, None)
            && !settings(layout.screen().0).render_game_behind
        {
            return false;
        }
        if runtime.server_forms().owns_input()
            && !settings("server_form.third_party_server_screen").render_game_behind
        {
            return false;
        }
        true
    }

    /// Resolves the menu and any popup above it; a popup cannot uncover an opaque parent.
    fn menu_settings(&self, runtime: &UiRuntime, view: &MenuView) -> ScreenSettings {
        let Some(engine) = self.form_presentation.engine.as_deref() else {
            return ScreenSettings::default();
        };
        let Some(mut screen) = menu_screens::screen_data(view, &|key| runtime.translation(key))
        else {
            return ScreenSettings::default();
        };
        let mut policy = engine.scene_settings(screen.reference, &screen.context);
        while let Some(overlay) = screen.overlay.take() {
            screen = *overlay;
            let next = engine.scene_settings(screen.reference, &screen.context);
            policy.absorbs_input |= next.absorbs_input;
            policy.render_game_behind &= next.render_game_behind;
            policy.render_only_when_topmost = next.render_only_when_topmost;
        }
        if view.popup_open() {
            let popup = engine.scene_settings("popup_dialog.modal_dialog_popup", &screen.context);
            policy.absorbs_input |= popup.absorbs_input;
            policy.render_game_behind &= popup.render_game_behind;
        }
        policy
    }
}

#[cfg(any(test, feature = "test-support"))]
pub mod test_support {
    use super::*;
    /// Reads the resolved menu policy for app transition integration tests.
    pub fn menu_settings(
        presentation: &UiPresentationRuntime,
        runtime: &UiRuntime,
        view: &MenuView,
    ) -> ScreenSettings {
        presentation.menu_settings(runtime, view)
    }
}
