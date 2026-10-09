//! Focus retained by the screen history and restored only on a return request.

use super::super::{MenuAction, MenuRuntime, MenuScreen, focus::same_control};
use client_ui::ui_runtime::presentation::UiPresentationRuntime;

#[derive(Debug, Default)]
pub(in crate::menu) struct NavigationFocus {
    retained: Vec<(MenuScreen, MenuAction)>,
    pending: bool,
    requested_action: Option<MenuAction>,
    home_actions: Option<Vec<MenuAction>>,
}

impl NavigationFocus {
    /// Uses the painted Home controls once they are available, including an empty screen.
    pub(in crate::menu) fn home_actions(&self) -> Option<&[MenuAction]> {
        self.home_actions.as_deref()
    }

    /// Newer explicit input updates the outstanding request before its validation.
    pub(in crate::menu) fn record_input(&mut self, action: Option<MenuAction>) {
        if self.pending {
            self.requested_action = action;
        }
    }

    /// A fresh screen keeps its normal entry policy until explicitly returned to.
    pub(super) fn enter(&mut self) {
        self.pending = false;
        self.requested_action = None;
    }
}

impl MenuRuntime {
    /// Reads popup ownership from borrowed runtime state without copying catalogs or feeds.
    pub(in crate::menu) fn navigation_popup_open(&self) -> bool {
        #[cfg(feature = "developer-control")]
        if self.sign_in_fixture.is_some() {
            return self.dialog.is_some();
        }
        if self.dialog.is_some()
            || self.dressing_room.editor.is_some()
            || (self.is_connecting() && self.feeds.server_trust.is_some())
            || self.join_request_pending()
        {
            return true;
        }
        if self.presentation_accounts || !self.sign_in_prompt_layer_available() {
            return false;
        }
        let auth = self.current_auth();
        matches!(auth.as_ref(), super::super::AuthState::AwaitingCode { .. })
            || (self.sign_in_requested
                && matches!(
                    auth.as_ref(),
                    super::super::AuthState::Checking | super::super::AuthState::Failed(_)
                ))
    }

    /// Retains base-screen focus without treating modal controls as screen controls.
    pub(super) fn remember_navigation_focus(&mut self) {
        if self.navigation_popup_open() {
            return;
        }
        if let Some(action) = self.focus_actions().get(self.focused).copied() {
            self.navigation_focus
                .retained
                .retain(|(screen, _)| *screen != self.screen);
            self.navigation_focus.retained.push((self.screen, action));
        }
    }

    /// Requests restoration after a history return; pointer outline admission stays untouched.
    pub(super) fn restore_navigation_focus(&mut self) {
        self.navigation_focus
            .retained
            .retain(|(screen, _)| self.history.screens().contains(screen));
        self.navigation_focus.pending = true;
        self.navigation_focus.requested_action = self.retained_navigation_action();
        if let Some(action) = self.retained_navigation_action() {
            self.focus_pointer(action);
        }
    }

    /// Finds the retained action for the current screen, never one from a popped screen.
    fn retained_navigation_action(&self) -> Option<MenuAction> {
        self.navigation_focus
            .retained
            .iter()
            .find_map(|(screen, action)| (*screen == self.screen).then_some(*action))
    }

    /// Consumes focus data only after the matching screen and popup context have been painted.
    pub(in crate::menu) fn refresh_presented_focus(
        &mut self,
        presentation: &UiPresentationRuntime,
    ) {
        let Some((screen, drawn_popup)) = presentation.drawn_menu_context() else {
            return;
        };
        if screen != self.screen {
            return;
        }
        let popup = self.navigation_popup_open();
        if popup {
            self.navigation_focus.pending = false;
        }
        if drawn_popup != popup {
            return;
        }
        self.refresh_settings_focus(presentation.visible_menu_actions());
        let (targets, landmarks) = presentation.settings_focus_geometry();
        self.refresh_settings_focus_geometry(targets, landmarks);
        if !self.navigation_focus.pending && (self.screen != MenuScreen::Home || popup) {
            return;
        }
        let visible: Vec<_> = presentation.visible_menu_actions().collect();
        if self.screen == MenuScreen::Home && !popup {
            let previous = self.focus_actions().get(self.focused).copied();
            let mut home_actions = Vec::with_capacity(visible.len());
            for action in visible.iter().copied() {
                if !home_actions
                    .iter()
                    .any(|candidate| same_control(*candidate, action))
                {
                    home_actions.push(action);
                }
            }
            self.focused = previous
                .and_then(|action| {
                    home_actions
                        .iter()
                        .position(|candidate| same_control(*candidate, action))
                })
                .unwrap_or(0);
            self.navigation_focus.home_actions = Some(home_actions);
        }
        if !self.navigation_focus.pending {
            return;
        }
        self.navigation_focus.pending = false;
        let actions = self.focus_actions();
        let available = |action: MenuAction| {
            visible
                .iter()
                .any(|candidate| same_control(*candidate, action))
        };
        let restored = self
            .navigation_focus
            .requested_action
            .take()
            .filter(|action| available(*action));
        let fallback = actions.iter().copied().find(|action| available(*action));
        if let Some(action) = restored.or(fallback) {
            self.focus_pointer(action);
        } else {
            // No enabled control can take focus until a later presentation publishes one.
            self.focused = actions.len();
        }
    }
}
