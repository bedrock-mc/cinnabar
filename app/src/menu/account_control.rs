//! The menu's view of the core's account control surface, so the play and
//! sign-in screens never see the transport. Without a launcher core the
//! account catalog and the auth supervisor keep feeding the menu.

use super::{AuthState, MenuFriendCard, MenuRealmCard, MenuRuntime, MenuServerCard};
use launcher::menu::view::{
    JoinStage, MenuHome, MenuProfile, PingInfo, ServerDetails, ServerTrustPrompt,
};

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
    /// Requests live experience counts only while selected experience details are visible.
    fn set_player_counts_visible(&mut self, _visible: bool) {}
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
    /// The join's pending question whether to trust a NetherNet server.
    fn server_trust(&mut self) -> Option<ServerTrustPrompt> {
        None
    }
    /// Answers trust prompt `id`.
    fn answer_server_trust(&mut self, _id: u64, _trusted: bool) {}
}

impl MenuRuntime {
    /// Answers the shown trust prompt; "Don't Trust" also cancels the join.
    pub(crate) fn answer_server_trust(&mut self, trusted: bool) {
        let Some(prompt) = self.feeds.server_trust.take() else {
            return;
        };
        self.feeds.server_trust_answer = Some((prompt, trusted));
        if !trusted {
            self.intents.disconnect = true;
        }
    }

    /// Shows a per-session core's trust question while joining and sends it the answer.
    pub(crate) fn sync_session_trust(&mut self, source: &dyn super::server_trust::TrustSource) {
        if let Some((prompt, trusted)) = self
            .feeds
            .server_trust_answer
            .take_if(|(prompt, _)| prompt.from_session_core)
        {
            source.answer(prompt.id, trusted);
        }
        match source.prompt().filter(|_| self.is_connecting()) {
            Some(prompt) => self.feeds.server_trust = Some(prompt),
            None if self.server_trust_from_session_core() => self.feeds.server_trust = None,
            None => {}
        }
    }

    /// Drops a per-session core's question and any answer to it once that core is gone, so neither
    /// reaches the next join's core, whose prompt ids start over.
    pub(crate) fn forget_session_trust(&mut self) {
        self.feeds
            .server_trust_answer
            .take_if(|(prompt, _)| prompt.from_session_core);
        if self.server_trust_from_session_core() {
            self.feeds.server_trust = None;
        }
    }

    /// Drops the launcher core's question and any answer to it when that core is retired, so a
    /// restarted core, whose prompt ids start over, never receives them.
    pub(crate) fn forget_launcher_trust(&mut self) {
        self.feeds
            .server_trust_answer
            .take_if(|(prompt, _)| !prompt.from_session_core);
        if self
            .feeds
            .server_trust
            .as_ref()
            .is_some_and(|prompt| !prompt.from_session_core)
        {
            self.feeds.server_trust = None;
        }
    }

    fn server_trust_from_session_core(&self) -> bool {
        self.feeds
            .server_trust
            .as_ref()
            .is_some_and(|prompt| prompt.from_session_core)
    }

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
            let selected = self
                .feeds
                .selected_featured
                .and_then(|index| self.featured.get(index))
                .map(|card| card.address.clone());
            self.feeds.details.extend(
                featured
                    .iter()
                    .map(|(card, details)| (card.address.clone(), details.clone())),
            );
            self.featured = featured.into_iter().map(|(card, _)| card).collect();
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
        if self.is_connecting() {
            self.feeds.join.observe(control.join_stage());
        }
        if let Some((prompt, trusted)) = self
            .feeds
            .server_trust_answer
            .take_if(|(prompt, _)| !prompt.from_session_core)
        {
            control.answer_server_trust(prompt.id, trusted);
        }
        let asked = control.server_trust().filter(|_| self.is_connecting());
        if asked.is_some() || !self.server_trust_from_session_core() {
            self.feeds.server_trust = asked;
        }
        if let Some(status) = control.account_status() {
            self.apply_control_auth(status);
        }
        while let Some(event) = control.poll_event() {
            match event {
                AccountEvent::Auth(state) => self.apply_control_auth(state),
                AccountEvent::Disconnected { reason } => {
                    self.disconnect_message = Some(super::disconnect::from_server(&reason));
                }
            }
        }
        if std::mem::take(&mut self.sign_out_requested) {
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
        for details in self.feeds.details.values_mut() {
            details.player_count = None;
        }
        self.feeds.profile = MenuProfile::default();
        self.feeds.home = MenuHome::default();
        self.catalog_message = None;
        self.enter(super::MenuScreen::Profile);
    }

