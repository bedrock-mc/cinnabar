//! Native account feeds and session-owned trust presentation.

use super::MenuRuntime;
use launcher::menu::view::{MenuHome, MenuProfile};
#[cfg(test)]
use launcher::menu::view::{ServerDetails, ServerTrustPrompt};

use launcher_host::account_control::AccountControl;

impl MenuRuntime {
    /// Answers the shown trust prompt; "Don't Trust" also cancels the join.
    pub fn answer_server_trust(&mut self, trusted: bool) {
        let Some(prompt) = self.feeds.server_trust.take() else {
            return;
        };
        self.feeds.server_trust_answer = Some((prompt, trusted));
        if !trusted {
            self.intents.disconnect = true;
        }
    }

    /// Shows the current session's trust question and returns its answer to that owner.
    pub fn sync_session_trust(&mut self, source: &dyn super::server_trust::TrustSource) {
        if let Some((prompt, trusted)) = self.feeds.server_trust_answer.take() {
            source.answer(prompt.id, trusted);
        }
        self.feeds.server_trust = source.prompt().filter(|_| self.is_connecting());
    }

    /// Discards the retired session's prompt and answer before another session can reuse its id.
    pub fn forget_session_trust(&mut self) {
        self.feeds.server_trust_answer = None;
        self.feeds.server_trust = None;
    }

    /// Applies account-worker results and forwards requests to the same native service owner.
    pub fn sync_account_control(&mut self, control: &mut dyn AccountControl) {
        if let Some(featured) = control.featured() {
            let selected = self
                .feeds
                .selected_featured
                .and_then(|index| self.featured.get(index))
                .map(|card| card.address.clone());
            self.feeds.details.extend(
                featured
                    .iter()
                    .map(|row| (row.card.address.clone(), row.details.clone())),
            );
            #[cfg(feature = "native-networking")]
            {
                self.native_gatherings = featured
                    .iter()
                    .filter_map(|row| {
                        row.gathering
                            .clone()
                            .map(|selection| (row.card.address.clone(), selection))
                    })
                    .collect();
            }
            self.featured
                .replace(featured.into_iter().map(|row| row.card).collect());
            if let Some(address) = selected {
                self.feeds.selected_featured = self
                    .featured
                    .iter()
                    .position(|card| card.address == address);
            }
            if self
                .feeds
                .selected_featured
                .is_some_and(|index| index >= self.featured.len())
            {
                self.feeds.selected_featured = None;
            }
        }
        if std::mem::take(&mut self.feeds.profile_refresh_requested) {
            control.refresh_profile();
        }
        if let Some(profile) = control.profile() {
            self.feeds.profile = profile;
        }
        if let Some(home) = control.home() {
            self.feeds.home = home;
            self.feeds.inbox_state.reconcile(&mut self.feeds.home);
        }
        for event in std::mem::take(&mut self.feeds.inbox_state.pending) {
            control.report_message(event);
        }
        let targets = if self.visible && !self.is_connecting() {
            ping_targets(
                self.featured
                    .iter()
                    .map(|server| server.address.as_str())
                    .chain(self.servers.iter().map(|server| server.address.as_str())),
            )
        } else {
            Vec::new()
        };
        control.set_ping_targets(targets);
        control.set_player_counts_visible(self.experience_counts_visible());
        // A round updates the rows it covered; others keep their last pong.
        if let Some(pings) = control.pings() {
            self.feeds.pings.extend(pings);
        }
        control.set_joining(self.is_connecting());
        if let Some(status) = control.account_status() {
            self.apply_control_auth(status);
        }
        if self.sign_out_requested {
            self.sign_out_requested = false;
            control.sign_out();
            self.finish_sign_out();
        }
        self.sync_invites(control);
        self.sync_realm_membership(control);
    }

    /// Queues sign-out when account services are absent; the storage worker owns its leases.
    pub fn sign_out_locally(&mut self) {
        if self.sign_out_requested {
            self.sign_out_requested = false;
            self.finish_sign_out();
        }
    }

    /// Retires validated presentation and queues the original credential-store sign-out.
    fn finish_sign_out(&mut self) {
        self.cancel_join();
        self.retry_target = None;
        self.accounts.operation = Some(launcher_host::accounts::Operation::SignOut);
        self.feeds.account_active_id = None;
        #[cfg(feature = "native-networking")]
        {
            self.auth_process = None;
        }
        self.auth_attempted = true;
        self.control_auth = None;
        self.realms.replace(Vec::new());
        self.friends.replace(Vec::new());
        for details in self.feeds.details.values_mut() {
            details.player_count = None;
        }
        self.feeds.profile = MenuProfile::default();
        self.feeds.home = MenuHome::default();
        self.catalog_message = None;
        self.enter(launcher::menu::MenuScreen::Profile);
    }

