//! The menu's view of the core's account control surface, so the play and
//! sign-in screens never see the transport. Without a launcher core the
//! account catalog and the auth supervisor keep feeding the menu.

use super::{AuthState, MenuFriendCard, MenuRealmCard, MenuRuntime, MenuServerCard};
use launcher::menu::view::{JoinStage, MenuHome, MenuProfile, PingInfo, ServerDetails};

/// Control method names the implementation calls.
#[allow(dead_code, reason = "named for the core-relay control clients")]
pub(crate) mod method {
    pub(crate) const REALMS_LIST: &str = "realms_list.v1";
    pub(crate) const FRIENDS_LIST: &str = "friends_list.v1";
    pub(crate) const CONNECT: &str = "connect.v1";
    pub(crate) const ACCOUNT_STATUS: &str = "account_status.v1";
    pub(crate) const SIGN_OUT: &str = "sign_out.v1";
}

/// An account-surface event pushed by the core.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum AccountEvent {
    /// The sign-in state changed (device code shown, signed in, signed out).
    #[allow(dead_code, reason = "auth changes arrive as polled status")]
    Auth(AuthState),
    /// The live session ended; the reason shows on the disconnect screen.
    Disconnected { reason: String },
}

/// What the play and sign-in screens need from the core.
pub(crate) trait AccountControl {
    /// `account_status.v1`: the current sign-in state, when known.
    fn account_status(&mut self) -> Option<AuthState>;
    /// `realms_list.v1`: joinable realms, or `None` while unavailable.
    fn realms(&mut self) -> Option<Vec<MenuRealmCard>>;
    /// `friends_list.v1`: friend worlds, or `None` while unavailable.
    fn friends(&mut self) -> Option<Vec<MenuFriendCard>>;
    /// `sign_out.v1`; `true` once the core accepted it.
    fn sign_out(&mut self) -> bool;
    /// The next pending account event, if any.
    fn poll_event(&mut self) -> Option<AccountEvent>;
    /// `featured_servers.v1`: cards plus their info-panel details, when fetched.
    fn featured(&mut self) -> Option<Vec<(MenuServerCard, ServerDetails)>> {
        None
    }
    /// `gatherings.v1`: joinable gatherings with their details, when fetched.
    fn gatherings(&mut self) -> Option<Vec<(MenuServerCard, ServerDetails)>> {
        None
    }
    /// `profile.v1`: the signed-in profile, when fetched.
    fn profile(&mut self) -> Option<MenuProfile> {
        None
    }
    /// Requests an immediate profile retry from the feed worker.
    fn refresh_profile(&mut self) {}
    /// `home.v1`: the start screen's service data, when fetched.
    fn home(&mut self) -> Option<MenuHome> {
        None
    }
    /// Reports an inbox interaction through the core messaging session.
    fn report_message(&mut self, _event: protocol::launcher_control::MessageEvent) {}
    /// The server rows `ping.v1` keeps fresh while the launcher shows them.
    fn set_ping_targets(&mut self, _targets: Vec<String>) {}
    /// Warms the explicitly selected server; `None` cancels a selection that is no longer shown.
    fn prepare_selected_server(&mut self, _address: Option<&str>) {}
    /// Pongs from the latest ping round, keyed by address.
    fn pings(&mut self) -> Option<Vec<(String, PingInfo)>> {
        None
    }
    /// The core's stage of preparing the current join; `None` once it hands over.
    fn join_stage(&mut self) -> Option<JoinStage> {
        None
    }
    /// Whether the menu is connecting, which speeds up event polling.
    fn set_joining(&mut self, _joining: bool) {}
}