    /// An experience details panel owns the live count subscription for its visible lifetime.
    fn experience_counts_visible(&self) -> bool {
        self.visible
            && !self.is_connecting()
            && self.screen == super::MenuScreen::Servers
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
    use super::*;
    use crate::menu::MenuAction;

    struct Catalog(Option<Vec<(MenuServerCard, ServerDetails)>>);
    impl AccountControl for Catalog {
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
        fn featured(&mut self) -> Option<Vec<(MenuServerCard, ServerDetails)>> {
            self.0.take()
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
        menu.featured = vec![card("first.test:19132"), card("picked.test:19132")];
        menu.feeds.selected_featured = Some(1);
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
        assert_eq!(
            menu.featured[menu.feeds.selected_featured.unwrap()].address,
            "picked.test:19132"
        );
        control.0 = Some(vec![(card("first.test:19132"), ServerDetails::default())]);
        menu.sync_account_control(&mut control);
        assert_eq!(menu.feeds.selected_featured, None);
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
        menu.enter(super::super::MenuScreen::Servers);
        assert!(menu.experience_counts_visible());
        menu.feeds.selected_saved = Some(0);
        assert!(!menu.experience_counts_visible());
        menu.feeds.selected_saved = None;
        menu.visible = false;
        assert!(!menu.experience_counts_visible());
        menu.visible = true;
        menu.observe_session(crate::session::SessionStatus {
            connecting: true,
            owns_directory: false,
        });
        assert!(!menu.experience_counts_visible());
    }

    struct Fake {
        events: Vec<AccountEvent>,
        signed_out: bool,
    }

    impl AccountControl for Fake {
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

    struct Trusting {
        prompt: Option<ServerTrustPrompt>,
        answers: Vec<(u64, bool)>,
    }

    impl AccountControl for Trusting {
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
        fn server_trust(&mut self) -> Option<ServerTrustPrompt> {
            self.prompt.clone()
        }
        fn answer_server_trust(&mut self, id: u64, trusted: bool) {
            self.answers.push((id, trusted));
        }
    }

    // The core's trust question shows while connecting; "Trust and Join" answers it and keeps the
    // join, while "Don't Trust" and Back answer no and cancel the join.
    #[test]
    fn server_trust_prompt_shows_while_joining_and_forwards_the_answer() {
        let prompt = ServerTrustPrompt {
            id: 4,
            url: "http://127.0.0.1:19132".into(),
            from_session_core: false,
        };
        for (answer, trusted, cancels) in [
            (MenuAction::ServerTrust(true), true, false),
            (MenuAction::ServerTrust(false), false, true),
            (MenuAction::AddBack, false, true),
        ] {
            let mut menu = MenuRuntime::new(true, 2, "Steve".to_owned());
            let mut control = Trusting {
                prompt: Some(prompt.clone()),
                answers: Vec::new(),
            };
            menu.sync_account_control(&mut control);
            assert!(
                menu.view().feeds.server_trust.is_none(),
                "shown outside a join"
            );
            menu.observe_session(crate::session::SessionStatus {
                connecting: true,
                owns_directory: false,
            });
            menu.sync_account_control(&mut control);
            assert_eq!(menu.view().feeds.server_trust.as_ref(), Some(&prompt));
            assert_eq!(
                menu.focus_actions(),
                vec![
                    MenuAction::ServerTrust(true),
                    MenuAction::ServerTrust(false)
                ]
            );
            if answer == MenuAction::AddBack {
                menu.go_back();
            } else {
                menu.activate(answer);
            }
            control.prompt = None;
            menu.sync_account_control(&mut control);
            assert_eq!(control.answers, vec![(4, trusted)]);
            assert_eq!(menu.take_disconnect_request(), cancels);
            assert!(menu.view().feeds.server_trust.is_none());
        }
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

    // A per-session core's question shows like the launcher core's, and its answer goes back to the
    // core that asked even though the launcher core reports nothing.
    #[test]
    fn session_core_trust_answers_return_to_the_session_core() {
        let mut menu = MenuRuntime::new(true, 2, "Steve".to_owned());
        menu.observe_session(crate::session::SessionStatus {
            connecting: true,
            owns_directory: true,
        });
        let asked = ServerTrustPrompt {
            id: 1,
            url: "http://127.0.0.1:19132".into(),
            from_session_core: true,
        };
        let session = SessionSource {
            prompt: std::cell::RefCell::new(Some(asked.clone())),
            answers: Default::default(),
        };
        let mut launcher = Trusting {
            prompt: None,
            answers: Vec::new(),
        };
        let mut frame = |menu: &mut MenuRuntime| {
            menu.sync_account_control(&mut launcher);
            menu.sync_session_trust(&session);
        };
        frame(&mut menu);
        assert_eq!(menu.view().feeds.server_trust.as_ref(), Some(&asked));
        frame(&mut menu);
        assert_eq!(menu.view().feeds.server_trust.as_ref(), Some(&asked));
        menu.activate(MenuAction::ServerTrust(true));
        frame(&mut menu);
        assert!(menu.view().feeds.server_trust.is_none());
        assert_eq!(*session.answers.borrow(), vec![(1, true)]);
        assert!(
            launcher.answers.is_empty(),
            "the launcher core got the session core's answer"
        );

        // An answer left when its core is gone is dropped rather than sent to the next core.
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
            "an old answer reached the next core"
        );

        // Likewise a launcher answer left when its core is retired never reaches the restarted one.
        let mut launcher = Trusting {
            prompt: Some(ServerTrustPrompt {
                id: 1,
                url: "http://127.0.0.1:19132".into(),
                from_session_core: false,
            }),
            answers: Vec::new(),
        };
        menu.sync_account_control(&mut launcher);
        menu.activate(MenuAction::ServerTrust(true));
        menu.forget_launcher_trust();
        let mut restarted = Trusting {
            prompt: None,
            answers: Vec::new(),
        };
        menu.sync_account_control(&mut restarted);
        assert!(
            restarted.answers.is_empty(),
            "a retired core's answer reached its replacement"
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
}
