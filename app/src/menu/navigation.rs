//! Menu screen navigation over a screen history, as vanilla's scene stack
//! pushes and pops screens, and how the menu sits in the scene stack.

use super::{LocalWorldAction, MenuRuntime, MenuScreen};

impl MenuRuntime {
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
    }

    /// Shows the history's top screen with fresh focus.
    pub(super) fn show_top(&mut self) {
        let screen = self.history.top().unwrap_or(MenuScreen::Home);
        if screen != MenuScreen::Store {
            self.store_snapshot = None;
        }
        self.screen = screen;
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
        if self.screen == MenuScreen::Settings && self.global_resources.settings.is_some() {
            self.global_resource_actions
                .push(crate::global_resources::Action::CloseSettings);
            return;
        }
        if self.dialog.is_some() {
            self.dismiss_accounts();
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
            // Death has no way back; only respawn or leaving ends it.
            MenuScreen::Death => {}
            MenuScreen::Store => self.store_actions.push(crate::store::StoreAction::Back),
            _ if self.history.screens().len() > 1 => {
                self.history.pop();
                self.show_top();
            }
            MenuScreen::Pause if self.death_shown => {
                self.history.reset(MenuScreen::Death);
                self.show_top();
            }
            MenuScreen::Pause => self.set_visible(false),
            _ => {}
        }
    }
}
