//! Keyboard and gamepad focus: the actions each launcher screen cycles
//! through, and moving or activating the focused one.

use super::*;

impl MenuRuntime {
    pub(crate) fn move_focus(&mut self, direction: i32) {
        let actions = self.focus_actions();
        if actions.is_empty() {
            self.focused = 0;
            return;
        }
        let length = actions.len() as i32;
        self.focused = (self.focused as i32 + direction).rem_euclid(length) as usize;
        match actions[self.focused].text_field() {
            Some(field) => self.focus_field(field),
            None => self.field = None,
        }
    }

    pub(crate) fn activate_focused(&mut self) {
        let actions = self.focus_actions();
        // A modal can replace the controls while the old focus index remains.
        self.focused = self.focused.min(actions.len().saturating_sub(1));
        let Some(action) = actions.get(self.focused).copied() else {
            return;
        };
        self.activate(action);
    }

    /// Tracks each visible settings control once, preserving focus across value changes.
    pub(super) fn refresh_settings_focus(&mut self, actions: impl IntoIterator<Item = MenuAction>) {
        if self.screen != MenuScreen::Settings || self.dialog.is_some() {
            self.settings_focus.clear();
            return;
        }
        let previous = self.focus_actions().get(self.focused).copied();
        let mut visible = Vec::new();
        for action in actions {
            let action = match action {
                MenuAction::SettingsOption(index, _) => {
                    let value = self.settings_options.get(usize::from(index));
                    let target = match settings_options::SETTINGS_OPTIONS
                        .get(usize::from(index))
                        .map(|option| option.kind)
                    {
                        Some(settings_options::SettingKind::Toggle) => 1 - value,
                        _ => value,
                    };
                    MenuAction::SettingsOption(index, target)
                }
                MenuAction::SettingsScale(_) => MenuAction::SettingsScale(self.gui_scale_offset),
                action => action,
            };
            if !visible
                .iter()
                .any(|candidate| same_control(*candidate, action))
            {
                visible.push(action);
            }
        }
        if visible.is_empty() {
            return;
        }
        self.focused = previous
            .and_then(|action| {
                visible
                    .iter()
                    .position(|candidate| same_control(*candidate, action))
            })
            .unwrap_or(self.focused.min(visible.len() - 1));
        self.settings_focus = visible;
    }

    /// Adjusts a focused slider or option; otherwise moves to the adjacent control.
    pub(super) fn move_horizontal_focus(&mut self, direction: i32) {
        let focused = self.focus_actions().get(self.focused).copied();
        match focused {
            Some(MenuAction::SettingsOption(index, _)) if self.dialog.is_none() => {
                if let Some(option) = settings_options::SETTINGS_OPTIONS.get(usize::from(index)) {
                    let value = self.settings_options.get(usize::from(index));
                    self.activate(MenuAction::SettingsOption(
                        index,
                        value.saturating_add(direction * option.step),
                    ));
                }
            }
            Some(MenuAction::SettingsScale(_)) if self.dialog.is_none() => {
                self.activate(MenuAction::SettingsScale(
                    self.gui_scale_offset.saturating_add(direction as i8),
                ));
            }
            _ => self.move_focus(direction),
        }
    }

