//! Keyboard and gamepad focus: the actions each launcher screen cycles
//! through, and moving or activating the focused one.

use super::*;
use launcher::menu::view::{SettingsFocusAxis, SettingsFocusLandmark, SettingsFocusTarget};

mod settings;
pub(super) use settings::SettingsFocusGeometry;

impl MenuRuntime {
    pub(super) fn focus_pointer(&mut self, action: MenuAction) {
        if let Some(index) = self.focus_actions().iter().position(|candidate| {
            if self.settings_dropdown.is_some() || self.settings_scale_picker {
                *candidate == action
            } else {
                same_control(*candidate, action)
            }
        }) {
            self.focused = index;
            self.settings_focus_geometry.reset_anchor();
            self.settings_focus_geometry.remember(action);
            self.retain_settings_slider_selection();
        }
    }

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
        self.settings_focus_geometry.remember(actions[self.focused]);
        self.retain_settings_slider_selection();
    }

    pub(crate) fn activate_focused(&mut self) {
        let actions = self.focus_actions();
        // A modal can replace the controls while the old focus index remains.
        self.focused = self.focused.min(actions.len().saturating_sub(1));
        let Some(action) = actions.get(self.focused).copied() else {
            return;
        };
        if self.screen == MenuScreen::Settings
            && self.dialog.is_none()
            && self.settings_dropdown.is_none()
            && !self.settings_scale_picker
            && let MenuAction::SettingsOption(index, _) = action
        {
            match settings_options::SETTINGS_OPTIONS
                .get(usize::from(index))
                .map(|option| option.kind)
            {
                Some(settings_options::SettingKind::Slider) => {
                    self.settings_slider_selected =
                        (self.settings_slider_selected != Some(index)).then_some(index);
                    return;
                }
                Some(settings_options::SettingKind::Toggle) => {
                    self.activate_from_navigation(self.live_settings_action(action));
                    return;
                }
                _ => {}
            }
        }
        let action = self.live_settings_action(action);
        self.activate_from_navigation(action);
    }

    pub(super) fn live_settings_action(&self, action: MenuAction) -> MenuAction {
        match action {
            MenuAction::SettingsOption(index, _)
                if settings_options::SETTINGS_OPTIONS
                    .get(usize::from(index))
                    .is_some_and(|option| {
                        matches!(option.kind, settings_options::SettingKind::Toggle)
                    }) =>
            {
                MenuAction::SettingsOption(index, 1 - self.settings_options.get(usize::from(index)))
            }
            MenuAction::SettingsFullscreen(_) => MenuAction::SettingsFullscreen(!self.fullscreen),
            action => action,
        }
    }

    /// Tracks each visible settings control once, preserving focus across value changes.
    pub(super) fn refresh_settings_focus(&mut self, actions: impl IntoIterator<Item = MenuAction>) {
        if self.screen != MenuScreen::Settings
            || self.dialog.is_some()
            || self.sign_in_focus().is_some()
        {
            self.settings_focus.clear();
            self.settings_slider_selected = None;
            self.settings_focus_geometry = SettingsFocusGeometry::default();
            return;
        }
        let previous = self.focus_actions().get(self.focused).copied();
        let picker = self.settings_dropdown.is_some() || self.settings_scale_picker;
        let mut visible = Vec::new();
        for action in actions {
            let action = match action {
                action if picker => action,
                MenuAction::SettingsOption(index, choice) => {
                    let value = self.settings_options.get(usize::from(index));
                    let target = match settings_options::SETTINGS_OPTIONS
                        .get(usize::from(index))
                        .map(|option| option.kind)
                    {
                        Some(settings_options::SettingKind::Toggle) => 1 - value,
                        Some(settings_options::SettingKind::Slider) => value,
                        _ => choice,
                    };
                    MenuAction::SettingsOption(index, target)
                }
                MenuAction::SettingsFullscreen(_) => {
                    MenuAction::SettingsFullscreen(!self.fullscreen)
                }
                action => action,
            };
            if !visible.iter().any(|candidate| {
                if picker {
                    *candidate == action
                } else {
                    same_control(*candidate, action)
                }
            }) {
                visible.push(action);
            }
        }
        if visible.is_empty() {
            self.settings_focus.clear();
            self.focused = 0;
            self.settings_slider_selected = None;
            return;
        }
        self.focused = previous
            .and_then(|action| {
                visible.iter().position(|candidate| {
                    if picker {
                        *candidate == action
                    } else {
                        same_control(*candidate, action)
                    }
                })
            })
            .or_else(|| {
                previous.and_then(|action| self.responsive_settings_focus(action, &visible))
            })
            .unwrap_or(self.focused.min(visible.len() - 1));
        self.settings_focus = visible;
        self.retain_settings_slider_selection();
    }

    /// Direction callbacks belong to selected sliders; other controls navigate spatially.
    pub(super) fn move_horizontal_focus(&mut self, direction: i32) {
        self.move_directional_focus(SettingsFocusAxis::Horizontal, direction);
    }

    pub(super) fn move_directional_focus(&mut self, axis: SettingsFocusAxis, direction: i32) {
        if !self.is_connecting()
            && matches!(self.dialog, None | Some(MenuDialog::Accounts))
            && self.sign_in_focus().is_some()
        {
            self.move_focus(direction);
            return;
        }
        self.retain_settings_slider_selection();
        if axis == SettingsFocusAxis::Horizontal
            && self.screen == MenuScreen::Settings
            && self.dialog.is_none()
            && let Some(index) = self.settings_slider_selected
            && let Some(option) = settings_options::SETTINGS_OPTIONS.get(usize::from(index))
        {
            let value = self.settings_options.get(usize::from(index));
            let next = self
                .settings_options
                .offset_value(usize::from(index), direction)
                .clamp(option.min, option.max);
            if next != value {
                self.activate_from_navigation(MenuAction::SettingsOption(index, next));
            }
            return;
        }
        if self.screen != MenuScreen::Settings
            || self.dialog.is_some()
            || !self.settings_focus_geometry.native
        {
            self.move_focus(direction);
            return;
        }
        let Some(current) = self.settings_focus.get(self.focused).copied() else {
            return;
        };
        let Some(next) = self
            .settings_focus_geometry
            .directional(current, axis, direction)
        else {
            return;
        };
        if let Some(index) = self
            .settings_focus
            .iter()
            .position(|action| same_control(*action, next))
        {
            self.focused = index;
            match next.text_field() {
                Some(field) => self.focus_field(field),
                None => self.field = None,
            }
            self.settings_focus_geometry.remember(next);
            self.retain_settings_slider_selection();
        }
    }

    pub(super) fn clear_settings_slider_selection(&mut self) -> bool {
        self.settings_slider_selected.take().is_some()
    }

    fn retain_settings_slider_selection(&mut self) {
        let retained = self.screen == MenuScreen::Settings
            && self.dialog.is_none()
            && self.settings_dropdown.is_none()
            && !self.settings_scale_picker
            && self.settings_slider_selected.is_some_and(|selected| {
                matches!(self.focus_actions().get(self.focused), Some(MenuAction::SettingsOption(index, _)) if *index == selected)
            });
        if !retained {
            self.settings_slider_selected = None;
        }
    }

    fn responsive_settings_focus(
        &self,
        previous: MenuAction,
        visible: &[MenuAction],
    ) -> Option<usize> {
        let replacement = match previous {
            MenuAction::SettingsScale(_) => MenuAction::SettingsScalePicker,
            MenuAction::SettingsScalePicker => {
                MenuAction::SettingsScale(self.gui_scale_display_offset)
            }
            MenuAction::SettingsOption(index, _) => MenuAction::SettingsDropdown(index),
            MenuAction::SettingsDropdown(index) => {
                MenuAction::SettingsOption(index, self.settings_options.get(usize::from(index)))
            }
            _ => return None,
        };
        visible.iter().position(|action| *action == replacement)
    }

    pub(super) fn refresh_settings_focus_geometry(
        &mut self,
        targets: &[SettingsFocusTarget],
        landmarks: &[SettingsFocusLandmark],
    ) {
        if landmarks.is_empty() && !self.settings_focus_geometry.native {
            return;
        }
        let previous = self.focus_actions().get(self.focused).copied();
        let had_native_focus = self.settings_focus_geometry.native;
        self.refresh_settings_focus(targets.iter().map(|target| target.action));
        if self.screen != MenuScreen::Settings
            || self.dialog.is_some()
            || self.sign_in_focus().is_some()
        {
            return;
        }
        self.settings_focus_geometry.update(targets, landmarks);
        let previous_valid = previous
            .is_some_and(|previous| self.settings_focus_geometry.target(previous).is_some());
        let responsive = previous
            .and_then(|previous| self.responsive_settings_focus(previous, &self.settings_focus));
        if (!had_native_focus || !previous_valid)
            && responsive.is_none()
            && let Some(entry) = self.settings_focus_geometry.entry()
            && let Some(index) = self
                .settings_focus
                .iter()
                .position(|action| same_control(*action, entry))
        {
            self.focused = index;
        }
        if let Some(action) = self.settings_focus.get(self.focused).copied() {
            self.settings_focus_geometry.remember(action);
        }
        self.retain_settings_slider_selection();
    }

    /// The actions keyboard and gamepad focus cycles through on the current screen.
    pub(super) fn focus_actions(&self) -> Vec<MenuAction> {
        if self.is_connecting() && self.feeds.server_trust.is_some() {
            return vec![
                MenuAction::ServerTrust(true),
                MenuAction::ServerTrust(false),
            ];
        }
        if !self.is_connecting()
            && self.dialog.is_none()
            && let Some(actions) = self.sign_in_focus()
        {
            return actions;
        }
        if let Some(dialog) = self.dialog {
            return match dialog {
                MenuDialog::Accounts => {
                    if let Some(actions) = self.sign_in_focus() {
                        return actions;
                    }
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
        if let Some(actions) = self.join_request_focus_actions() {
            return actions;
        }
        if self.disconnect_message.is_some() && !self.is_connecting() {
            return if self.can_reconnect() {
                vec![MenuAction::Reconnect, MenuAction::DismissDialog]
            } else {
                vec![MenuAction::DismissDialog]
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
                let realms = actions
                    .iter()
                    .position(|action| *action == MenuAction::Navigate(MenuScreen::Social))
                    .expect("home navigation includes Realms");
                actions.insert(realms + 1, MenuAction::Store(crate::store::OPEN));
                actions.push(MenuAction::Navigate(MenuScreen::DressingRoom));
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
            MenuScreen::DressingRoom => self.dressing_room_focus(),
            MenuScreen::Profile => {
                let mut actions = vec![MenuAction::AddBack];
                if self.presentation_accounts
                    || self.current_auth().as_ref() == &AuthState::Authenticated
                {
                    let profile = self.presented_profile();
                    if profile.unavailable {
                        actions.push(MenuAction::RefreshProfile);
                    } else if profile.loaded {
                        actions.extend([
                            MenuAction::SelectProfileTab(launcher::menu::ProfileTab::Overview),
                            MenuAction::SelectProfileTab(launcher::menu::ProfileTab::Stats),
                        ]);
                        if self.profile_tab == launcher::menu::ProfileTab::Overview
                            && profile.friends.is_some_and(|n| n > 0)
                        {
                            actions.push(MenuAction::Navigate(MenuScreen::Friends));
                        }
                    }
                } else {
                    actions.push(MenuAction::StartSignIn);
                }
                actions
            }
            MenuScreen::Settings if self.global_resources.settings.is_some() => {
                self.settings_focus.clone()
            }
            MenuScreen::Settings
                if self.settings_focus_geometry.native || !self.settings_focus.is_empty() =>
            {
                self.settings_focus.clone()
            }
            MenuScreen::Settings => {
                let mut actions = nav();
                actions.push(MenuAction::SettingsFullscreen(!self.fullscreen));
                actions.extend(
                    self.gui_scale_choices
                        .iter()
                        .map(|choice| MenuAction::SettingsScale(choice.offset)),
                );
                actions.push(MenuAction::ToggleRenderMode);
                actions
            }
            MenuScreen::AddServer => {
                let mut actions = vec![
                    MenuAction::AddName,
                    MenuAction::AddAddress,
                    MenuAction::AddPort,
                    MenuAction::AddBack,
                ];
                if !self.name.as_str().trim().is_empty() && !self.address.as_str().trim().is_empty()
                {
                    actions.extend([MenuAction::AddSave, MenuAction::AddSaveConnect]);
                }
                actions
            }
            MenuScreen::Pause => {
                let mut actions = vec![
                    MenuAction::PauseResume,
                    MenuAction::PauseSettings,
                    MenuAction::Navigate(MenuScreen::DressingRoom),
                    MenuAction::PauseDisconnect,
                ];
                if self.hosting_world() {
                    actions.push(MenuAction::Invite(launcher::menu::invite::Action::Open));
                }
                actions
            }
            MenuScreen::Death => vec![MenuAction::Respawn, MenuAction::Navigate(MenuScreen::Pause)],
            MenuScreen::Inbox => {
                use super::inbox::{Action, CATEGORIES, category_index};
                if self.feeds.inbox_state.delete_pending.is_some() {
                    return vec![
                        MenuAction::Inbox(Action::Cancel),
                        MenuAction::Inbox(Action::ConfirmDelete),
                    ];
                }
                if self.feeds.inbox_state.opened.is_some() {
                    return vec![MenuAction::Inbox(Action::Cancel)];
                }
                if self.feeds.inbox_state.filters {
                    return vec![
                        MenuAction::Inbox(Action::Filters),
                        MenuAction::Inbox(Action::MarkAllRead),
                        MenuAction::Inbox(Action::DeleteAllRead),
                    ];
                }
                let mut actions = vec![
                    MenuAction::Navigate(MenuScreen::Home),
                    MenuAction::Inbox(Action::Filters),
                ];
                actions
                    .extend((0..CATEGORIES.len()).map(|i| MenuAction::Inbox(Action::Category(i))));
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
                actions
            }
            MenuScreen::Friends => vec![MenuAction::Navigate(MenuScreen::Home)],
            MenuScreen::Store => vec![MenuAction::Store(crate::store::StoreAction::Back)],
            MenuScreen::Invite => self.invite_focus_actions(),
        }
    }
}

/// Dropdown radio rows are distinct controls; slider stops share one control.
fn same_control(a: MenuAction, b: MenuAction) -> bool {
    match (a, b) {
        (MenuAction::SettingsOption(a, av), MenuAction::SettingsOption(b, bv)) if a == b => {
            av == bv
                || matches!(
                    settings_options::SETTINGS_OPTIONS
                        .get(usize::from(a))
                        .map(|option| option.kind),
                    Some(
                        settings_options::SettingKind::Toggle
                            | settings_options::SettingKind::Slider
                    )
                )
        }
        (MenuAction::SettingsFullscreen(_), MenuAction::SettingsFullscreen(_)) => true,
        _ => a == b,
    }
}

#[cfg(test)]
mod review_tests {
    use super::*;

    fn native_rows(menu: &mut MenuRuntime, rows: &[(MenuAction, [f32; 4])]) {
        let rect = |[left, top, right, bottom]: [f32; 4]| {
            ui::UiRect::new(
                ui::UiPoint::new(left, top).unwrap(),
                ui::UiPoint::new(right, bottom).unwrap(),
            )
            .unwrap()
        };
        let targets = rows
            .iter()
            .map(|(action, bounds)| SettingsFocusTarget {
                action: *action,
                bounds: rect(*bounds),
                landmark: Some(0),
            })
            .collect::<Vec<_>>();
        let landmarks = [SettingsFocusLandmark {
            id: 0,
            parent: None,
            bounds: rect([0.0, 0.0, 100.0, 100.0]),
            scroll_axis: None,
            delegate: None,
            delegate_landmark: None,
            remember: false,
            trap: false,
            focus_control_disabled: false,
        }];
        menu.refresh_settings_focus_geometry(&targets, &landmarks);
    }

    #[test]
    fn native_inline_options_navigate_without_commit_and_keep_each_choice_focus() {
        let mut menu = MenuRuntime::new(true, 2, "Player".into());
        menu.screen = MenuScreen::Settings;
        let index = settings_options::SETTINGS_OPTIONS
            .iter()
            .position(|option| option.name == "third_person")
            .unwrap() as u16;
        let original = menu.settings_options.get(usize::from(index));
        let rows = [
            (MenuAction::AddBack, [0.0, 0.0, 90.0, 10.0]),
            (
                MenuAction::SettingsOption(index, 0),
                [0.0, 20.0, 30.0, 30.0],
            ),
            (
                MenuAction::SettingsOption(index, 1),
                [30.0, 20.0, 60.0, 30.0],
            ),
            (
                MenuAction::SettingsOption(index, 2),
                [60.0, 20.0, 90.0, 30.0],
            ),
        ];
        native_rows(&mut menu, &rows);
        menu.focus_pointer(MenuAction::SettingsOption(index, 0));
        menu.move_horizontal_focus(1);
        assert_eq!(
            menu.focus_actions()[menu.focused],
            MenuAction::SettingsOption(index, 1)
        );
        assert_eq!(menu.settings_options.get(usize::from(index)), original);
        menu.activate_focused();
        assert_eq!(menu.settings_options.get(usize::from(index)), 1);
        native_rows(&mut menu, &rows);
        assert_eq!(
            menu.focus_actions()[menu.focused],
            MenuAction::SettingsOption(index, 1)
        );
        menu.focus_pointer(MenuAction::SettingsOption(index, 2));
        menu.move_directional_focus(SettingsFocusAxis::Vertical, -1);
        assert_eq!(
            menu.focus_actions()[menu.focused],
            MenuAction::AddBack,
            "Up leaves the row instead of stepping to an inline sibling"
        );
        menu.move_directional_focus(SettingsFocusAxis::Vertical, -1);
        assert_eq!(
            menu.focus_actions()[menu.focused],
            MenuAction::AddBack,
            "directional navigation does not wrap"
        );
    }

    #[test]
    fn native_gui_scale_choices_commit_only_on_select() {
        let mut menu = MenuRuntime::new(true, 2, "Player".into());
        menu.screen = MenuScreen::Settings;
        menu.sync_gui_scale(
            0,
            ui::DesktopGuiScale::for_window([1920, 1080])
                .choices()
                .collect(),
        );
        let rows = [
            (MenuAction::SettingsScale(-2), [0.0, 0.0, 30.0, 10.0]),
            (MenuAction::SettingsScale(-1), [30.0, 0.0, 60.0, 10.0]),
            (MenuAction::SettingsScale(0), [60.0, 0.0, 90.0, 10.0]),
        ];
        native_rows(&mut menu, &rows);
        menu.focus_pointer(MenuAction::SettingsScale(-2));
        menu.move_horizontal_focus(1);
        assert_eq!(
            menu.focus_actions()[menu.focused],
            MenuAction::SettingsScale(-1)
        );
        assert_eq!(menu.gui_scale_offset(), 0);
        menu.activate_focused();
        assert_eq!(menu.gui_scale_offset(), -1);
        native_rows(&mut menu, &rows);
        assert_eq!(
            menu.focus_actions()[menu.focused],
            MenuAction::SettingsScale(-1)
        );
    }

    #[test]
    fn native_slider_selection_exits_on_back_focus_loss_and_empty_layout() {
        let mut menu = MenuRuntime::new(true, 2, "Player".into());
        menu.screen = MenuScreen::Settings;
        let index = settings_options::SETTINGS_OPTIONS
            .iter()
            .position(|option| option.name == "gamma")
            .unwrap() as u16;
        let value = menu.settings_options.get(usize::from(index));
        let rows = [
            (
                MenuAction::SettingsOption(index, value),
                [0.0, 0.0, 90.0, 10.0],
            ),
            (MenuAction::AddBack, [0.0, 20.0, 90.0, 30.0]),
        ];
        native_rows(&mut menu, &rows);
        menu.activate_focused();
        assert_eq!(menu.settings_slider_selected, Some(index));
        assert_eq!(menu.settings_control_activation, None);
        assert!(menu.clear_settings_slider_selection());
        assert_eq!(menu.settings_options.get(usize::from(index)), value);
        menu.activate_focused();
        menu.move_directional_focus(SettingsFocusAxis::Vertical, 1);
        assert_eq!(menu.settings_slider_selected, None);
        assert_eq!(menu.focus_actions()[menu.focused], MenuAction::AddBack);
        menu.focus_pointer(MenuAction::SettingsOption(index, value));
        menu.activate_focused();
        menu.refresh_settings_focus_geometry(&[], &[]);
        assert_eq!(menu.settings_slider_selected, None);
        assert!(menu.focus_actions().is_empty());
        menu.move_horizontal_focus(1);
        menu.activate_focused();
        assert_eq!(menu.settings_options.get(usize::from(index)), value);
    }
    #[test]
    fn toggle_activation_reads_the_current_value_between_paints_and_arrows_do_not_commit() {
        let mut menu = MenuRuntime::new(true, 2, "Player".to_owned());
        menu.activate(MenuAction::Navigate(MenuScreen::Settings));
        let index = settings_options::SETTINGS_OPTIONS
            .iter()
            .position(|option| option.name == "keyboard_mouse_autojump")
            .unwrap();
        let initial = menu.settings_options.get(index);
        menu.refresh_settings_focus([MenuAction::SettingsOption(index as u16, 1 - initial)]);
        menu.activate_focused();
        assert_eq!(menu.settings_options.get(index), 1 - initial);
        menu.activate_focused();
        assert_eq!(
            menu.settings_options.get(index),
            initial,
            "each keydown reads the live value before another frame is drawn"
        );
        menu.set_option(index as u16, 0);
        menu.refresh_settings_focus([
            MenuAction::SettingsOption(index as u16, 1),
            MenuAction::AddBack,
        ]);
        menu.move_horizontal_focus(1);
        assert_eq!(
            menu.settings_options.get(index),
            0,
            "switch arrows navigate without committing a value"
        );
    }
    #[test]
    fn gui_scale_picker_commits_once_and_keeps_focus_through_responsive_layouts() {
        let mut menu = MenuRuntime::new(true, 2, "Test".into());
        menu.screen = MenuScreen::Settings;
        menu.sync_gui_scale(
            0,
            ui::DesktopGuiScale::for_window([1920, 1080])
                .choices()
                .collect(),
        );
        menu.settings_focus = vec![MenuAction::SettingsScale(0)];
        menu.refresh_settings_focus([MenuAction::AddBack, MenuAction::SettingsScalePicker]);
        assert_eq!(
            menu.focus_actions()[menu.focused],
            MenuAction::SettingsScalePicker
        );
        menu.activate_focused();
        menu.refresh_settings_focus([
            MenuAction::SettingsScalePicker,
            MenuAction::SettingsScale(-2),
            MenuAction::SettingsScale(-1),
            MenuAction::SettingsScale(0),
        ]);
        menu.move_focus(-1);
        assert_eq!(menu.gui_scale_offset(), 0);
        menu.activate_focused();
        assert_eq!(menu.gui_scale_offset(), -1);
        assert!(!menu.settings_scale_picker);
        menu.refresh_settings_focus([MenuAction::AddBack, MenuAction::SettingsScalePicker]);
        assert_eq!(
            menu.focus_actions()[menu.focused],
            MenuAction::SettingsScalePicker
        );
        menu.refresh_settings_focus([
            MenuAction::AddBack,
            MenuAction::SettingsScale(-2),
            MenuAction::SettingsScale(-1),
            MenuAction::SettingsScale(0),
        ]);
        assert_eq!(
            menu.focus_actions()[menu.focused],
            MenuAction::SettingsScale(-1)
        );
        menu.activate(MenuAction::SettingsScalePicker);
        menu.go_back();
        assert!(!menu.settings_scale_picker);
        assert_eq!(menu.gui_scale_offset(), -1);
        assert_eq!(menu.screen, MenuScreen::Settings);
    }
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
        assert_eq!(
            menu.settings_options.value("gamma"),
            value,
            "an unselected slider does not adjust"
        );
        menu.activate_focused();
        assert_eq!(menu.settings_slider_selected, Some(gamma as u16));
        assert_eq!(
            menu.settings_control_activation, None,
            "selecting adjustment mode does not commit a value"
        );
        menu.move_horizontal_focus(1);
        assert_eq!(menu.settings_options.value("gamma"), value + 1);
    }

    #[test]
    fn settings_picker_retains_choices_and_commits_only_when_activated() {
        let mut menu = MenuRuntime::new(true, 2, "Test".into());
        menu.screen = MenuScreen::Settings;
        let index = settings_options::SETTINGS_OPTIONS
            .iter()
            .position(|option| option.name == "third_person")
            .unwrap() as u16;
        let original = menu.settings_options.get(usize::from(index));
        menu.activate(MenuAction::SettingsDropdown(index));
        let choices = std::iter::once(MenuAction::SettingsDropdown(index))
            .chain((0..3).map(|value| MenuAction::SettingsOption(index, value)))
            .collect::<Vec<_>>();
        menu.refresh_settings_focus(choices.clone());
        assert_eq!(menu.focus_actions(), choices);
        assert_eq!(menu.focused, original as usize + 1);
        menu.move_focus(1);
        assert_eq!(menu.settings_options.get(usize::from(index)), original);
        menu.activate_focused();
        assert_eq!(
            menu.settings_options.get(usize::from(index)),
            (original + 1) % 3
        );
        assert_eq!(menu.settings_dropdown, None);
        menu.refresh_settings_focus([MenuAction::AddBack, MenuAction::SettingsDropdown(index)]);
        assert_eq!(
            menu.focus_actions()[menu.focused],
            MenuAction::SettingsDropdown(index)
        );
    }

    #[test]
    fn settings_picker_back_keeps_the_settings_page_and_value() {
        let mut menu = MenuRuntime::new(true, 2, "Test".into());
        menu.activate(MenuAction::Navigate(MenuScreen::Settings));
        let index = settings_options::SETTINGS_OPTIONS
            .iter()
            .position(|option| option.name == "third_person")
            .unwrap() as u16;
        let value = menu.settings_options.get(usize::from(index));
        menu.activate(MenuAction::SettingsDropdown(index));
        menu.go_back();
        assert_eq!(menu.screen, MenuScreen::Settings);
        assert_eq!(menu.settings_dropdown, None);
        assert_eq!(menu.settings_options.get(usize::from(index)), value);
    }

    #[test]
    fn settings_pack_overlay_with_no_controls_keeps_background_input_closed() {
        let mut menu = MenuRuntime::new(true, 2, "Test".into());
        menu.screen = MenuScreen::Settings;
        let index = settings_options::SETTINGS_OPTIONS
            .iter()
            .position(|option| option.name == "gamma")
            .unwrap() as u16;
        let value = menu.settings_options.get(usize::from(index));
        menu.settings_focus = vec![MenuAction::SettingsOption(index, value)];
        std::sync::Arc::make_mut(&mut menu.global_resources).settings = Some(0);
        menu.refresh_settings_focus([]);
        menu.move_horizontal_focus(1);
        menu.activate_focused();
        assert!(menu.focus_actions().is_empty());
        assert_eq!(menu.settings_options.get(usize::from(index)), value);
    }

    /// Keyboard and controller navigation retain each radio row's selected value.
    #[test]
    fn settings_dropdown_focus_reaches_and_activates_each_radio_choice() {
        for name in ["animations", "graphics_mode"] {
            let mut menu = MenuRuntime::new(true, 2, "Test".into());
            menu.screen = MenuScreen::Settings;
            let index = settings_options::SETTINGS_OPTIONS
                .iter()
                .position(|option| option.name == name)
                .unwrap() as u16;
            let actions = [
                MenuAction::SettingsDropdown(index),
                MenuAction::SettingsOption(index, 0),
                MenuAction::SettingsOption(index, 1),
            ];
            menu.refresh_settings_focus(actions);
            assert_eq!(
                menu.focus_actions(),
                actions,
                "each radio row is a distinct focus control"
            );
            menu.focused = 0;
            menu.activate_focused();
            assert_eq!(menu.settings_dropdown, Some(index));
            menu.refresh_settings_focus(actions);
            menu.focus_pointer(actions[1]);
            assert_eq!(menu.focus_actions()[menu.focused], actions[1]);
            menu.move_focus(1);
            assert_eq!(menu.focus_actions()[menu.focused], actions[2]);
            menu.refresh_settings_focus(actions);
            assert_eq!(menu.focus_actions()[menu.focused], actions[2]);
            menu.activate_focused();
            assert_eq!(menu.settings_options.value(name), 1);
            assert_eq!(menu.settings_dropdown, None);
            menu.focused = 0;
            menu.activate_focused();
            menu.refresh_settings_focus(actions);
            menu.focus_pointer(actions[1]);
            menu.activate_focused();
            assert_eq!(menu.settings_options.value(name), 0);
            assert_eq!(menu.settings_dropdown, None);
        }
    }
}
