use super::*;

#[test]
fn configuration_uses_default_override_and_explicit_disable() {
    let default = application_id(None)
        .unwrap()
        .expect("presence is enabled by default");
    assert_eq!(default, DEFAULT_APPLICATION_ID);
    assert_ne!(default, 0);
    assert_eq!(
        application_id(Some("123456789012345678")),
        Ok(Some(123456789012345678))
    );
    assert_eq!(application_id(Some("0")), Ok(None));
    for invalid in [
        "",
        "00",
        "-1",
        "+1",
        " 42",
        "42 ",
        "abc",
        "18446744073709551616",
    ] {
        assert!(application_id(Some(invalid)).is_err(), "{invalid}");
    }
}

fn server(endpoint: &str) -> Destination {
    Destination::Server(endpoint.to_owned())
}

#[test]
fn unchanged_frames_do_not_publish_and_reconnect_republishes_latest_state() {
    let mut publication = Publication::default();
    assert!(publication.changed(State::Menus, 0, None, 0));
    assert!(!publication.changed(State::Menus, 0, None, 0));
    assert!(publication.changed(State::Joining, 0, None, 0));
    assert!(publication.changed(State::Playing, 0, None, 0));
    assert!(!publication.changed(State::Playing, 0, None, 0));
    assert!(publication.changed(State::Playing, 1, None, 0));
    assert!(!publication.changed(State::Playing, 1, None, 0));
    assert!(publication.changed(State::Menus, 1, None, 0));
}

#[test]
fn activity_states_keep_start_time_and_exclude_account_and_join_data() {
    let mut states = Vec::new();
    for state in [State::Menus, State::Joining, State::Playing] {
        let payload = serde_json::to_value(state.activity(1234, None)).unwrap();
        assert_eq!(payload["details"], launcher::PRODUCT_NAME);
        assert_eq!(payload["timestamps"]["start"], 1234);
        assert_eq!(payload["assets"]["large_image"], LARGE_IMAGE_URL);
        assert_eq!(payload["assets"]["large_text"], launcher::PRODUCT_NAME);
        assert!(payload.get("secrets").is_none());
        assert!(payload.get("party").is_none());
        states.push(payload["state"].as_str().unwrap().to_owned());
    }
    states.sort();
    states.dedup();
    assert_eq!(states.len(), 3);
}

#[test]
fn playing_text_names_servers_and_worlds_but_not_realm_or_friend_identities() {
    let text = |destination: Destination| {
        serde_json::to_value(State::Playing.activity(1234, Some(&destination))).unwrap()["state"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    assert_eq!(
        text(server("play.example.net:19133")),
        "Playing on play.example.net:19133"
    );
    assert_eq!(
        text(Destination::LocalWorld("My World".into())),
        "Singleplayer: My World"
    );
    assert_eq!(text(Destination::Realm), "Playing on a Realm");
    assert_eq!(
        text(Destination::FriendWorld),
        "Playing in a friend's world"
    );
    assert_eq!(text(Destination::Experience), "Playing an experience");
}

#[test]
fn server_transfers_publish_once_and_menu_discards_the_destination() {
    let (first, second) = (
        server("first.example.net:19133"),
        server("second.example.net:19134"),
    );
    let mut publication = Publication::default();
    assert!(publication.changed(State::Playing, 1, Some(&first), 0));
    assert!(!publication.changed(State::Playing, 1, Some(&first), 0));
    assert!(publication.changed(State::Playing, 1, Some(&second), 0));
    assert!(!publication.changed(State::Playing, 1, Some(&second), 0));
    assert!(publication.changed(State::Playing, 2, Some(&second), 0));
    assert!(publication.changed(State::Menus, 2, None, 0));
    assert!(publication.last.unwrap().2.is_none());
}

#[test]
fn in_game_timer_counts_the_session_not_reconnects() {
    let world = server("play.example.net:19132");
    let mut publication = Publication::default();
    publication.changed(State::Menus, 0, None, 10);
    assert_eq!(publication.playing_since, None);
    publication.changed(State::Playing, 0, Some(&world), 20);
    assert_eq!(publication.playing_since, Some(20));
    // A Discord reconnect republishes without restarting the timer.
    publication.changed(State::Playing, 1, Some(&world), 30);
    assert_eq!(publication.playing_since, Some(20));
    // A transfer is a new session.
    publication.changed(State::Playing, 1, Some(&server("other:19132")), 40);
    assert_eq!(publication.playing_since, Some(40));
    publication.changed(State::Menus, 1, None, 50);
    assert_eq!(publication.playing_since, None);
}

#[test]
fn long_text_remains_valid_utf8_within_discord_limit() {
    let endpoint = server(&"界".repeat(MAX_STATE_BYTES));
    let payload = serde_json::to_value(State::Playing.activity(1234, Some(&endpoint))).unwrap();
    assert!(payload["state"].as_str().unwrap().len() <= MAX_STATE_BYTES);
}