impl MenuRuntime {
    /// Pull the core's account state into the menu: lists replace the catalog's,
    /// the status overrides the auth supervisor's, events surface on screen, and
    /// a pending sign-out request is sent.
    pub(crate) fn sync_account_control(&mut self, control: &mut dyn AccountControl) {
        if let Some(realms) = control.realms() {
            self.realms = realms;
        }
        if let Some(mut friends) = control.friends() {
            // The friends service can list one session twice; show each host's world once.
            let mut seen = std::collections::HashSet::new();
            friends.retain(|friend| seen.insert((friend.xuid.clone(), friend.world_name.clone())));
            self.friends = friends;
        }
        if let Some(featured) = control.featured() {
            self.feeds.details.extend(
                featured
                    .iter()
                    .map(|(card, details)| (card.address.clone(), details.clone())),
            );
            self.featured = featured.into_iter().map(|(card, _)| card).collect();
            if self
                .feeds
                .selected_featured
                .is_some_and(|index| index >= self.featured.len())
            {
                self.feeds.selected_featured = None;
            }
        }
        if let Some(gatherings) = control.gatherings() {
            self.feeds.details.extend(
                gatherings
                    .iter()
                    .map(|(card, details)| (card.address.clone(), details.clone())),
            );
            self.gatherings = gatherings.into_iter().map(|(card, _)| card).collect();
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
            let mut seen = std::collections::HashSet::new();
            // Gatherings have no server until joined, so only featured and saved servers are pinged.
            self.featured
                .iter()
                .map(|server| server.address.clone())
                .chain(self.servers.iter().map(|server| server.address.clone()))
                .filter(|address| !address.is_empty() && seen.insert(address.clone()))
                .collect()
        } else {
            Vec::new()
        };
        control.set_ping_targets(targets);
        if !self.is_connecting() {
            let selected =
                (self.visible && self.is_launcher() && self.screen == super::MenuScreen::Servers)
                    .then(|| {
                        self.feeds
                            .selected_saved
                            .and_then(|index| self.servers.get(index))
                            .map(|server| server.address.as_str())
                            .or_else(|| {
                                self.feeds
                                    .selected_featured
                                    .and_then(|index| self.featured.get(index))
                                    .map(|server| server.address.as_str())
                            })
                    })
                    .flatten()
                    .filter(|address| !address.trim().is_empty());
            control.prepare_selected_server(selected);
        }
        // A round updates the rows it covered; others keep their last pong.
        if let Some(pings) = control.pings() {
            self.feeds.pings.extend(pings);
        }
        control.set_joining(self.is_connecting());
        if self.is_connecting() {
            self.feeds.join.observe(control.join_stage());
        }
        if let Some(status) = control.account_status() {
            self.control_auth = Some(status);
        }
        while let Some(event) = control.poll_event() {
            match event {
                AccountEvent::Auth(state) => self.control_auth = Some(state),
                AccountEvent::Disconnected { reason } => {
                    self.disconnect_message = Some(super::disconnect::from_server(&reason));
                }
            }
        }
        if std::mem::take(&mut self.sign_out_requested) {
            control.prepare_selected_server(None);
            control.sign_out();
            self.finish_sign_out();
        }
    }

    /// Sign out without a launcher core: the saved tokens are removed here.
    pub(crate) fn sign_out_locally(&mut self) {
        if std::mem::take(&mut self.sign_out_requested) {
            let _ = std::fs::remove_file(self.layout.auth_cache());
            self.finish_sign_out();
        }
    }

