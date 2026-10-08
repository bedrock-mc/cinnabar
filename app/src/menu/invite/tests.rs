use launcher::menu::invite::{Friend, Section};

use super::*;
use crate::menu::{AuthState, MenuFriendCard, MenuRealmCard, account_control::AccountEvent};

/// A core that records what the menu asks of it and answers the friends list when told.
#[derive(Default)]
struct Core {
    requests: usize,
    answer: Option<Result<Vec<Friend>, ()>>,
    sent: Vec<Vec<String>>,
}

impl AccountControl for Core {
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
    fn request_people(&mut self) {
        self.requests += 1;
    }
    fn people(&mut self) -> Option<Result<Vec<Friend>, ()>> {
        self.answer.take()
    }
    fn send_invites(&mut self, xuids: Vec<String>) {
        self.sent.push(xuids);
    }
}

fn friend(xuid: &str, online: bool) -> Friend {
    Friend {
        xuid: xuid.to_owned(),
        gamertag: format!("gamer{xuid}"),
        online,
        picture_path: String::new(),
    }
}

/// The pause screen over a live local world; `hosted` picks the dedicated-server backend.
fn paused(hosted: bool) -> MenuRuntime {
    let mut menu = MenuRuntime::new(true, 2, "Host".to_owned());
    menu.feeds.profile.xuid = "2535400000000009".to_owned();
    menu.request_local_world_join("Home".to_owned(), hosted);
    menu.intents.join = None;
    menu.show_world();
    menu.open_pause();
    menu
}

/// The invite screen open over a hosted world, listing friends A and C online and B offline.
fn listing(core: &mut Core) -> MenuRuntime {
    let mut menu = paused(true);
    menu.activate(MenuAction::Invite(Action::Open));
    menu.sync_account_control(core);
    core.answer = Some(Ok(vec![
        friend("A", true),
        friend("B", false),
        friend("C", true),
    ]));
    menu.sync_account_control(core);
    menu
}

#[test]
fn only_a_hosted_world_offers_invites() {
    let mut menu = paused(false);
    let open = MenuAction::Invite(Action::Open);
    assert!(!menu.view().hosting);
    assert!(!menu.focus_actions().contains(&open));
    menu.activate(open);
    assert_eq!(menu.screen(), MenuScreen::Pause);

    let mut menu = paused(true);
    assert!(menu.view().hosting);
    assert!(menu.focus_actions().contains(&open));
    menu.activate(open);
    assert_eq!(menu.screen(), MenuScreen::Invite);
    assert!(menu.view().invite.is_some_and(|invite| invite.loading()));
}

#[test]
fn opening_asks_for_the_friends_once_and_shows_the_answer() {
    let mut core = Core::default();
    let menu = listing(&mut core);
    assert_eq!(core.requests, 1);
    let invite = menu.view().invite.expect("invite screen state");
    assert!(!invite.loading());
    assert_eq!(invite.section(Section::Online).count(), 2);
    assert_eq!(invite.section(Section::Offline).count(), 1);
}

#[test]
fn send_invites_exactly_the_picked_friends_and_returns_to_pause() {
    let mut core = Core::default();
    let mut menu = listing(&mut core);
    for toggle in [
        Action::Toggle(Section::Online, 1),
        Action::Toggle(Section::Offline, 0),
        Action::Toggle(Section::Online, 0),
        Action::Toggle(Section::Online, 0),
    ] {
        menu.activate(MenuAction::Invite(toggle));
    }
    menu.activate(MenuAction::Invite(Action::Send));
    assert_eq!(menu.screen(), MenuScreen::Pause);
    assert!(menu.view().invite.is_none());
    menu.sync_account_control(&mut core);
    menu.sync_account_control(&mut core);
    assert_eq!(core.sent, [["C", "B"]]);
}

#[test]
fn leaving_without_sending_invites_no_one() {
    let mut core = Core::default();
    let mut menu = listing(&mut core);
    menu.activate(MenuAction::Invite(Action::Toggle(Section::Online, 0)));
    menu.activate(MenuAction::AddBack);
    assert_eq!(menu.screen(), MenuScreen::Pause);
    menu.activate(MenuAction::Invite(Action::Open));
    menu.activate(MenuAction::Invite(Action::Send));
    menu.sync_account_control(&mut core);
    assert!(core.sent.is_empty(), "{:?}", core.sent);
    assert_eq!(core.requests, 2, "each opening asks again");
}

#[test]
fn a_failed_friends_list_stops_loading() {
    let mut core = Core::default();
    let mut menu = paused(true);
    menu.activate(MenuAction::Invite(Action::Open));
    core.answer = Some(Err(()));
    menu.sync_account_control(&mut core);
    let invite = menu.view().invite.expect("invite screen state");
    assert!(invite.failed() && !invite.loading());
}
