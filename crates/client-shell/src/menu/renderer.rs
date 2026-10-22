//! Renderer and shader-pack settings choices.
//! Choices for renderers this build does not compile are ignored.

use super::{MenuRuntime, RenderMode};
use launcher::menu::{MenuScreen, RendererChoice};

impl MenuRuntime {
    /// Opens the pack controls through the normal settings navigation path.
    pub(super) fn activate_shaders_tab(&mut self) {
        if render_model::SHADER_PACKS_ENABLED {
            self.activate_settings_section(
                launcher::menu::settings_sections::SettingSection::Shaders
                    .definition()
                    .index,
            );
        }
    }

    /// Routes the Shaders tab's reload button to the render host in shader-pack builds.
    pub(super) fn activate_shader_reload(&mut self) {
        if render_model::SHADER_PACKS_ENABLED {
            self.request_shader_pack_reload();
        }
    }

    /// Opens a confirmation only for a current imported library entry.
    pub(super) fn remove_shader_pack_dialog(&mut self, index: u16, revision: u64) {
        if render_model::SHADER_PACKS_ENABLED
            && revision == self.shader_packs_revision
            && self.shader_packs.get(usize::from(index)).is_some()
            && self.shader_pack_removing.is_none()
        {
            self.dialog = Some(launcher::menu::MenuDialog::RemoveShaderPack(
                index, revision,
            ));
            self.focused = 0;
        }
    }

    /// Queues deletion after confirmation, rejecting stale lists and duplicate requests.
    /// The host decides whether deleting this entry changes the active renderer.
    pub(super) fn confirm_remove_shader_pack(&mut self, index: u16, revision: u64) {
        if !render_model::SHADER_PACKS_ENABLED
            || revision != self.shader_packs_revision
            || self.dialog
                != Some(launcher::menu::MenuDialog::RemoveShaderPack(
                    index, revision,
                ))
            || self.shader_pack_removing.is_some()
        {
            return;
        }
        let Some(name) = self.shader_packs.get(usize::from(index)).cloned() else {
            return;
        };
        self.dialog = None;
        self.shader_pack_removing = Some(std::sync::Arc::from(name.as_str()));
        self.shader_pack_remove_request = Some(name);
    }

    /// Consumes one confirmed library deletion for the host's background worker.
    pub fn take_shader_pack_removal(&mut self) -> Option<String> {
        self.shader_pack_remove_request.take()
    }

    /// Releases the pending state after the host reports a deletion outcome.
    pub fn finish_shader_pack_removal(&mut self) {
        self.shader_pack_removing = None;
    }

    /// Replaces the discovered shader-pack names the picker offers, in display order. Names
    /// past `u16::MAX` are dropped so every listed pack stays addressable by its choice.
    pub fn sync_shader_packs(&mut self, mut names: Vec<String>) {
        names.truncate(usize::from(u16::MAX));
        if self.shader_packs[..] != names[..] {
            self.shader_packs = names.into();
            self.shader_packs_revision += 1;
            if matches!(
                self.dialog,
                Some(launcher::menu::MenuDialog::RemoveShaderPack(..))
            ) {
                self.dialog = None;
            }
        }
    }

    /// Mirrors the applied pack name; a pending menu choice wins until taken.
    pub fn sync_shader_pack(&mut self, applied: Option<&str>) {
        if self.shader_pack_request.is_none() && self.shader_pack.as_deref() != applied {
            self.shader_pack = applied.map(str::to_owned);
        }
    }

    /// Consume the pack name the picker selected. Its [`RenderMode::ShaderPack`] switch, when
    /// the mode changed too, arrives through [`Self::take_render_mode_request`].
    pub fn take_shader_pack_request(&mut self) -> Option<String> {
        self.shader_pack_request.take()
    }

    /// Asks the app to reload shader packs as Iris's reload key does: re-list the pack
    /// folder, then re-read the selected pack (source and options) from disk and swap it in
    /// once translated, drawing the current pack meanwhile. The one entry point for anything
    /// that changes a pack on disk, such as its options screen applying.
    pub fn request_shader_pack_reload(&mut self) {
        self.shader_pack_reload = true;
    }

    /// Consume a [`Self::request_shader_pack_reload`].
    pub fn take_shader_pack_reload(&mut self) -> bool {
        std::mem::take(&mut self.shader_pack_reload)
    }

    /// Whether files dropped on the window go to the shader-pack folder: in shader-pack
    /// builds while the Shaders tab, which lists the packs, is on screen.
    #[must_use]
    pub fn takes_shader_pack_drops(&self) -> bool {
        use launcher::menu::settings_sections::SettingSection;
        render_model::SHADER_PACKS_ENABLED
            && self.visible
            && self.screen == MenuScreen::Settings
            && SettingSection::from_index(self.settings.section) == Some(SettingSection::Shaders)
    }

