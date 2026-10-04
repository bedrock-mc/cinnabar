//! Menu flows at the session boundary: death and respawn, local worlds, disconnect wording.
use super::{MenuAction, MenuRuntime, MenuScreen};

#[test]
fn death_screen_opens_once_per_death_and_requests_respawn() {
    let mut menu = MenuRuntime::new(false, 2, "Steve".to_owned());
    menu.open_death();
    assert!(menu.is_visible());
    assert_eq!(menu.view().screen, MenuScreen::Death);
    menu.activate(MenuAction::Respawn);
    assert!(!menu.is_visible());
    assert!(menu.take_respawn_request());
    assert!(!menu.take_respawn_request(), "the request is consumed once");
    menu.open_death();
    assert!(
        !menu.is_visible(),
        "the same death does not reopen the screen"
    );
    menu.note_player_alive();
    menu.open_death();
    assert!(menu.is_visible(), "a later death shows it again");
}

#[test]
fn local_world_choices_reach_the_worlds_module() {
    let mut menu = MenuRuntime::new(true, 2, "Steve".to_owned());
    menu.activate(MenuAction::PlayLocalWorld(0));
    assert_eq!(
        menu.take_local_world_request(),
        None,
        "no world at that index"
    );
    menu.set_local_worlds(vec![super::LocalWorldCard::default()]);
    menu.activate(MenuAction::PlayLocalWorld(0));
    assert_eq!(menu.take_local_world_request(), Some(0));
}

// The port box edits its own value and joins the host as `host:port`; a typed port wins.
#[test]
fn server_draft_joins_the_separate_port_box() {
    let mut menu = MenuRuntime::new(true, 2, "Steve".to_owned());
    menu.activate(MenuAction::PlayAddServer);
    assert_eq!(menu.view().port, "19132");
    assert_eq!(menu.draft_endpoint(), "", "an empty host stays empty");
    menu.activate(MenuAction::AddPort);
    assert_eq!(menu.view().field, Some(super::MenuField::Port));
    menu.address.set_text("play.example");
    menu.port.set_text("19133");
    assert_eq!(menu.draft_endpoint(), "play.example:19133");
    menu.address.set_text("play.example:25565");
    assert_eq!(menu.draft_endpoint(), "play.example:25565");
    menu.address.set_text("::1");
    assert_eq!(menu.draft_endpoint(), "[::1]:19133");
    menu.address.set_text("play.example");
    menu.port.clear();
    assert_eq!(menu.draft_endpoint(), "play.example");
}

// A new device code opens the pre-filled sign-in page once; repeats of that code do not.
#[test]
fn sign_in_page_opens_once_per_device_code() {
    let mut menu = MenuRuntime::new(true, 2, "Steve".to_owned());
    let awaiting = |code: &str| super::AuthState::AwaitingCode {
        uri: "https://www.microsoft.com/link".to_owned(),
        code: code.to_owned(),
    };
    menu.control_auth = Some(awaiting("JS6SVMLR"));
    menu.open_sign_in_page();
    assert_eq!(menu.sign_in_page_code.as_deref(), Some("JS6SVMLR"));
    menu.control_auth = Some(awaiting("NEWCODE1"));
    menu.open_sign_in_page();
    assert_eq!(menu.sign_in_page_code.as_deref(), Some("NEWCODE1"));
}

const KICK: &str =
    "server disconnected: We've detected movement cheats (network read failed: closed)";

#[test]
fn launcher_shows_the_server_reason_on_the_disconnect_screen() {
    use super::disconnect::{DisconnectBody, describe};
    let mut menu = MenuRuntime::new(true, 2, "Player".to_owned());
    assert!(menu.absorb_session_failure(KICK));
    let error = menu.view().disconnect_message.unwrap();
    assert_eq!(
        describe(&error).body,
        DisconnectBody::Server("We've detected movement cheats".to_owned())
    );
}

#[test]
fn launcher_words_a_transport_failure_as_vanilla_does() {
    use super::disconnect::{DisconnectBody, describe};
    let mut menu = MenuRuntime::new(true, 2, "Player".to_owned());
    assert!(menu.absorb_session_failure("network session failed: closed"));
    let error = menu.view().disconnect_message.unwrap();
    assert_eq!(
        describe(&error).body,
        DisconnectBody::Key("disconnect.closed")
    );
}
