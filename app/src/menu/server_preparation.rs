//! The one visible server eligible for a transport head start before Join.

use super::{MenuAction, MenuRuntime, MenuScreen, MenuServerTab, SavedServer};

pub(super) fn navigation_actions() -> [MenuAction; 7] {
    [
        MenuAction::Navigate(MenuScreen::Home),
        MenuAction::Navigate(MenuScreen::Play),
        MenuAction::Navigate(MenuScreen::Social),
        MenuAction::Navigate(MenuScreen::Servers),
        MenuAction::Navigate(MenuScreen::Profile),
        MenuAction::Navigate(MenuScreen::Settings),
        MenuAction::OpenExitDialog,
    ]
}

impl MenuRuntime {
    pub(super) fn server_focus_actions(&self) -> impl Iterator<Item = MenuAction> + '_ {
        navigation_actions()
            .into_iter()
            .chain([
                MenuAction::SelectServerTab(MenuServerTab::Featured),
                MenuAction::SelectServerTab(MenuServerTab::Favorites),
                MenuAction::SelectServerTab(MenuServerTab::Recent),
                MenuAction::SelectServerTab(MenuServerTab::Saved),
                MenuAction::PlayAddServer,
            ])
            .chain(
                (0..self.featured.len())
                    .filter(|_| self.server_tab == MenuServerTab::Featured)
                    .map(MenuAction::PlayFeatured),
            )
            .chain(
                (0..self.gatherings.len())
                    .filter(|_| self.server_tab == MenuServerTab::Featured)
                    .map(MenuAction::PlayGathering),
            )
            .chain(
                self.servers
                    .iter()
                    .enumerate()
                    .filter(|(_, server)| {
                        self.server_tab != MenuServerTab::Featured && self.server_is_shown(server)
                    })
                    .map(|(index, _)| MenuAction::PlaySaved(index)),
            )
    }

    pub(super) fn server_preparation_address(&self) -> Option<&str> {
        if !self.visible
            || !self.is_launcher()
            || self.screen != MenuScreen::Servers
            || self.dialog.is_some()
            || self.account_change_pending()
        {
            return None;
        }
        self.hovered
            .and_then(|action| self.server_action_address(action))
            .or_else(|| {
                self.server_focus_actions()
                    .nth(self.focused)
                    .and_then(|action| self.server_action_address(action))
            })
            .or_else(|| {
                self.feeds.selected_saved.and_then(|index| {
                    self.servers
                        .get(index)
                        .filter(|server| self.server_is_shown(server))
                        .map(|server| Some(server.address.as_str()))
                })
            })
            .or_else(|| {
                (self.server_tab == MenuServerTab::Featured)
                    .then_some(self.feeds.selected_featured)
                    .flatten()
                    .and_then(|index| self.featured_address(index))
            })
            .unwrap_or_else(|| self.default_server_address())
            .filter(|address| !address.trim().is_empty())
    }

    fn server_action_address(&self, action: MenuAction) -> Option<Option<&str>> {
        match action {
            MenuAction::SelectFeatured(index) | MenuAction::PlayFeatured(index) => {
                (self.server_tab == MenuServerTab::Featured)
                    .then(|| self.featured_address(index))
                    .flatten()
            }
            MenuAction::SelectSaved(index) | MenuAction::PlaySaved(index) => self
                .servers
                .get(index)
                .filter(|server| self.server_is_shown(server))
                .map(|server| Some(server.address.as_str())),
            MenuAction::PlayGathering(index) => (self.server_tab == MenuServerTab::Featured
                && index < self.gatherings.len())
            .then_some(None),
            _ => None,
        }
    }

    fn featured_address(&self, index: usize) -> Option<Option<&str>> {
        (index < self.featured.len() + self.gatherings.len()).then(|| {
            self.featured
                .get(index)
                .map(|server| server.address.as_str())
        })
    }

    fn server_is_shown(&self, server: &SavedServer) -> bool {
        match self.server_tab {
            MenuServerTab::Featured | MenuServerTab::Saved => true,
            MenuServerTab::Favorites => server.favorite,
            MenuServerTab::Recent => server.last_joined_unix > 0,
        }
    }

    fn default_server_address(&self) -> Option<&str> {
        if self.server_tab == MenuServerTab::Featured {
            if let Some(server) = self.featured.first() {
                return Some(server.address.as_str());
            }
            if !self.gatherings.is_empty() {
                return None;
            }
        }
        self.servers
            .iter()
            .find(|server| self.server_is_shown(server))
            .map(|server| server.address.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn menu_with_servers() -> MenuRuntime {
        let mut menu = MenuRuntime::new(true, 2, "Steve".to_owned());
        menu.enter(MenuScreen::Servers);
        menu.servers = ["saved.test", "favorite.test"]
            .into_iter()
            .enumerate()
            .map(|(index, address)| SavedServer {
                name: address.into(),
                address: address.into(),
                favorite: index == 1,
                last_joined_unix: 0,
            })
            .collect();
        menu.featured = vec![super::super::MenuServerCard {
            name: "Featured".into(),
            address: "featured.test".into(),
            caption: String::new(),
            image_path: String::new(),
            icon: None,
        }];
        menu
    }

    #[test]
    fn stale_pointer_target_does_not_replace_the_current_tab_selection() {
        let mut menu = menu_with_servers();
        menu.activate(MenuAction::SelectServerTab(MenuServerTab::Saved));
        menu.hovered = Some(MenuAction::PlayFeatured(0));
        assert_eq!(menu.server_preparation_address(), Some("saved.test"));
        menu.activate(MenuAction::SelectServerTab(MenuServerTab::Favorites));
        menu.hovered = Some(MenuAction::SelectSaved(0));
        assert_eq!(menu.server_preparation_address(), Some("favorite.test"));
        menu.hovered = Some(MenuAction::SelectSaved(99));
        assert_eq!(menu.server_preparation_address(), Some("favorite.test"));
    }

    #[test]
    fn invalid_persisted_selection_uses_the_displayed_default_server() {
        let mut menu = menu_with_servers();
        menu.feeds.selected_saved = Some(menu.servers.len());
        menu.feeds.selected_featured = Some(menu.featured.len());
        assert_eq!(menu.server_preparation_address(), Some("featured.test"));
        menu.featured.clear();
        assert_eq!(menu.server_preparation_address(), Some("saved.test"));
    }

    #[test]
    fn selected_gatherings_never_prepare_an_unselected_saved_server() {
        let mut menu = menu_with_servers();
        menu.gatherings = menu.featured.clone();
        menu.hovered = Some(MenuAction::SelectFeatured(menu.featured.len()));
        assert_eq!(menu.server_preparation_address(), None);
        menu.hovered = None;
        menu.featured.clear();
        assert_eq!(menu.server_preparation_address(), None);
        menu.hovered = Some(MenuAction::PlayGathering(0));
        assert_eq!(menu.server_preparation_address(), None);
        assert!(menu.take_join_intent().is_none());
    }

    #[test]
    fn unchanged_server_highlight_allocates_no_work() {
        let mut menu = MenuRuntime::new(true, 2, "Steve".to_owned());
        menu.enter(MenuScreen::Servers);
        menu.servers = vec![SavedServer {
            name: "Saved".into(),
            address: "selected.test".into(),
            favorite: false,
            last_joined_unix: 0,
        }];
        let before = crate::tests::alloc_count::thread_allocations();
        for _ in 0..32 {
            assert_eq!(menu.server_preparation_address(), Some("selected.test"));
        }
        assert_eq!(crate::tests::alloc_count::thread_allocations(), before);
    }
}