    /// The highlighted choice; `None` while the selected pack is not in the synced list.
    pub(super) fn renderer_choice(&self) -> Option<RendererChoice> {
        match self.render_mode {
            RenderMode::Vanilla => Some(RendererChoice::Vanilla),
            RenderMode::Enhanced => Some(RendererChoice::Enhanced),
            RenderMode::ShaderPack => {
                let selected = self.shader_pack.as_deref()?;
                let index = self.shader_packs.iter().position(|name| name == selected)?;
                u16::try_from(index)
                    .ok()
                    .map(|index| RendererChoice::ShaderPack(index, self.shader_packs_revision))
            }
        }
    }

    /// Queues the mode and pack requests for a picker press on a compiled renderer.
    pub(super) fn select_renderer(&mut self, choice: RendererChoice) {
        let mode = match choice {
            RendererChoice::Vanilla
                if render_model::ENHANCED_RENDERING_ENABLED
                    || render_model::SHADER_PACKS_ENABLED =>
            {
                RenderMode::Vanilla
            }
            RendererChoice::Enhanced if render_model::ENHANCED_RENDERING_ENABLED => {
                RenderMode::Enhanced
            }
            RendererChoice::ShaderPack(index, revision)
                if render_model::SHADER_PACKS_ENABLED && revision == self.shader_packs_revision =>
            {
                let Some(name) = self.shader_packs.get(usize::from(index)) else {
                    return;
                };
                self.shader_pack = Some(name.clone());
                self.shader_pack_request = Some(name.clone());
                RenderMode::ShaderPack
            }
            _ => return,
        };
        if self.render_mode != mode {
            self.render_mode = mode;
            self.render_mode_request = Some(mode);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use launcher::menu::MenuAction;

    /// Creates a menu with two discovered packs.
    fn menu() -> MenuRuntime {
        let mut menu = MenuRuntime::new(true, 2, "Steve".to_owned());
        menu.sync_shader_packs(vec!["Dusk.zip".to_owned(), "Lumen".to_owned()]);
        menu
    }

    #[test]
    fn picking_a_pack_requests_shader_pack_mode_and_its_name_only_when_compiled() {
        let mut menu = menu();
        menu.activate(MenuAction::SelectRenderer(RendererChoice::ShaderPack(
            1,
            menu.shader_packs_revision,
        )));
        if render_model::SHADER_PACKS_ENABLED {
            assert_eq!(
                menu.take_render_mode_request(),
                Some(RenderMode::ShaderPack)
            );
            assert_eq!(menu.take_shader_pack_request().as_deref(), Some("Lumen"));
            assert_eq!(
                menu.view().renderer,
                Some(RendererChoice::ShaderPack(1, menu.shader_packs_revision))
            );
            menu.activate(MenuAction::SelectRenderer(RendererChoice::Vanilla));
            assert_eq!(menu.take_render_mode_request(), Some(RenderMode::Vanilla));
            assert_eq!(menu.take_shader_pack_request(), None);
            assert_eq!(menu.view().renderer, Some(RendererChoice::Vanilla));
        } else {
            assert_eq!(menu.take_render_mode_request(), None);
            assert_eq!(menu.take_shader_pack_request(), None);
            assert_eq!(menu.view().renderer, Some(RendererChoice::Vanilla));
        }
        assert_eq!(menu.take_render_mode_request(), None);
    }

    #[test]
    fn a_picker_action_from_before_a_pack_refresh_is_ignored() {
        if !render_model::SHADER_PACKS_ENABLED {
            return;
        }
        let mut menu = menu();
        menu.sync_shader_packs(vec!["Lumen".to_owned()]);
        menu.sync_render_mode(RenderMode::ShaderPack);
        menu.sync_shader_pack(Some("Lumen"));
        let old_choice = menu.view().renderer.unwrap();
        menu.sync_shader_packs(vec!["Dusk.zip".to_owned(), "Lumen".to_owned()]);
        menu.activate(MenuAction::SelectRenderer(old_choice));
        assert_eq!(menu.take_shader_pack_request(), None);
        assert_eq!(menu.shader_pack.as_deref(), Some("Lumen"));
    }

    #[test]
    fn reselecting_the_saved_pack_requests_it_again() {
        if !render_model::SHADER_PACKS_ENABLED {
            return;
        }
        let mut menu = menu();
        menu.sync_render_mode(RenderMode::ShaderPack);
        menu.sync_shader_pack(Some("Lumen"));
        menu.select_renderer(RendererChoice::ShaderPack(1, menu.shader_packs_revision));
        assert_eq!(menu.take_shader_pack_request().as_deref(), Some("Lumen"));
    }

    #[test]
    fn picking_enhanced_follows_the_enhanced_feature() {
        let mut menu = menu();
        menu.activate(MenuAction::SelectRenderer(RendererChoice::Enhanced));
        let expected = render_model::ENHANCED_RENDERING_ENABLED.then_some(RenderMode::Enhanced);
        assert_eq!(menu.take_render_mode_request(), expected);
        assert_eq!(
            menu.view().render_mode,
            expected.unwrap_or(RenderMode::Vanilla)
        );
    }

    #[test]
    fn missing_packs_are_ignored_and_the_selection_follows_its_name() {
        let mut menu = menu();
        menu.activate(MenuAction::SelectRenderer(RendererChoice::ShaderPack(
            7,
            menu.shader_packs_revision,
        )));
        assert_eq!(menu.take_render_mode_request(), None);
        assert_eq!(menu.take_shader_pack_request(), None);
        menu.sync_render_mode(RenderMode::ShaderPack);
        menu.sync_shader_pack(Some("Lumen"));
        assert_eq!(
            menu.view().renderer,
            Some(RendererChoice::ShaderPack(1, menu.shader_packs_revision))
        );
        menu.sync_shader_packs(vec!["Lumen".to_owned()]);
        assert_eq!(
            menu.view().renderer,
            Some(RendererChoice::ShaderPack(0, menu.shader_packs_revision))
        );
        assert_eq!(menu.view().shader_packs[..], ["Lumen".to_owned()]);
        menu.sync_shader_packs(Vec::new());
        assert_eq!(menu.view().renderer, None);
        assert_eq!(menu.view().shader_pack.as_deref(), Some("Lumen"));
    }

    #[test]
    fn a_pending_pack_choice_survives_mirroring_until_taken() {
        if !render_model::SHADER_PACKS_ENABLED {
            return;
        }
        let mut menu = menu();
        menu.activate(MenuAction::SelectRenderer(RendererChoice::ShaderPack(
            0,
            menu.shader_packs_revision,
        )));
        menu.sync_render_mode(RenderMode::Vanilla);
        menu.sync_shader_pack(None);
        assert_eq!(
            menu.view().renderer,
            Some(RendererChoice::ShaderPack(0, menu.shader_packs_revision))
        );
        assert_eq!(menu.take_shader_pack_request().as_deref(), Some("Dusk.zip"));
        assert_eq!(
            menu.take_render_mode_request(),
            Some(RenderMode::ShaderPack)
        );
        menu.sync_render_mode(RenderMode::Vanilla);
        menu.sync_shader_pack(None);
        assert_eq!(menu.view().renderer, Some(RendererChoice::Vanilla));
    }

    #[test]
    fn a_reload_request_is_taken_once() {
        let mut menu = menu();
        assert!(!menu.take_shader_pack_reload());
        menu.request_shader_pack_reload();
        menu.request_shader_pack_reload();
        assert!(menu.take_shader_pack_reload());
        assert!(!menu.take_shader_pack_reload());
    }

    #[test]
    fn only_shader_settings_takes_dropped_shader_packs() {
        use launcher::menu::settings_sections::SettingSection;
        let mut menu = menu();
        menu.screen = MenuScreen::Settings;
        for section in [
            SettingSection::General,
            SettingSection::GlobalTexturePack,
            SettingSection::Video,
            SettingSection::Shaders,
        ] {
            menu.settings.section = section.definition().index;
            assert_eq!(
                menu.takes_shader_pack_drops(),
                render_model::SHADER_PACKS_ENABLED && section == SettingSection::Shaders,
                "{section:?}"
            );
        }
        menu.visible = false;
        assert!(!menu.takes_shader_pack_drops());
        menu.visible = true;
        menu.screen = MenuScreen::Play;
        assert!(!menu.takes_shader_pack_drops());
    }

    #[test]
    fn the_shaders_tab_reload_action_uses_the_existing_request() {
        let mut menu = menu();
        menu.activate(MenuAction::ReloadShaders);
        assert_eq!(
            menu.take_shader_pack_reload(),
            render_model::SHADER_PACKS_ENABLED
        );
        assert!(!menu.take_shader_pack_reload());
    }

    #[test]
    fn the_video_shortcut_opens_shaders_without_changing_the_pack() {
        use launcher::menu::settings_sections::SettingSection;
        let mut menu = menu();
        menu.sync_render_mode(RenderMode::ShaderPack);
        menu.sync_shader_pack(Some("Lumen"));
        menu.activate(MenuAction::Navigate(MenuScreen::Settings));
        menu.activate(MenuAction::SettingsSection(
            SettingSection::Video.definition().index,
        ));
        menu.activate(MenuAction::SettingsShaders);
        assert_eq!(
            menu.view().settings.section,
            if render_model::SHADER_PACKS_ENABLED {
                SettingSection::Shaders.definition().index
            } else {
                SettingSection::Video.definition().index
            }
        );
        assert_eq!(menu.take_shader_pack_request(), None);
        assert_eq!(menu.take_render_mode_request(), None);
        assert_eq!(menu.view().shader_pack.as_deref(), Some("Lumen"));
    }

    /// The reload key's row follows the same build switch as the renderer it reloads.
    #[test]
    fn shader_pack_controls_follow_the_shader_pack_build() {
        use launcher::menu::settings_options::{BindingId, SHADER_PACK_BINDINGS};
        assert_eq!(SHADER_PACK_BINDINGS, render_model::SHADER_PACKS_ENABLED);
        assert_eq!(
            BindingId::ReloadShaders.row(false).is_some(),
            render_model::SHADER_PACKS_ENABLED
        );
    }
}

#[cfg(test)]
mod removal_tests {
    use super::*;
    use launcher::menu::{MenuAction, MenuDialog};

    #[test]
    fn removing_a_remembered_pack_preserves_the_applied_renderer() {
        if !render_model::SHADER_PACKS_ENABLED {
            return;
        }
        for mode in [RenderMode::Vanilla, RenderMode::Enhanced] {
            let mut menu = MenuRuntime::new(true, 2, "Steve".into());
            menu.sync_shader_packs(vec!["Dusk.zip".into()]);
            menu.sync_shader_pack(Some("Dusk.zip"));
            menu.sync_render_mode(mode);
            let revision = menu.view().shader_packs_revision;
            menu.activate(MenuAction::RemoveShaderPackDialog(0, revision));
            menu.activate(MenuAction::ConfirmRemoveShaderPack(0, revision));
            assert_eq!(menu.take_shader_pack_removal().as_deref(), Some("Dusk.zip"));
            assert_eq!(menu.view().render_mode, mode);
            assert!(menu.take_render_mode_request().is_none());
        }
    }

    #[test]
    fn removal_requires_current_confirmation_and_leaves_mode_changes_to_the_host() {
        if !render_model::SHADER_PACKS_ENABLED {
            return;
        }
        let mut menu = MenuRuntime::new(true, 2, "Steve".into());
        menu.sync_shader_packs(vec!["Dusk.zip".into(), "Lumen".into()]);
        let revision = menu.view().shader_packs_revision;
        menu.activate(MenuAction::SelectRenderer(RendererChoice::ShaderPack(
            0, revision,
        )));
        menu.take_render_mode_request();
        menu.take_shader_pack_request();
        menu.activate(MenuAction::ConfirmRemoveShaderPack(0, revision));
        assert!(menu.take_shader_pack_removal().is_none());
        menu.activate(MenuAction::RemoveShaderPackDialog(0, revision));
        assert_eq!(
            menu.view().dialog,
            Some(MenuDialog::RemoveShaderPack(0, revision))
        );
        menu.activate(MenuAction::DismissDialog);
        menu.activate(MenuAction::ConfirmRemoveShaderPack(0, revision));
        assert!(menu.take_shader_pack_removal().is_none());
        menu.activate(MenuAction::RemoveShaderPackDialog(0, revision));
        menu.sync_shader_packs(vec!["Lumen".into()]);
        menu.activate(MenuAction::ConfirmRemoveShaderPack(0, revision));
        assert!(menu.take_shader_pack_removal().is_none());
        let revision = menu.view().shader_packs_revision;
        menu.activate(MenuAction::SelectRenderer(RendererChoice::ShaderPack(
            0, revision,
        )));
        menu.activate(MenuAction::RemoveShaderPackDialog(0, revision));
        menu.activate(MenuAction::ConfirmRemoveShaderPack(0, revision));
        assert_eq!(menu.take_shader_pack_removal().as_deref(), Some("Lumen"));
        assert_eq!(menu.view().render_mode, ui::RenderMode::ShaderPack);
        assert!(menu.take_render_mode_request().is_none());
        assert_eq!(menu.view().shader_pack_removing.as_deref(), Some("Lumen"));
        menu.activate(MenuAction::RemoveShaderPackDialog(0, revision));
        assert!(menu.view().dialog.is_none());
        menu.finish_shader_pack_removal();
        assert!(menu.view().shader_pack_removing.is_none());
    }
}
