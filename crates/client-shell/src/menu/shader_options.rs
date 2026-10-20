//! The Shader Pack Settings screen over Video settings: presses queue for the app, which owns
//! the pack's options and publishes the screen back.

use std::sync::Arc;

use launcher::menu::MenuScreen;
#[cfg(test)]
use launcher::menu::RendererChoice;
use launcher::shader_options::{Action, View};

use super::MenuRuntime;

/// Queued commands and the published screen; the screen is open while `view` is set.
#[derive(Debug, Default)]
pub(super) struct ShaderOptions {
    actions: Vec<Action>,
    view: Option<Arc<View>>,
    pending_section: Option<u8>,
}

impl MenuRuntime {
    /// Whether the requested renderer has a pack whose options can be opened.
    pub(super) fn shader_options_available(&self) -> bool {
        render_model::SHADER_PACKS_ENABLED
            && self.render_mode == ui::RenderMode::ShaderPack
            && self.shader_pack.is_some()
    }

    /// Queues a press. [`Action::Open`] opens the screen for the selected pack, showing it as
    /// loading until the app publishes it; other presses need the screen open.
    pub(super) fn activate_shader_options(&mut self, action: Action) {
        if !render_model::SHADER_PACKS_ENABLED {
            return;
        }
        if action == Action::Open {
            if !self.shader_options_available() {
                return;
            }
            let pack = self.shader_pack.as_deref().expect("a requested pack");
            self.shader_options.view = Some(Arc::new(View::loading(pack)));
        } else if self.shader_options.view.is_none() {
            return;
        }
        self.shader_options.actions.push(action);
    }

    /// Back on Settings while the screen is open leaves its sub-screen or closes it.
    pub(super) fn shader_options_back(&mut self) -> bool {
        let open = self.shader_options.view.is_some()
            && self.dialog.is_none()
            && self.screen == MenuScreen::Settings;
        if open {
            self.shader_options.actions.push(Action::Back);
        }
        open
    }

    /// Defers a section change until the option editor has closed successfully.
    pub(super) fn activate_settings_section(&mut self, section: u8) {
        let Some(view) = &self.shader_options.view else {
            self.activate_settings(launcher::menu::MenuAction::SettingsSection(section));
            return;
        };
        self.shader_options.pending_section = Some(section);
        if view.status != launcher::shader_options::Status::Saving {
            self.shader_options.actions.push(Action::Done);
        }
    }

    /// The commands queued since the last call, oldest first.
    pub fn take_shader_option_actions(&mut self) -> Vec<Action> {
        std::mem::take(&mut self.shader_options.actions)
    }

    /// Publishes the open screen, or closes it with `None`.
    pub fn sync_shader_options(&mut self, view: Option<View>) {
        self.shader_options.view = view.map(Arc::new);
        if let Some(section) = self.shader_options.pending_section {
            match self.shader_options.view.as_deref().map(|view| &view.status) {
                None => {
                    self.shader_options.pending_section = None;
                    self.activate_settings(launcher::menu::MenuAction::SettingsSection(section));
                }
                Some(launcher::shader_options::Status::Ready) => {
                    self.shader_options.actions.push(Action::Done);
                }
                _ => {}
            }
        }
    }