    /// An experience details panel owns the live count subscription for its visible lifetime.
    fn experience_counts_visible(&self) -> bool {
        self.visible
            && !self.is_connecting()
            && self.screen == launcher::menu::MenuScreen::Servers
            && self.feeds.selected_saved.is_none()
            && self
                .featured
                .get(self.feeds.selected_featured.unwrap_or(0))
                .is_some_and(|server| !launcher::menu::pingable(&server.address))
    }
}

/// The featured and saved server addresses a ping round covers, each once. Experiences have no
/// server until joined, so they are left out.
fn ping_targets<'a>(addresses: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    addresses
        .into_iter()
        .filter(|address| {
            !address.is_empty() && launcher::menu::pingable(address) && seen.insert(*address)
        })
        .map(str::to_owned)
        .collect()
}

#[cfg(test)]
mod tests {
    use launcher::menu::MenuAction;
    use {super::*, launcher::menu::auth::AuthState, launcher::menu::view::MenuServerCard};

    struct Catalog(Option<Vec<(MenuServerCard, ServerDetails)>>);
    impl AccountControl for Catalog {
        fn account_status(&mut self) -> Option<AuthState> {
            None
        }
        fn sign_out(&mut self) -> bool {
            false
        }
        fn featured(&mut self) -> Option<Vec<launcher_host::account_control::FeaturedRow>> {
            self.0
                .take()
                .map(|rows| rows.into_iter().map(Into::into).collect())
        }
    }

    #[test]
    fn creator_catalog_refresh_keeps_the_picked_server_when_the_order_changes() {
        let card = |address: &str| MenuServerCard {
            name: address.into(),
            address: address.into(),
            caption: String::new(),
            image_path: String::new(),
            icon: None,
        };
        let mut menu = MenuRuntime::new(true, 2, "Fixture".into());
        menu.featured = vec![card("first.test:19132"), card("picked.test:19132")].into();
        menu.feeds.selected_featured = Some(1);
        let previous = menu.view();
        let mut control = Catalog(Some(vec![
            (
                card("picked.test:19132"),
                ServerDetails {
                    group: "creator".into(),
                    ..Default::default()
                },
            ),
            (card("first.test:19132"), ServerDetails::default()),
        ]));
        menu.sync_account_control(&mut control);
        assert_eq!(menu.feeds.selected_featured, Some(0));
        assert_eq!(previous.featured[1].address, "picked.test:19132");
        assert_eq!(previous.feeds.selected_featured, Some(1));
        assert!(!previous.featured.shares_storage(&menu.view().featured));
        assert!(previous.realms.shares_storage(&menu.view().realms));
        assert!(previous.friends.shares_storage(&menu.view().friends));
        assert_eq!(
            menu.featured[menu.feeds.selected_featured.unwrap()].address,
            "picked.test:19132"
        );
        control.0 = Some(vec![(card("first.test:19132"), ServerDetails::default())]);
        menu.sync_account_control(&mut control);
        assert_eq!(menu.feeds.selected_featured, None);
        assert_eq!(previous.featured.len(), 2);
    }

    #[test]
    fn equal_account_catalog_deliveries_keep_list_storage_and_refresh_details() {
        let mut menu = MenuRuntime::new(true, 2, "Fixture".into());
        let card = MenuServerCard {
            name: "Featured".into(),
            address: "featured.example.test:19132".into(),
            caption: String::new(),
            image_path: String::new(),
            icon: None,
        };
        let mut control = Catalog(Some(vec![(card.clone(), ServerDetails::default())]));
        menu.sync_account_control(&mut control);
        let previous = menu.view();
        control.0 = Some(vec![(
            card,
            ServerDetails {
                player_count: Some(42),
                ..Default::default()
            },
        )]);
        menu.sync_account_control(&mut control);
        let current = menu.view();
        assert!(previous.featured.shares_storage(&current.featured));
        assert_eq!(
            current.feeds.details[&current.featured[0].address].player_count,
            Some(42)
        );
        assert_eq!(
            previous.feeds.details[&previous.featured[0].address].player_count,
            None
        );
    }

