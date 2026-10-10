//! Host adapter for the launcher's optimistic inbox state.
use launcher::menu::inbox::*;

impl super::MenuRuntime {
    /// Applies an inbox action before the account worker collects its queued reports.
    pub(super) fn activate_inbox(&mut self, action: Action) {
        self.feeds.activate_inbox(action);
    }
}

#[cfg(test)]
mod tests {
    use launcher::menu::inbox::Action;
    use {crate::menu::MenuRuntime, launcher::menu::view::MenuHome};

    #[test]
    fn inbox_settings_focus_excludes_the_hidden_category_list() {
        use launcher::menu::{MenuAction, MenuScreen};
        let mut menu = MenuRuntime::new(true, 2, "Test".into());
        menu.screen = MenuScreen::Inbox;
        menu.feeds.inbox_state.filters = true;
        let actions = menu.focus_actions();
        assert!(!actions.iter().any(|action| matches!(
            action,
            MenuAction::Inbox(Action::Category(_)) | MenuAction::Navigate(MenuScreen::Home)
        )));
        for action in [Action::Filters, Action::MarkAllRead, Action::DeleteAllRead] {
            assert!(actions.contains(&MenuAction::Inbox(action)));
        }
    }
    /// Builds a partial inbox page with a service total larger than its loaded rows.
    fn partial_feed() -> MenuHome {
        MenuHome {
            inbox: vec![launcher::menu::view::InboxItem {
                instance_id: "old".into(),
                category: "News".into(),
                unread: true,
                ..Default::default()
            }],
            inbox_counts: [(0, 30), (1, 5)].into(),
            inbox_unread: 35,
            ..Default::default()
        }
    }

    #[test]
    fn expired_opened_message_restores_inbox_keyboard_navigation() {
        use launcher::menu::{MenuAction, MenuScreen};
        let mut menu = MenuRuntime::new(true, 2, "Test".into());
        menu.screen = MenuScreen::Inbox;
        menu.feeds.home = partial_feed();
        menu.activate_inbox(Action::Open(0));
        assert_eq!(menu.focus_actions(), [MenuAction::Inbox(Action::Cancel)]);
        menu.feeds.home.inbox.clear();
        menu.feeds.inbox_state.reconcile(&mut menu.feeds.home);
        assert!(menu.feeds.inbox_state.opened.is_none());
        assert!(
            menu.focus_actions()
                .contains(&MenuAction::Inbox(Action::Category(0)))
        );
        assert!(
            menu.focus_actions()
                .contains(&MenuAction::Navigate(MenuScreen::Home))
        );
    }
}