    /// The actions keyboard and gamepad focus cycles through on the current screen.
    pub(super) fn focus_actions(&self) -> Vec<MenuAction> {
        if self.is_connecting() && self.feeds.server_trust.is_some() {
            return vec![
                MenuAction::ServerTrust(true),
                MenuAction::ServerTrust(false),
            ];
        }
        if let Some(dialog) = self.dialog {
            return match dialog {
                MenuDialog::Accounts => {
                    if self.feeds.account_adding {
                        return vec![MenuAction::CancelSignIn];
                    }
                    let mut actions: Vec<_> = self
                        .feeds
                        .accounts
                        .iter()
                        .enumerate()
                        .filter(|(_, account)| {
                            Some(&account.id) != self.feeds.account_active_id.as_ref()
                        })
                        .map(|(index, _)| MenuAction::SwitchAccount(index))
                        .collect();
                    actions.extend([
                        MenuAction::AddAccount,
                        MenuAction::Navigate(MenuScreen::Profile),
                        MenuAction::DismissDialog,
                    ]);
                    actions
                }
                MenuDialog::SettingsResetGroup(group) => vec![
                    MenuAction::SettingsConfirmResetGroup(group),
                    MenuAction::DismissDialog,
                ],
                MenuDialog::SettingsResetBindings(gamepad) => vec![
                    MenuAction::SettingsConfirmResetBindings(gamepad),
                    MenuAction::DismissDialog,
                ],
                MenuDialog::SettingsSupport(super::settings_support::SupportDialog::Help) => vec![
                    MenuAction::SettingsSupport(super::settings_support::SupportAction::Open(
                        super::settings_support::SupportLink::Help,
                    )),
                    MenuAction::DismissDialog,
                ],
                MenuDialog::SettingsSupport(_) => vec![MenuAction::DismissDialog],
                MenuDialog::StorageError => vec![MenuAction::DismissDialog],
                MenuDialog::StorageDelete => vec![
                    MenuAction::SettingsStorage(
                        super::settings_storage::StorageAction::ConfirmDelete,
                    ),
                    MenuAction::DismissDialog,
                ],
                MenuDialog::Exit => vec![MenuAction::ConfirmExit, MenuAction::DismissDialog],
                MenuDialog::RemoveSaved(index) => vec![
                    MenuAction::ConfirmRemoveSaved(index),
                    MenuAction::DismissDialog,
                ],
            };
        }
        let nav = || {
            vec![
                MenuAction::Navigate(MenuScreen::Home),
                MenuAction::Navigate(MenuScreen::Play),
                MenuAction::Navigate(MenuScreen::Social),
                MenuAction::Navigate(MenuScreen::Servers),
                MenuAction::Navigate(MenuScreen::Profile),
                MenuAction::Navigate(MenuScreen::Settings),
                MenuAction::OpenExitDialog,
            ]
        };
        match self.screen {
            MenuScreen::Home => {
                let mut actions = nav();
                actions[4] = MenuAction::OpenAccounts;
                actions.extend((0..self.friends.len().min(1)).map(MenuAction::PlayFriend));
                actions.extend((0..self.realms.len().min(1)).map(MenuAction::PlayRealm));
                actions.extend((0..self.featured.len().min(2)).map(MenuAction::PlayFeatured));
                actions
            }
            MenuScreen::Play => {
                if let Some(actions) = self.local_focus_actions() {
                    return actions;
                }
                let mut actions = nav();
                actions.extend([
                    MenuAction::LocalWorld(LocalWorldAction::BeginCreate),
                    MenuAction::LocalWorld(LocalWorldAction::OpenTemplates),
                ]);
                for index in 0..self.local_worlds.len() {
                    actions.extend([
                        MenuAction::PlayLocalWorld(index),
                        MenuAction::LocalWorld(LocalWorldAction::Edit(index)),
                    ]);
                }
                actions.extend((0..self.friends.len()).map(MenuAction::PlayFriend));
                actions.extend((0..self.realms.len()).map(MenuAction::PlayRealm));
                actions.extend(
                    self.servers
                        .iter()
                        .enumerate()
                        .filter(|(_, server)| server.last_joined_unix > 0)
                        .map(|(index, _)| MenuAction::PlaySaved(index)),
                );
                actions
            }
            MenuScreen::Social => {
                let mut actions = nav();
                actions.push(MenuAction::RefreshCatalog);
                actions.extend((0..self.friends.len()).map(MenuAction::PlayFriend));
                actions
            }
            MenuScreen::Servers => {
                let mut actions = nav();
                actions.extend([
                    MenuAction::SelectServerTab(MenuServerTab::Featured),
                    MenuAction::SelectServerTab(MenuServerTab::Favorites),
                    MenuAction::SelectServerTab(MenuServerTab::Recent),
                    MenuAction::SelectServerTab(MenuServerTab::Saved),
                    MenuAction::PlayAddServer,
                ]);
                match self.server_tab {
                    MenuServerTab::Featured => {
                        actions.extend((0..self.featured.len()).map(MenuAction::PlayFeatured));
                    }
                    MenuServerTab::Favorites => actions.extend(
                        self.servers
                            .iter()
                            .enumerate()
                            .filter(|(_, server)| server.favorite)
                            .map(|(index, _)| MenuAction::PlaySaved(index)),
                    ),
                    MenuServerTab::Recent => actions.extend(
                        self.servers
                            .iter()
                            .enumerate()
                            .filter(|(_, server)| server.last_joined_unix > 0)
                            .map(|(index, _)| MenuAction::PlaySaved(index)),
                    ),
                    MenuServerTab::Saved => {
                        actions.extend((0..self.servers.len()).map(MenuAction::PlaySaved));
                    }
                }
                actions
            }
            MenuScreen::Profile => {
                let mut actions = vec![MenuAction::AddBack];
                // Match view(): an active helper outranks the core's previous report.
                let auth = match (
                    self.auth_process.as_ref().map(AuthSupervisor::state),
                    self.control_auth.as_ref(),
                ) {
                    (Some(state @ (AuthState::Checking | AuthState::AwaitingCode { .. })), _) => {
                        Some(state)
                    }
                    (_, Some(control)) => Some(control),
                    (supervisor, None) => supervisor,
                };
                if matches!(
                    auth,
                    Some(AuthState::Checking | AuthState::AwaitingCode { .. })
                ) {
                    return vec![MenuAction::CancelSignIn];
                }
                if auth == Some(&AuthState::Authenticated) {
                    if self.feeds.profile.unavailable {
                        actions.push(MenuAction::RefreshProfile);
                    } else if self.feeds.profile.loaded {
                        actions.extend([
                            MenuAction::SelectProfileTab(launcher::menu::ProfileTab::Overview),
                            MenuAction::SelectProfileTab(launcher::menu::ProfileTab::Stats),
                        ]);
                        if self.profile_tab == launcher::menu::ProfileTab::Overview
                            && self.feeds.profile.friends.is_some_and(|n| n > 0)
                        {
                            actions.push(MenuAction::Navigate(MenuScreen::Friends));
                        }
                    }
                } else {
                    actions.push(MenuAction::StartSignIn);
                }
                actions
            }
            MenuScreen::Settings if !self.settings_focus.is_empty() => self.settings_focus.clone(),
            MenuScreen::Settings => {
                let mut actions = nav();
                actions.push(MenuAction::SettingsFullscreen(!self.fullscreen));
                actions.extend(
                    self.gui_scale_choices
                        .iter()
                        .copied()
                        .map(MenuAction::SettingsScale),
                );
                actions.push(MenuAction::ToggleRenderMode);
                actions
            }
            MenuScreen::AddServer => vec![
                MenuAction::AddName,
                MenuAction::AddAddress,
                MenuAction::AddPort,
                MenuAction::AddSave,
                MenuAction::AddSaveConnect,
                MenuAction::AddBack,
            ],
            MenuScreen::Pause => vec![
                MenuAction::PauseResume,
                MenuAction::PauseSettings,
                MenuAction::PauseDisconnect,
            ],
            MenuScreen::Death => vec![MenuAction::Respawn, MenuAction::Navigate(MenuScreen::Pause)],
            MenuScreen::Inbox => {
                use super::inbox::{Action, CATEGORIES, category_index};
                if self.feeds.inbox_state.opened.is_some() {
                    return vec![MenuAction::Inbox(Action::Cancel)];
                }
                let mut actions = vec![
                    MenuAction::Navigate(MenuScreen::Home),
                    MenuAction::Inbox(Action::Filters),
                ];
                actions
                    .extend((0..CATEGORIES.len()).map(|i| MenuAction::Inbox(Action::Category(i))));
                if self.feeds.inbox_state.filters {
                    actions.extend([
                        MenuAction::Inbox(Action::MarkAllRead),
                        MenuAction::Inbox(Action::DeleteAllRead),
                    ]);
                }
                for (i, _) in self
                    .feeds
                    .home
                    .inbox
                    .iter()
                    .enumerate()
                    .filter(|(_, item)| {
                        category_index(&item.category) == Some(self.feeds.inbox_state.category)
                    })
                {
                    if self.feeds.inbox_state.delete_pending.is_none() {
                        actions.extend([
                            MenuAction::Inbox(Action::Open(i)),
                            MenuAction::Inbox(Action::Delete(i)),
                        ]);
                    }
                }
                if self.feeds.inbox_state.delete_pending.is_some() {
                    return vec![
                        MenuAction::Inbox(Action::Cancel),
                        MenuAction::Inbox(Action::ConfirmDelete),
                    ];
                }
                actions
            }
            MenuScreen::Friends => vec![MenuAction::Navigate(MenuScreen::Home)],
            MenuScreen::Store => vec![MenuAction::Store(crate::store::StoreAction::Back)],
        }
    }
}

