//! Retains the complete launcher output after its input and finite motion settle.
use launcher::menu::MenuScreen;
use std::{collections::HashMap, sync::Arc};
use {super::super::*, launcher::menu::MenuView};

pub(in crate::ui_runtime::presentation) struct RetainedMenu {
    view: Arc<MenuView>,
    frame: ([u32; 2], f32, SafeArea),
    font: Arc<RuntimeFontCatalog>,
    textures: Arc<UiRenderTextureArray>,
    settings: Arc<crate::ui_runtime::scene_stack::ScreenSettingsTable>,
    text_generation: [usize; 3],
    session: u64,
    scale: Option<u8>,
    look: forms::oreui::Look,
    offsets: HashMap<String, f32>,
}

impl UiPresentationRuntime {
    /// Only launcher scenes with no live overlay or continuously animated preview are retained.
    fn retainable_menu(&self, runtime: &UiRuntime) -> Option<&MenuView> {
        let view = self.menu_view.as_deref()?;
        (view.visible
            && !view.over_world
            && matches!(
                view.screen,
                MenuScreen::Home | MenuScreen::Servers | MenuScreen::Settings
            )
            && !(view.screen == MenuScreen::Servers && forms::oreui::animated_server_details(view))
            && !view.popup_open()
            && view.field.is_none()
            && view.key_remap.is_none()
            && !view.connecting
            && view.local.progress.is_none()
            && view.disconnect_message.is_none()
            && !view.auth_state.awaiting_browser()
            && !(view.screen == MenuScreen::Home
                && view
                    .feeds
                    .home
                    .live_event
                    .as_ref()
                    .is_some_and(|event| event.countdown))
            && !(view.screen == MenuScreen::Home && self.player_preview_icon.is_some())
            && self.loading_stage.is_none()
            && !runtime.chat_focused
            && !runtime.inventory_open
            && !runtime.local_sleeping
            && !runtime.emotes.is_open()
            && !runtime.sign_editor.is_open()
            && !runtime.forms.owns_input()
            && !runtime.credits.owns_input()
            && runtime.hud.toasts().is_empty()
            && !runtime.experiences.active
            && !runtime.experiences.holds_world_entry()
            && !self.mod_panel_open()
            && !self.experience_modal_open())
        .then_some(view)
    }

    /// Changes in view, resources, scroll position or viewport invalidate the retained output.
    pub(in crate::ui_runtime::presentation) fn retained_menu_input(
        &self,
        runtime: &UiRuntime,
        frame: ([u32; 2], f32, SafeArea),
    ) -> Option<UiRenderInput> {
        self.last_frame.as_ref()?;
        let view = self.retainable_menu(runtime)?;
        let saved = self.retained_menu.as_ref()?;
        ((Arc::ptr_eq(&saved.view, self.menu_view.as_ref()?) || saved.view.as_ref() == view)
            && saved.frame == frame
            && saved.scale == self.gui_scale_preference
            && Arc::ptr_eq(&saved.font, &self.font)
            && Arc::ptr_eq(&saved.textures, &self.textures)
            && Arc::ptr_eq(&saved.settings, &self.form_presentation.screen_settings)
            && saved.text_generation == runtime.text_generation()
            && saved.session == runtime.session_id()
            && saved.look == self.form_presentation.oreui_look
            && saved.offsets == *self.menu_scrolls.offsets()
            && !self.menu_scrolls.active()
            && !self
                .form_presentation
                .oreui_transitions
                .active(self.menu_seconds))
        .then(|| self.last_input.clone())
        .flatten()
    }

    /// Save only a completed frame; the next unchanged build keeps its hits and draw buffers.
    pub(in crate::ui_runtime::presentation) fn remember_menu(
        &mut self,
        runtime: &UiRuntime,
        frame: ([u32; 2], f32, SafeArea),
    ) {
        let view = if self.last_frame.is_some()
            && !self.menu_scrolls.active()
            && !self
                .form_presentation
                .oreui_transitions
                .active(self.menu_seconds)
        {
            self.retainable_menu(runtime)
                .and(self.menu_view.as_ref())
                .map(Arc::clone)
        } else {
            None
        };
        self.retained_menu = view.map(|view| RetainedMenu {
            view,
            frame,
            font: Arc::clone(&self.font),
            textures: Arc::clone(&self.textures),
            settings: Arc::clone(&self.form_presentation.screen_settings),
            text_generation: runtime.text_generation(),
            session: runtime.session_id(),
            scale: self.gui_scale_preference,
            look: self.form_presentation.oreui_look,
            offsets: self.menu_scrolls.offsets().clone(),
        });
    }
}
