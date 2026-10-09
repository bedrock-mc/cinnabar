//! One scene-construction visitor supplies rendering and debug visibility.

use super::*;

/// Tracks whether the visible scene stack permits the developer overlay.
#[derive(Default)]
pub(in crate::ui_runtime) struct DebugOverlayEligibility {
    gameplay: bool,
    blocked: bool,
}

impl DebugOverlayEligibility {
    /// Observes scene policy in the same bottom-to-top order as the renderer.
    pub(in crate::ui_runtime) fn push(&mut self, key: Scene, settings: ScreenSettings) {
        self.blocked |= !matches!(key, Scene::Gameplay | Scene::Crosshair | Scene::Hud)
            || (self.gameplay && !settings.render_game_behind);
        self.gameplay |= key == Scene::Gameplay;
    }

    /// Requires a visible gameplay scene with no other screen above its overlays.
    pub(in crate::ui_runtime) fn allowed(&self) -> bool {
        self.gameplay && !self.blocked
    }
}

/// Gameplay passes input through, captures the mouse and stays rendered.
/// JSON-UI overlays use its policy as the bottom of the scene stack.
const fn gameplay_settings() -> ScreenSettings {
    ScreenSettings {
        screen_not_flushable: false,
        always_accepts_input: false,
        render_game_behind: true,
        absorbs_input: false,
        is_showing_menu: false,
        is_modal: true,
        should_steal_mouse: true,
        low_frequency_rendering: false,
        screen_draws_last: false,
        force_render_below: false,
        send_telemetry: true,
        close_on_player_hurt: false,
        use_custom_pocket_toast: false,
        cache_screen: false,
        gamepad_cursor: false,
        gamepad_cursor_deflection_mode: false,
        vertical_scroll_delta: None,
        load_screen_immediately: false,
        render_only_when_topmost: false,
        should_be_skipped_during_automation: false,
    }
}

/// The OreUI death overlay preserves world overlays while owning player input.
pub(in crate::ui_runtime) fn death_settings() -> ScreenSettings {
    ScreenSettings {
        render_game_behind: true,
        force_render_below: true,
        absorbs_input: true,
        should_steal_mouse: false,
        ..ScreenSettings::default()
    }
}

impl UiRuntime {
    /// Visits the authoritative scene construction without allocating a stack.
    pub(super) fn visit_scenes_in(
        &self,
        player_runtime: &player_state::PlayerState,
        host: SceneHost,
        table: &ScreenSettingsTable,
        mut visit: impl FnMut(Scene, ScreenSettings),
    ) {
        let json = |reference: Option<&str>| {
            reference
                .and_then(|reference| table.get(reference))
                .unwrap_or_default()
        };
        if self.session_id() != 0 && host.over_world {
            visit(Scene::Gameplay, gameplay_settings());
            // The HUD screens are JSON scenes; without their definitions nothing draws them.
            if let Some(settings) = table.get(json_ui::CROSSHAIR_SCREEN) {
                visit(Scene::Crosshair, settings);
            }
            if let Some(settings) = table.get(json_ui::HUD_SCREEN) {
                visit(Scene::Hud, settings);
            }
        }
        if self.local_sleeping() {
            visit(Scene::Bed, json(Some(BED_SCREEN)));
        }
        if self.inventory_open() {
            let reference = crate::ui_runtime::presentation::forms::container_screen_reference(
                player_runtime,
                self,
            );
            visit(Scene::Container, json(reference));
        }
        if self.chat_focused() {
            visit(
                Scene::Chat,
                json(Some(crate::ui_runtime::presentation::forms::CHAT_SCREEN)),
            );
        }
        if self.emotes().is_open() {
            visit(
                Scene::Emote,
                json(Some(crate::ui_runtime::presentation::forms::EMOTE_SCREEN)),
            );
        }
        if host.loading && host.menu.is_none() {
            let reference = crate::ui_runtime::presentation::forms::LOADING_SCREEN;
            visit(Scene::Loading, json(Some(reference)));
        }
        if self.sign_editor().is_open() {
            let reference = crate::ui_runtime::presentation::forms::SIGN_SCREEN;
            visit(Scene::SignEditor, json(Some(reference)));
        }
        let form = self.server_forms().active();
        let settings_form = self.server_forms().settings_form_active();
        // An answered form keeps absorbing input until its answer is sent.
        if self.server_forms().owns_input() && !settings_form {
            visit(
                Scene::ServerForm,
                json(Some(form.map_or(SERVER_FORM_SCREEN, form_screen))),
            );
        }
        if let Some(screen) = host.menu {
            let reference = crate::ui_runtime::presentation::forms::menu_reference(screen);
            let settings = if screen == MenuScreen::Death {
                death_settings()
            } else {
                json(reference)
            };
            visit(Scene::Menu(screen), settings);
        }
        if let Some(entry) = form.filter(|_| settings_form) {
            visit(Scene::ServerSettingsForm, json(Some(form_screen(entry))));
        }
        if self.credits().owns_input() {
            visit(
                Scene::Credits,
                json(Some(crate::ui_runtime::credits::CREDITS_SCREEN)),
            );
        }
    }

    /// Tests developer-overlay eligibility using the same scene construction as rendering.
    pub(in crate::ui_runtime) fn debug_overlay_allowed_in(
        &self,
        player_runtime: &player_state::PlayerState,
        host: SceneHost,
        table: &ScreenSettingsTable,
    ) -> bool {
        let mut eligibility = DebugOverlayEligibility::default();
        self.visit_scenes_in(player_runtime, host, table, |key, settings| {
            eligibility.push(key, settings);
        });
        eligibility.allowed()
    }
}