    /// An experience has no server until it is joined, so a featured experience is never pinged.
    #[test]
    fn experiences_are_not_pinged() {
        let addresses = [
            "gathering/5b0f2bd4-8a8e-4a6e-9d3c-0a1b2c3d4e5f",
            "play.example.test",
            "play.example.test",
        ];
        assert_eq!(ping_targets(addresses), ["play.example.test"]);
    }

    #[test]
    fn player_counts_are_requested_only_for_visible_experience_details() {
        let mut menu = MenuRuntime::new(true, 2, "Steve".to_owned());
        menu.featured.push(MenuServerCard {
            name: "Experience".into(),
            address: format!("{}example", launcher::menu::EXPERIENCE_ADDRESS_PREFIX),
            caption: String::new(),
            image_path: String::new(),
            icon: None,
        });
        assert!(!menu.experience_counts_visible());
        menu.enter(launcher::menu::MenuScreen::Servers);
        assert!(menu.experience_counts_visible());
        menu.feeds.selected_saved = Some(0);
        assert!(!menu.experience_counts_visible());
        menu.feeds.selected_saved = None;
        menu.visible = false;
        assert!(!menu.experience_counts_visible());
        menu.visible = true;
        menu.observe_session(client_state::SessionStatus { connecting: true });
        assert!(!menu.experience_counts_visible());
    }

    struct SessionSource {
        prompt: std::cell::RefCell<Option<ServerTrustPrompt>>,
        answers: std::cell::RefCell<Vec<(u64, bool)>>,
    }

    impl super::super::server_trust::TrustSource for SessionSource {
        fn prompt(&self) -> Option<ServerTrustPrompt> {
            self.prompt.borrow().clone()
        }
        fn answer(&self, id: u64, trusted: bool) {
            self.prompt.borrow_mut().take();
            self.answers.borrow_mut().push((id, trusted));
        }
    }

    // An answer belongs to the original session, including across prompt-id reuse.
    #[test]
    fn session_trust_answers_return_only_to_the_original_session() {
        let mut menu = MenuRuntime::new(true, 2, "Steve".to_owned());
        menu.observe_session(client_state::SessionStatus { connecting: true });
        let asked = ServerTrustPrompt {
            id: 1,
            url: "http://127.0.0.1:19132".into(),
        };
        let session = SessionSource {
            prompt: std::cell::RefCell::new(Some(asked.clone())),
            answers: Default::default(),
        };
        let frame = |menu: &mut MenuRuntime| menu.sync_session_trust(&session);
        frame(&mut menu);
        assert_eq!(menu.view().feeds.server_trust.as_ref(), Some(&asked));
        frame(&mut menu);
        assert_eq!(menu.view().feeds.server_trust.as_ref(), Some(&asked));
        menu.activate(MenuAction::ServerTrust(true));
        frame(&mut menu);
        assert!(menu.view().feeds.server_trust.is_none());
        assert_eq!(*session.answers.borrow(), vec![(1, true)]);
        // Retiring a session drops its unanswered prompt before the next session.
        session.prompt.replace(Some(asked.clone()));
        menu.sync_session_trust(&session);
        menu.activate(MenuAction::ServerTrust(false));
        menu.forget_session_trust();
        let next = SessionSource {
            prompt: std::cell::RefCell::new(None),
            answers: Default::default(),
        };
        menu.sync_session_trust(&next);
        assert!(
            next.answers.borrow().is_empty(),
            "an old answer reached the next session"
        );
    }

    #[test]
    fn a_join_to_a_featured_server_carries_its_logo_however_the_address_is_written() {
        let mut menu = MenuRuntime::new(true, 2, "Steve".to_owned());
        menu.featured = vec![MenuServerCard {
            name: "The Hive".into(),
            address: "geo.hivebedrock.network".into(),
            caption: String::new(),
            image_path: String::new(),
            icon: None,
        }]
        .into();
        menu.feeds.details.insert(
            "geo.hivebedrock.network".into(),
            ServerDetails {
                logo_url: "https://cdn.example/hive.png".into(),
                ..Default::default()
            },
        );
        let badge = menu
            .featured_badge("geo.hivebedrock.network:19132")
            .unwrap();
        assert_eq!(
            (badge.image_url.as_str(), badge.name.as_str()),
            ("https://cdn.example/hive.png", "The Hive")
        );
        assert!(menu.featured_badge("play.example.net").is_none());
        menu.feeds.details.clear();
        assert!(menu.featured_badge("geo.hivebedrock.network").is_none());
    }
}