    /// Forget the validated sign-in and return to the signed-out profile; the
    /// launcher core then restarts offline and signing in again runs the
    /// device-code helper.
    fn finish_sign_out(&mut self) {
        self.accounts.operation = Some(super::accounts::Operation::SignOut);
        self.feeds.account_active_id = None;
        self.auth_process = None;
        self.auth_attempted = true;
        self.control_auth = None;
        self.stop_catalog();
        self.catalog_started = false;
        self.realms.clear();
        self.friends.clear();
        self.feeds.profile = MenuProfile::default();
        self.feeds.home = MenuHome::default();
        self.catalog_message = None;
        self.enter(super::MenuScreen::Profile);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fake {
        events: Vec<AccountEvent>,
        signed_out: bool,
        prepared: Vec<Option<String>>,
    }

    impl AccountControl for Fake {
        fn prepare_selected_server(&mut self, address: Option<&str>) {
            self.prepared.push(address.map(str::to_owned));
        }
        fn account_status(&mut self) -> Option<AuthState> {
            Some(AuthState::Authenticated)
        }
        fn realms(&mut self) -> Option<Vec<MenuRealmCard>> {
            None
        }
        fn friends(&mut self) -> Option<Vec<MenuFriendCard>> {
            Some(vec![MenuFriendCard {
                gamertag: "Alex".into(),
                world_name: "Base".into(),
                members: "1 players".into(),
                xuid: "1".into(),
            }])
        }
        fn sign_out(&mut self) -> bool {
            self.signed_out = true;
            true
        }
        fn poll_event(&mut self) -> Option<AccountEvent> {
            self.events.pop()
        }
    }

    struct Staged(Option<JoinStage>);

    impl AccountControl for Staged {
        fn account_status(&mut self) -> Option<AuthState> {
            None
        }
        fn realms(&mut self) -> Option<Vec<MenuRealmCard>> {
            None
        }
        fn friends(&mut self) -> Option<Vec<MenuFriendCard>> {
            None
        }
        fn sign_out(&mut self) -> bool {
            false
        }
        fn poll_event(&mut self) -> Option<AccountEvent> {
            None
        }
        fn join_stage(&mut self) -> Option<JoinStage> {
            self.0
        }
    }

    // The core's stages drive the join until its report vanishes at the handoff,
    // and a failed join's Disconnect lands on the disconnect screen in vanilla's words.
    #[test]
    fn join_progress_follows_the_core_until_handoff() {
        use launcher::menu::view::{JoinKind, JoinProgress};
        let mut menu = MenuRuntime::new(true, 2, "Steve".to_owned());
        menu.observe_session(crate::session::SessionStatus {
            connecting: true,
            owns_directory: false,
        });
        menu.feeds.join = JoinProgress::new(JoinKind::Realm);
        let mut control = Staged(None);
        let mut step = |menu: &mut MenuRuntime, stage| {
            control.0 = stage;
            menu.sync_account_control(&mut control);
            menu.view().feeds.join.stage
        };
        assert_eq!(step(&mut menu, None), JoinStage::Connecting);
        assert_eq!(step(&mut menu, Some(JoinStage::Realm)), JoinStage::Realm);
        let downloading = |received_bytes| JoinStage::Packs {
            done: 0,
            total: 2,
            received_bytes,
            total_bytes: 100,
        };
        assert_eq!(step(&mut menu, Some(downloading(10))), downloading(10));
        assert_eq!(step(&mut menu, Some(downloading(60))), downloading(60));
        assert_eq!(step(&mut menu, None), JoinStage::Generating);

        let error = crate::runtime::network::session_failure_display(
            "Bedrock session failed: Server disconnected during login: Unknown",
            Some(&protocol::ServerDisconnectEvent {
                reason: "Unknown".to_owned(),
                message: Some("disconnectionScreen.cantConnectToRealm".to_owned()),
                filtered_message: None,
            }),
        );
        assert!(menu.absorb_session_failure(&error));
        let view = menu.view();
        assert_eq!(
            super::super::disconnect::describe(&view.disconnect_message.unwrap()).body,
            super::super::disconnect::DisconnectBody::Key("disconnectionScreen.cantConnectToRealm")
        );
    }

    #[test]
    fn control_state_feeds_the_menu_view() {
        assert_eq!(method::SIGN_OUT, "sign_out.v1");
        let mut menu = MenuRuntime::new(true, 2, "Steve".to_owned());
        let mut control = Fake {
            events: vec![AccountEvent::Disconnected {
                reason: "Server closed".into(),
            }],
            signed_out: false,
            prepared: Vec::new(),
        };
        menu.sync_account_control(&mut control);
        let view = menu.view();
        assert_eq!(view.auth_state, AuthState::Authenticated);
        assert_eq!(view.friends.len(), 1);
        let error = view.disconnect_message.as_deref().unwrap();
        assert_eq!(
            super::super::disconnect::describe(error).body,
            super::super::disconnect::DisconnectBody::Server("Server closed".to_owned())
        );
        menu.activate(super::super::MenuAction::SignOut);
        menu.sync_account_control(&mut control);
        assert!(control.signed_out);
        assert!(menu.friends.is_empty());
        assert_eq!(menu.view().screen, super::super::MenuScreen::Profile);
    }

    #[test]
    fn selected_server_preparation_follows_only_explicit_shown_details() {
        use super::super::{MenuAction, MenuScreen};
        let mut menu = MenuRuntime::new(true, 2, "Steve".to_owned());
        menu.servers = vec![launcher::menu::view::SavedServer {
            name: "Saved".into(),
            address: "saved.test".into(),
            favorite: false,
            last_joined_unix: 0,
        }];
        menu.featured = vec![MenuServerCard {
            name: "Featured".into(),
            address: "featured.test".into(),
            caption: String::new(),
            image_path: String::new(),
            icon: None,
        }];
        let mut control = Fake {
            events: Vec::new(),
            signed_out: false,
            prepared: Vec::new(),
        };
        menu.enter(MenuScreen::Servers);
        menu.sync_account_control(&mut control);
        assert_eq!(
            control.prepared.last().unwrap(),
            &None,
            "showing the catalog does not select a target"
        );
        menu.activate(MenuAction::SelectSaved(0));
        menu.sync_account_control(&mut control);
        assert_eq!(
            control.prepared.last().unwrap().as_deref(),
            Some("saved.test")
        );
        menu.activate(MenuAction::SelectSaved(9));
        menu.sync_account_control(&mut control);
        assert_eq!(control.prepared.last().unwrap(), &None);
        menu.activate(MenuAction::SelectFeatured(0));
        menu.sync_account_control(&mut control);
        assert_eq!(
            control.prepared.last().unwrap().as_deref(),
            Some("featured.test")
        );
        assert!(
            menu.take_join_intent().is_none(),
            "selection prepares transport without joining"
        );
        menu.observe_session(crate::session::SessionStatus {
            connecting: true,
            owns_directory: false,
        });
        let selections = control.prepared.len();
        menu.sync_account_control(&mut control);
        assert_eq!(
            control.prepared.len(),
            selections,
            "the join retains its warm target to claim"
        );
        menu.observe_session(crate::session::SessionStatus::default());
        menu.enter(MenuScreen::Home);
        menu.sync_account_control(&mut control);
        assert_eq!(control.prepared.last().unwrap(), &None);
        menu.enter(MenuScreen::Servers);
        menu.activate(MenuAction::SignOut);
        menu.sync_account_control(&mut control);
        assert_eq!(control.prepared.last().unwrap(), &None);
        assert!(control.signed_out);
    }
}