/// Whether two actions identify the same control despite a changed value.
fn same_control(a: MenuAction, b: MenuAction) -> bool {
    match (a, b) {
        (MenuAction::SettingsOption(a, _), MenuAction::SettingsOption(b, _)) => a == b,
        (MenuAction::SettingsScale(_), MenuAction::SettingsScale(_)) => true,
        _ => a == b,
    }
}

#[cfg(test)]
mod review_tests {
    use super::*;
    #[test]
    fn review_settings_focus_reaches_visible_ordinary_controls() {
        let mut menu = MenuRuntime::new(true, 2, "Test".into());
        menu.screen = MenuScreen::Settings;
        let gamma = settings_options::SETTINGS_OPTIONS
            .iter()
            .position(|option| option.name == "gamma")
            .unwrap();
        let value = menu.settings_options.value("gamma");
        menu.settings_focus = vec![MenuAction::SettingsOption(gamma as u16, value)];
        assert_eq!(menu.focus_actions(), menu.settings_focus);
        menu.refresh_settings_focus([
            MenuAction::SettingsOption(gamma as u16, 0),
            MenuAction::SettingsOption(gamma as u16, 100),
        ]);
        assert_eq!(
            menu.focus_actions().len(),
            1,
            "a segmented slider is one focus control"
        );
        menu.move_horizontal_focus(1);
        assert_eq!(menu.settings_options.value("gamma"), value + 1);
    }
}
