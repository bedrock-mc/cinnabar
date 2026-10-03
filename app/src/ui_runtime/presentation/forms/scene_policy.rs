//! Scene-stack policy from the active pack's resolved screen roots.

use json_ui::ScreenSettings;

use super::menu_screens;
use crate::{
    menu::{MenuRuntime, MenuView},
    ui_runtime::{UiRuntime, presentation::UiPresentationRuntime},
};

impl UiPresentationRuntime {
    /// Input-absorbing screens stop gameplay independently of their background policy.
    pub(crate) fn absorbs_gameplay_input(&self, runtime: &UiRuntime, menu: &MenuRuntime) -> bool {
        runtime.ui_focused()
            || (menu.is_visible() && self.menu_settings(runtime, &menu.view()).absorbs_input)
    }

    /// Every visible scene above the world must permit drawing the game behind it.
    pub(crate) fn renders_game_behind(&self, runtime: &UiRuntime, menu: &MenuRuntime) -> bool {
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
        if runtime.inventory_open()
            && let Some(layout) = super::containers::ScreenLayout::of(runtime, None)
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
        if view.dialog.is_some() {
            let popup = engine.scene_settings("popup_dialog.modal_dialog_popup", &screen.context);
            policy.absorbs_input |= popup.absorbs_input;
            policy.render_game_behind &= popup.render_game_behind;
        }
        policy
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::menu::MenuAction;
    use crate::ui_runtime::presentation::forms::{ServerUiPack, pack_harness};

    #[test]
    fn java_chat_keeps_the_world_and_hud_but_absorbs_gameplay() {
        let Some(mut presentation) = pack_harness::engine_presentation() else {
            eprintln!(
                "skipping java_chat_keeps_the_world_and_hud_but_absorbs_gameplay: fixture unavailable; requires installed local carriers (make assets)"
            );
            return;
        };
        let mut runtime = UiRuntime::new(1);
        runtime.open_chat();
        let menu = MenuRuntime::new(false, 2, "Tester".into());
        let engine = presentation.form_presentation.engine.as_deref().unwrap();
        let chat = engine.scene_settings(super::super::chat_screen::CHAT_SCREEN, engine.context());
        let hud = engine.scene_settings(json_ui::HUD_SCREEN, engine.context());
        assert!(chat.absorbs_input && chat.render_game_behind);
        assert!(!hud.absorbs_input && hud.renders(false));
        assert!(presentation.renders_game_behind(&runtime, &menu));
        assert!(presentation.absorbs_gameplay_input(&runtime, &menu));
        presentation.set_server_ui_pack(&ServerUiPack {
            ui_layers: vec![vec![(
                "ui/hud_screen.json".into(),
                br#"{"namespace":"hud","hud_screen":{"render_only_when_topmost":true}}"#.to_vec(),
            )]],
            ..Default::default()
        });
        let engine = presentation.form_presentation.engine.as_deref().unwrap();
        assert!(
            !engine
                .scene_settings(json_ui::HUD_SCREEN, engine.context())
                .renders(false)
        );
    }

    #[test]
    fn retail_menu_defaults_and_server_visibility_override_share_the_resolved_root() {
        let Some(mut presentation) = pack_harness::engine_presentation() else {
            return;
        };
        let runtime = UiRuntime::new(1);
        let mut menu = MenuRuntime::new(false, 2, "Tester".into());
        menu.open_pause();
        let pause = presentation.menu_settings(&runtime, &menu.view());
        assert!(pause.absorbs_input && pause.render_game_behind && pause.render_only_when_topmost);
        assert!(presentation.renders_game_behind(&runtime, &menu));
        menu.activate(MenuAction::PauseSettings);
        let settings = presentation.menu_settings(&runtime, &menu.view());
        assert!(settings.absorbs_input && settings.render_game_behind);
        assert!(!presentation.renders_game_behind(&runtime, &menu));
        menu.set_visible(false);
        menu.open_pause();
        presentation.set_server_ui_pack(&ServerUiPack {
            ui_layers: vec![vec![(
                "ui/pause_screen.json".into(),
                br#"{"namespace":"pause","pause_screen":{"render_game_behind":false}}"#.to_vec(),
            )]],
            ..Default::default()
        });
        assert!(!presentation.renders_game_behind(&runtime, &menu));
        assert!(presentation.absorbs_gameplay_input(&runtime, &menu));
    }
}
