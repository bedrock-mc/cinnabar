//! Menu flows at the session boundary: death and respawn, local worlds, disconnect wording.
use super::{MenuAction, MenuRuntime, MenuScreen};

#[test]
fn settings_background_follows_the_world_or_launcher_below() {
    let mut menu = MenuRuntime::new(true, 2, "Tester".into());
    menu.activate(MenuAction::Navigate(MenuScreen::Settings));
    assert!(
        menu.uses_panorama(),
        "launcher Settings keeps the title background"
    );

    menu.show_world();
    menu.open_pause();
    menu.activate(MenuAction::Navigate(MenuScreen::Settings));
    assert!(menu.over_world());
    assert!(
        !menu.uses_panorama(),
        "Settings opened from pause retains the live world background"
    );
    let player = crate::player_runtime::PlayerRuntime::new(1);
    let ui = client_ui::ui_runtime::UiRuntime::new(1);
    let presentation = crate::ui_runtime::presentation::forms::tests::mini_engine_presentation();
    assert!(crate::screen_policy::renders_game(
        &player,
        Some(&ui),
        Some(&menu),
        Some(&presentation)
    ));
    assert!(crate::screen_policy::absorbs_input(
        &player,
        Some(&ui),
        Some(&menu),
        Some(&presentation)
    ));
    assert!(crate::screen_policy::renders_game(
        &player,
        None,
        Some(&menu),
        None
    ));
    menu.go_back();
    assert_eq!(menu.screen(), MenuScreen::Pause);
    assert!(!menu.uses_panorama());

    menu.show_world();
    menu.open_death();
    menu.activate(MenuAction::Navigate(MenuScreen::Settings));
    assert!(
        !menu.uses_panorama(),
        "Settings above death retains the world"
    );
    menu.show_home();
    menu.activate(MenuAction::Navigate(MenuScreen::Settings));
    assert!(
        menu.uses_panorama(),
        "returning home restores the title background"
    );
}

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

#[test]
fn add_server_focus_skips_disabled_actions_and_tracks_the_footer_order() {
    let mut menu = MenuRuntime::new(true, 2, "Steve".to_owned());
    menu.activate(MenuAction::PlayAddServer);
    assert_eq!(
        menu.focus_actions(),
        vec![
            MenuAction::AddName,
            MenuAction::AddAddress,
            MenuAction::AddPort,
            MenuAction::AddBack,
        ]
    );
    menu.name.set_text("My server");
    menu.address.set_text("example.test");
    assert_eq!(
        menu.focus_actions(),
        vec![
            MenuAction::AddName,
            MenuAction::AddAddress,
            MenuAction::AddPort,
            MenuAction::AddBack,
            MenuAction::AddSave,
            MenuAction::AddSaveConnect,
        ]
    );
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

// Saving, editing, cancelling or deleting a server keeps the Servers tab, as vanilla pops the form.
#[test]
fn server_form_returns_to_the_servers_tab() {
    let root = crate::ui_runtime::presentation::forms::pack_harness::scratch_dir("server-tab");
    use crate::install_layout::{InstallEnvironment, Platform};
    let layout = super::InstallLayout::resolve(
        Platform::Linux,
        &InstallEnvironment {
            executable: root.join("target/debug/bedrock-client"),
            user_root: None,
            home: Some(root.join("home")),
            local_app_data: None,
            xdg_config_home: None,
            xdg_data_home: None,
            xdg_runtime_dir: None,
        },
    )
    .unwrap();
    let mut menu = MenuRuntime::new_with_layout(
        true,
        Some(2),
        "Steve".to_owned(),
        layout,
        crate::player_skin::LocalPlayerSkin::generated_default("Steve"),
    );
    menu.activate(MenuAction::Navigate(MenuScreen::Servers));
    menu.activate(MenuAction::PlayAddServer);
    menu.name.set_text("Local");
    menu.address.set_text("127.0.0.1");
    menu.activate(MenuAction::AddSave);
    assert_eq!(menu.screen(), MenuScreen::Servers, "add returns to Servers");
    assert_eq!(menu.servers.len(), 1);
    assert_eq!(menu.servers[0].name, "Local");

    menu.activate(MenuAction::EditSaved(0));
    menu.name.set_text("Renamed");
    menu.activate(MenuAction::AddSave);
    assert_eq!(
        menu.screen(),
        MenuScreen::Servers,
        "edit returns to Servers"
    );
    assert_eq!(menu.servers[0].name, "Renamed");

    menu.activate(MenuAction::PlayAddServer);
    menu.activate(MenuAction::AddBack);
    assert_eq!(
        menu.screen(),
        MenuScreen::Servers,
        "cancel returns to Servers"
    );

    menu.activate(MenuAction::RemoveSavedDialog(0));
    menu.activate(MenuAction::ConfirmRemoveSaved(0));
    assert_eq!(
        menu.screen(),
        MenuScreen::Servers,
        "delete stays on Servers"
    );
    assert!(menu.servers.is_empty());
    menu.saves.flush();
    let _ = std::fs::remove_dir_all(root);
}