    /// The screen for [`launcher::menu::MenuView`].
    pub(super) fn shader_options_view(&self) -> Option<Arc<View>> {
        self.shader_options.view.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use launcher::menu::MenuAction;

    fn menu() -> MenuRuntime {
        let mut menu = MenuRuntime::new(true, 2, "Steve".to_owned());
        menu.sync_shader_packs(vec!["Dusk.zip".to_owned()]);
        menu
    }

    #[test]
    fn the_screen_opens_only_for_a_selected_pack() {
        let mut menu = menu();
        menu.activate(MenuAction::ShaderOptions(Action::Open));
        menu.activate(MenuAction::ShaderOptions(Action::Apply));
        assert!(menu.take_shader_option_actions().is_empty());
        assert_eq!(menu.view().shader_options, None);
        menu.activate(MenuAction::SelectRenderer(RendererChoice::ShaderPack(0)));
        menu.activate(MenuAction::ShaderOptions(Action::Open));
        if !render_model::SHADER_PACKS_ENABLED {
            assert!(menu.take_shader_option_actions().is_empty());
            return;
        }
        assert_eq!(
            menu.view().shader_options.as_deref(),
            Some(&View::loading("Dusk.zip"))
        );
        menu.activate(MenuAction::ShaderOptions(Action::Apply));
        assert_eq!(
            menu.take_shader_option_actions(),
            [Action::Open, Action::Apply]
        );
        menu.sync_shader_options(None);
        menu.activate(MenuAction::ShaderOptions(Action::Apply));
        assert!(menu.take_shader_option_actions().is_empty());
    }

    #[test]
    fn back_and_leaving_video_go_to_the_open_screen() {
        if !render_model::SHADER_PACKS_ENABLED {
            return;
        }
        let mut menu = menu();
        menu.activate(MenuAction::Navigate(MenuScreen::Settings));
        menu.activate(MenuAction::SelectRenderer(RendererChoice::ShaderPack(0)));
        menu.activate(MenuAction::ShaderOptions(Action::Open));
        menu.take_shader_option_actions();
        menu.activate(MenuAction::AddBack);
        assert_eq!(
            menu.screen(),
            MenuScreen::Settings,
            "Back stays on Settings"
        );
        menu.activate(MenuAction::SettingsSection(0));
        assert_eq!(
            menu.take_shader_option_actions(),
            [Action::Back, Action::Done]
        );
    }
    /// Opens a ready option editor in the Video section.
    fn editing_menu() -> MenuRuntime {
        let mut menu = menu();
        menu.activate(MenuAction::Navigate(MenuScreen::Settings));
        menu.activate(MenuAction::SettingsSection(
            launcher::menu::settings_sections::SettingSection::Video
                .definition()
                .index,
        ));
        menu.activate(MenuAction::SelectRenderer(RendererChoice::ShaderPack(0)));
        menu.activate(MenuAction::ShaderOptions(Action::Open));
        menu.take_shader_option_actions();
        let mut view = View::loading("Dusk.zip");
        view.status = launcher::shader_options::Status::Ready;
        menu.sync_shader_options(Some(view));
        menu
    }

    #[test]
    fn section_navigation_waits_for_a_successful_option_close() {
        if !render_model::SHADER_PACKS_ENABLED {
            return;
        }
        let mut menu = editing_menu();
        let video = menu.settings.section;
        menu.activate(MenuAction::SettingsSection(0));
        assert_eq!(menu.settings.section, video);
        assert_eq!(menu.take_shader_option_actions(), [Action::Done]);
        let mut view = View::loading("Dusk.zip");
        view.status = launcher::shader_options::Status::SaveFailed("disk full".into());
        menu.sync_shader_options(Some(view));
        assert_eq!(menu.settings.section, video);
        assert!(menu.take_shader_option_actions().is_empty());
        menu.activate(MenuAction::SettingsSection(0));
        assert_eq!(menu.take_shader_option_actions(), [Action::Done]);
        menu.sync_shader_options(None);
        assert_eq!(menu.settings.section, 0);
        assert!(menu.view().shader_options.is_none());
    }

    #[test]
    fn section_navigation_waits_for_an_existing_save_then_requests_close() {
        if !render_model::SHADER_PACKS_ENABLED {
            return;
        }
        let mut menu = editing_menu();
        let video = menu.settings.section;
        let mut view = View::loading("Dusk.zip");
        view.status = launcher::shader_options::Status::Saving;
        menu.sync_shader_options(Some(view.clone()));
        menu.activate(MenuAction::SettingsSection(0));
        assert_eq!(menu.settings.section, video);
        assert!(menu.take_shader_option_actions().is_empty());
        view.status = launcher::shader_options::Status::Ready;
        menu.sync_shader_options(Some(view));
        assert_eq!(menu.take_shader_option_actions(), [Action::Done]);
        assert_eq!(menu.settings.section, video);
        menu.sync_shader_options(None);
        assert_eq!(menu.settings.section, 0);
    }

    #[test]
    fn back_declines_a_join_popup_before_touching_shader_options() {
        if !render_model::SHADER_PACKS_ENABLED {
            return;
        }
        let mut menu = editing_menu();
        menu.push_join_request(7, "Alex".into(), std::time::Duration::ZERO);
        menu.activate(MenuAction::AddBack);
        assert_eq!(menu.take_join_reply(), Some((7, false)));
        assert!(menu.take_shader_option_actions().is_empty());
        assert!(menu.view().shader_options.is_some());
    }
}
