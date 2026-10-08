use std::sync::Arc;

use json_ui::{HitKind, HitRegion, RectOut};
use launcher::menu::invite::{Action, Friend, InviteState, Section};

use super::super::menu_screens::{action_for, screen_data};
use crate::menu::{MenuAction, MenuScreen, MenuView};

fn friend(xuid: &str, gamertag: &str, online: bool) -> Friend {
    Friend {
        xuid: xuid.to_owned(),
        gamertag: gamertag.to_owned(),
        online,
        picture_path: String::new(),
    }
}

/// Two online friends and one offline, with the second online friend picked.
fn invite_view() -> MenuView {
    let mut state = InviteState::default();
    state.set_friends(Some(vec![
        friend("1", "OnlineAda", true),
        friend("2", "OfflineBo", false),
        friend("3", "OnlineCy", true),
    ]));
    state.toggle(Section::Online, 1);
    let mut view = MenuView::new(true, "Host".to_owned());
    view.screen = MenuScreen::Invite;
    view.over_world = true;
    view.hosting = true;
    view.invite = Some(Arc::new(state));
    view
}

fn region(kind: HitKind, pressed: Option<&str>, collection: Option<(&str, usize)>) -> HitRegion {
    let rect = RectOut {
        x: 0.0,
        y: 0.0,
        w: 10.0,
        h: 10.0,
    };
    HitRegion {
        key: "/screen/control".to_owned(),
        name: "control".to_owned(),
        kind,
        rect,
        clip: rect,
        layer: 0,
        order: 0,
        pressed: pressed.map(str::to_owned),
        control_name: None,
        collection_index: collection.map(|(_, index)| index),
        collection: collection.map(|(name, _)| name.to_owned()),
        enabled: true,
        checked: None,
        max_length: None,
        group_index: None,
        renderer: None,
        drag_axes: [false; 2],
        sound: None,
        input: Default::default(),
        focus: None,
        widget: Default::default(),
        collections: Vec::new(),
        modal_root: None,
    }
}

#[test]
fn checkboxes_route_to_their_own_list_and_row() {
    let view = invite_view();
    let toggle = |collection, index| {
        action_for(
            &view,
            &region(HitKind::Toggle, None, Some((collection, index))),
        )
    };
    assert_eq!(
        toggle("online_xbox_live_friends", 1),
        Some(MenuAction::Invite(Action::Toggle(Section::Online, 1)))
    );
    assert_eq!(
        toggle("offline_xbox_live_friends", 0),
        Some(MenuAction::Invite(Action::Toggle(Section::Offline, 0)))
    );
    assert_eq!(toggle("online_platform_friends", 0), None);
    let press = |id| action_for(&view, &region(HitKind::Button, Some(id), None));
    assert_eq!(
        press("button.send_invites"),
        Some(MenuAction::Invite(Action::Send))
    );
    assert_eq!(press("button.menu_exit"), Some(MenuAction::AddBack));
}

#[test]
fn the_pause_invite_button_answers_only_while_hosting() {
    let mut pause = MenuView::new(true, "Host".to_owned());
    pause.screen = MenuScreen::Pause;
    let invite = region(HitKind::Button, Some("button.menu_invite_players"), None);
    assert_eq!(action_for(&pause, &invite), None);
    pause.hosting = true;
    assert_eq!(
        action_for(&pause, &invite),
        Some(MenuAction::Invite(Action::Open))
    );
}

#[test]
fn the_send_button_counts_the_picks() {
    let none = |_: &str| -> Option<Arc<str>> { None };
    assert_eq!(super::send_label(0, &none), super::SEND);
    assert_eq!(super::send_label(1, &none), super::SEND_ONE);
    let english = |key: &str| -> Option<Arc<str>> {
        (key == super::SEND_COUNT).then(|| "Send %d Invites".into())
    };
    assert_eq!(super::send_label(3, &english), "Send 3 Invites");
}

/// Lays `view`'s vanilla screen out from the installed UI carrier.
fn render(view: &MenuView) -> Option<json_ui::ScreenRender> {
    let carrier = super::super::pack_harness::carrier()?;
    let catalog = json_ui::Catalog::from_files(
        carrier
            .ui_files()
            .iter()
            .map(|file| (&*file.path, &*file.bytes)),
    )
    .ok()?;
    let screen = screen_data(view, &|_| None)?;
    let env = json_ui::LayoutEnv {
        text: &super::super::tests::FixedText,
        textures: &super::super::tests::NoTextures,
    };
    json_ui::render_screen(
        screen.reference,
        &catalog,
        &screen.context,
        &screen.data,
        [480.0, 270.0],
        &env,
        &json_ui::ViewState::default(),
    )
}

fn texts(render: &json_ui::ScreenRender) -> Vec<&str> {
    render
        .nodes
        .iter()
        .filter_map(|node| match &node.draw {
            json_ui::Draw::Text { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect()
}

// The populated lists lay out one checkbox row per friend under vanilla's headers, each
// routing to its row and showing the player's pick.
#[test]
fn the_invite_screen_lists_each_friend_with_a_checkbox() {
    let view = invite_view();
    let Some(render) = render(&view) else {
        eprintln!(
            "skipping the_invite_screen_lists_each_friend_with_a_checkbox: fixture unavailable; requires installed local carriers (make assets)"
        );
        return;
    };
    let texts = texts(&render);
    for wanted in [
        "OnlineAda",
        "OnlineCy",
        "OfflineBo",
        "invite.OnlineFriends",
        "invite.OfflineFriends",
        super::SEND_ONE,
    ] {
        assert!(texts.contains(&wanted), "{wanted}: {texts:?}");
    }
    assert!(!texts.contains(&super::NO_FRIENDS), "{texts:?}");
    let rows: Vec<_> = render
        .hits
        .iter()
        .filter(|region| region.kind == HitKind::Toggle)
        .filter_map(|region| Some((action_for(&view, region)?, region.checked)))
        .collect();
    assert_eq!(
        rows,
        [
            (
                MenuAction::Invite(Action::Toggle(Section::Online, 0)),
                Some(false)
            ),
            (
                MenuAction::Invite(Action::Toggle(Section::Online, 1)),
                Some(true)
            ),
            (
                MenuAction::Invite(Action::Toggle(Section::Offline, 0)),
                Some(false)
            ),
        ]
    );
    assert!(
        render.hits.iter().any(|region| region.enabled
            && action_for(&view, region) == Some(MenuAction::Invite(Action::Send))),
        "the send button is pressable"
    );
}

// An account without friends shows vanilla's empty-list line instead of the lists.
#[test]
fn an_empty_friends_list_says_so() {
    let mut view = invite_view();
    let mut state = InviteState::default();
    state.set_friends(Some(Vec::new()));
    view.invite = Some(Arc::new(state));
    let Some(render) = render(&view) else {
        eprintln!(
            "skipping an_empty_friends_list_says_so: fixture unavailable; requires installed local carriers (make assets)"
        );
        return;
    };
    let texts = texts(&render);
    assert!(texts.contains(&super::NO_FRIENDS), "{texts:?}");
    assert!(!texts.contains(&"invite.OnlineFriends"), "{texts:?}");
}
