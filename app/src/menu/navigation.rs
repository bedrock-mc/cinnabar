//! Menu screen navigation over a screen history, as vanilla's scene stack
//! pushes and pops screens, and how the menu sits in the scene stack.

mod focus;
pub(super) use focus::NavigationFocus;

use super::{LocalWorldAction, MenuAction, MenuRuntime, MenuScreen};

impl MenuRuntime {
    pub(super) fn remember_session_origin(&mut self) {
        if self.over_world() || self.is_connecting() || self.disconnect_message.is_some() {
            return;
        }
        self.session_origin = self.history.screens().iter().rev().copied().find(|screen| {
            matches!(
                screen,
                MenuScreen::Play | MenuScreen::Social | MenuScreen::Servers
            )
        });
    }

    pub(super) fn show_session_origin(&mut self, fallback: MenuScreen) {
        let screen = self.session_origin.unwrap_or(fallback);
        self.navigation_focus = NavigationFocus::default();
        self.history.reset(MenuScreen::Home);
        if screen != MenuScreen::Home {
            self.history.push(screen);
        }
        self.show_top();
    }

    /// Leaving a world reveals the Play page retained when it was joined.
    pub(crate) fn show_after_disconnect(&mut self) {
        self.reset_death();
        self.retry_target = None;
        self.show_session_origin(MenuScreen::Home);
    }

    /// Whether a visible menu opened over the session's world (its history
    /// starts at pause or death) rather than the launcher; true while hidden.
    pub(crate) fn over_world(&self) -> bool {
        !self.visible
            || matches!(
                self.history.screens().first(),
                Some(MenuScreen::Pause | MenuScreen::Death)
            )
    }

    /// The visible screen, which the scene stack puts over the game.
    pub(crate) fn scene(&self) -> Option<MenuScreen> {
        self.visible.then_some(self.screen)
    }

    /// Opens `screen` over the current one, or returns to it when it is already
    /// open below; a tab of an open vanilla screen takes that screen's place.
    pub(super) fn enter(&mut self, screen: MenuScreen) {
        use launcher::menu::menu_reference;
        self.remember_navigation_focus();
        let returning = screen != self.screen && self.history.screens().contains(&screen);
        let same = |open: MenuScreen| {
            open == screen
                || menu_reference(open).is_some_and(|r| menu_reference(screen) == Some(r))
        };
        match self
            .history
            .screens()
            .iter()
            .copied()
            .find(|open| same(*open))
        {
            Some(open) => {
                self.history.pop_back_to(open);
                self.history.pop();
                self.history.push(screen);
            }
            None => self.history.push(screen),
        }
        self.show_top();
        if returning {
            self.restore_navigation_focus();
        }
    }

    /// Shows the history's top screen with fresh focus.
    pub(super) fn show_top(&mut self) {
        let screen = self.history.top().unwrap_or(MenuScreen::Home);
        self.close_realm_membership();
        if screen == MenuScreen::Death
            && self.screen != MenuScreen::Death
            && self.death_shown
            && self.death_presentation.controls_ready()
        {
            self.death_presentation.return_seconds = Some(0.0);
        }
        if screen != MenuScreen::Store {
            self.store_snapshot = None;
        }
        self.screen = screen;
        self.navigation_focus.enter();
        if screen != MenuScreen::DressingRoom && self.dressing_room.editor.is_some() {
            std::sync::Arc::make_mut(&mut self.dressing_room).editor = None;
        }
        self.settings_focus_geometry = super::focus::SettingsFocusGeometry::default();
        self.settings_slider_selected = None;
        self.settings_focus.clear();
        if screen == MenuScreen::Profile {
            self.feeds.profile_refresh_requested = true;
        }
        self.focused = 0;
        self.hovered = None;
        self.field = None;
        self.dialog = None;
        self.message = None;
        self.visible = true;
    }

    /// Returns to the screen below; an in-game root closes the menu.
    pub(super) fn go_back(&mut self) {
        if self.realm_membership.state.is_some() {
            self.activate_realm_membership(launcher::menu::realm_membership::Action::Back);
            return;
        }
        if !self.is_connecting()
            && matches!(self.dialog, None | Some(super::MenuDialog::Accounts))
            && self.sign_in_focus().is_some()
        {
            self.activate(MenuAction::CancelSignIn);
            return;
        }
        // Back on a join request's popup declines it, as vanilla's modal escape.
        if self.join_request_prompted() {
            self.answer_join_request(false);
            return;
        }
        if self.dressing_room.editor.is_some() {
            self.activate(super::MenuAction::DressingRoom(
                launcher::dressing_room::Action::Cancel,
            ));
            return;
        }
        if self.screen == MenuScreen::Settings && self.global_resources.settings.is_some() {
            self.global_resource_actions
                .push(crate::global_resources::Action::CloseSettings);
            return;
        }
        if self.dialog.is_some() {
            self.dismiss_accounts();
            return;
        }
        if self.disconnect_message.is_some() && !self.is_connecting() {
            self.dismiss_disconnect();
            return;
        }
        if self.screen == MenuScreen::Settings && self.settings_scale_picker {
            self.activate(MenuAction::SettingsScalePicker);
            return;
        }
        if self.screen == MenuScreen::Settings
            && let Some(index) = self.settings_dropdown
        {
            self.activate(MenuAction::SettingsDropdown(index));
            return;
        }
        if self.local_screen_open() {
            self.queue_local_action(LocalWorldAction::Back);
            return;
        }
        // Back on the server trust question declines it.
        if self.is_connecting() && self.feeds.server_trust.is_some() {
            self.answer_server_trust(false);
            return;
        }
        // Back on the join progress screen is its cancel button, where vanilla offers one.
        if self.is_connecting() {
            self.intents.disconnect |= self.feeds.join.cancellable();
            return;
        }
        match self.screen {
            MenuScreen::Death => self.activate(MenuAction::OpenDeathGameMenu),
            MenuScreen::Store => self.store_actions.push(crate::store::StoreAction::Back),
            _ if self.history.screens().len() > 1 => {
                self.history.pop();
                self.show_top();
                self.restore_navigation_focus();
            }
            MenuScreen::Pause if self.death_shown => {
                self.navigation_focus = NavigationFocus::default();
                self.history.reset(MenuScreen::Death);
                self.show_top();
            }
            MenuScreen::Pause => self.set_visible(false),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod focus_tests;
