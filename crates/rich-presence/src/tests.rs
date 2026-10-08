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

fn server(endpoint: &str) -> Target {
    Target {
        destination: Destination::Server(endpoint.to_owned()),
        join: Some(endpoint.to_owned()),
        badge: None,
        max_players: None,
    }
}

fn place(destination: Destination, join: Option<&str>) -> Target {
    Target {
        destination,
        join: join.map(str::to_owned),
        badge: None,
        max_players: None,
    }
}

#[test]
fn featured_art_badges_the_card_only_while_playing_and_only_over_https() {
    let badged = |url: &str| Target {
        badge: Some(Badge {
            image_url: url.to_owned(),
            name: "The Hive".to_owned(),
        }),
        ..server("geo.hivebedrock.network:19132")
    };
    let hive = badged("https://cdn.example/hive.png");
    let payload = serde_json::to_value(State::Playing.activity(1, Some(&hive), None)).unwrap();
    assert_eq!(
        payload["assets"]["small_image"],
        "https://cdn.example/hive.png"
    );
    assert_eq!(payload["assets"]["small_text"], "The Hive");
    assert_eq!(payload["assets"]["large_image"], LARGE_IMAGE_URL);
    let joining = serde_json::to_value(State::Joining.activity(1, Some(&hive), None)).unwrap();
    assert!(joining["assets"].get("small_image").is_none());
    let long = format!("https://cdn.example/{}", "a".repeat(MAX_IMAGE_BYTES));
    for rejected in [
        "http://cdn.example/hive.png",
        "file:///hive.png",
        long.as_str(),
    ] {
        let payload =
            serde_json::to_value(State::Playing.activity(1, Some(&badged(rejected)), None))
                .unwrap();
        assert!(payload["assets"].get("small_image").is_none(), "{rejected}");
    }
}

#[test]
fn unchanged_frames_do_not_publish_and_reconnect_republishes_latest_state() {
    let mut publication = Publication::default();
    assert!(publication.changed(State::Menus, 0, None, None, 0));
    assert!(!publication.changed(State::Menus, 0, None, None, 0));
    assert!(
        !publication.changed(State::Joining, 0, None, None, 0),
        "joining keeps the menus card so the in-world card is not rate-limited behind it"
    );
    assert!(publication.changed(State::Playing, 0, None, None, 0));
    assert_eq!(publication.playing_since, Some(0));
    assert!(!publication.changed(State::Playing, 0, None, None, 0));
    assert!(publication.changed(State::Playing, 1, None, None, 0));
    assert!(!publication.changed(State::Playing, 1, None, None, 0));
    assert!(publication.changed(State::Menus, 1, None, None, 0));
}

#[test]
fn party_size_shows_players_of_the_limit_and_republishes_as_players_come_and_go() {
    let world = Target {
        max_players: Some(8),
        ..server("play.example.net:19132")
    };
    let size = |players, state: State| {
        serde_json::to_value(state.activity(1, Some(&world), players)).unwrap()["party"]["size"]
            .clone()
    };
    assert_eq!(size(Some(3), State::Playing), serde_json::json!([3, 8]));
    assert_eq!(
        size(Some(20), State::Playing),
        serde_json::json!([8, 8]),
        "never above max"
    );
    assert_eq!(
        size(Some(0), State::Playing),
        serde_json::json!([1, 8]),
        "the player counts"
    );
    assert!(size(None, State::Playing).is_null());
    assert!(size(Some(3), State::Joining).is_null());
    let unlimited = server("play.example.net:19132");
    let payload = serde_json::to_value(State::Playing.activity(1, Some(&unlimited), Some(3)));
    assert!(payload.unwrap()["party"].get("size").is_none());
    let mut publication = Publication::default();
    assert!(publication.changed(State::Playing, 0, Some(&world), Some(1), 0));
    assert!(!publication.changed(State::Playing, 0, Some(&world), Some(1), 5));
    assert!(publication.changed(State::Playing, 0, Some(&world), Some(2), 5));
    assert_eq!(
        publication.playing_since,
        Some(0),
        "a player joining is the same session"
    );
}

#[test]
fn activity_states_keep_start_time_and_exclude_account_and_join_data() {
    let mut states = Vec::new();
    for state in [State::Menus, State::Joining, State::Playing] {
        let payload = serde_json::to_value(state.activity(1234, None, None)).unwrap();
        // Discord already titles the card with the application's name.
        assert!(payload.get("details").is_none());
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
        let target = place(destination, None);
        serde_json::to_value(State::Playing.activity(1234, Some(&target), None)).unwrap()["state"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    assert_eq!(
        text(Destination::Server("play.example.net:19133".into())),
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
fn invites_carry_the_join_address_only_in_the_secret_and_only_while_playing() {
    let experience = place(Destination::Experience, Some("gathering/secret-id"));
    let payload =
        serde_json::to_value(State::Playing.activity(1, Some(&experience), None)).unwrap();
    let secret = payload["secrets"]["join"].as_str().unwrap();
    assert_eq!(join_address(secret), Some("gathering/secret-id"));
    let party = payload["party"]["id"].as_str().unwrap();
    assert!(!party.contains("secret-id"));
    assert!(!payload["state"].as_str().unwrap().contains("secret-id"));
    for state in [State::Menus, State::Joining] {
        let payload = serde_json::to_value(state.activity(1, Some(&experience), None)).unwrap();
        assert!(payload.get("secrets").is_none() && payload.get("party").is_none());
    }
    let local = place(Destination::LocalWorld("My World".into()), None);
    let payload = serde_json::to_value(State::Playing.activity(1, Some(&local), None)).unwrap();
    assert!(payload.get("secrets").is_none() && payload.get("party").is_none());
}

#[test]
fn players_on_one_destination_share_a_party() {
    let payload =
        |target: &Target| serde_json::to_value(State::Playing.activity(1, Some(target), None));
    let first = payload(&server("play.example.net:19132")).unwrap();
    let second = payload(&server("play.example.net:19132")).unwrap();
    let other = payload(&server("other.example.net:19132")).unwrap();
    assert_eq!(first["party"]["id"], second["party"]["id"]);
    assert_ne!(first["party"]["id"], other["party"]["id"]);
}

#[test]
fn received_secrets_must_be_ones_cinnabar_publishes() {
    for rejected in [
        "play.example.net:19132",
        "cinnabar1:",
        "cinnabar1:bad host",
        "cinnabar1:host\n:19132",
        "cinnabar2:play.example.net:19132",
    ] {
        assert_eq!(join_address(rejected), None, "{rejected:?}");
    }
    let oversized = format!("cinnabar1:{}", "a".repeat(128));
    assert_eq!(join_address(&oversized), None);
    let unpublishable = server(&"a".repeat(128));
    let payload =
        serde_json::to_value(State::Playing.activity(1, Some(&unpublishable), None)).unwrap();
    assert!(payload.get("secrets").is_none() && payload.get("party").is_none());
}

#[test]
fn another_experience_republishes_its_invite() {
    let experience = |id: &str| place(Destination::Experience, Some(id));
    let mut publication = Publication::default();
    assert!(publication.changed(State::Playing, 0, Some(&experience("gathering/1")), None, 0));
    assert!(publication.changed(State::Playing, 0, Some(&experience("gathering/2")), None, 0));
}

#[test]
fn server_transfers_publish_once_and_menu_discards_the_destination() {
    let (first, second) = (
        server("first.example.net:19133"),
        server("second.example.net:19134"),
    );
    let mut publication = Publication::default();
    assert!(publication.changed(State::Playing, 1, Some(&first), None, 0));
    assert!(!publication.changed(State::Playing, 1, Some(&first), None, 0));
    assert!(publication.changed(State::Playing, 1, Some(&second), None, 0));
    assert!(!publication.changed(State::Playing, 1, Some(&second), None, 0));
    assert!(publication.changed(State::Playing, 2, Some(&second), None, 0));
    assert!(publication.changed(State::Menus, 2, None, None, 0));
    assert!(publication.last.unwrap().2.is_none());
}

#[test]
fn in_game_timer_counts_the_session_not_reconnects() {
    let world = server("play.example.net:19132");
    let mut publication = Publication::default();
    publication.changed(State::Menus, 0, None, None, 10);
    assert_eq!(publication.playing_since, None);
    publication.changed(State::Playing, 0, Some(&world), None, 20);
    assert_eq!(publication.playing_since, Some(20));
    // A Discord reconnect republishes without restarting the timer.
    publication.changed(State::Playing, 1, Some(&world), None, 30);
    assert_eq!(publication.playing_since, Some(20));
    // A transfer is a new session.
    publication.changed(State::Playing, 1, Some(&server("other:19132")), None, 40);
    assert_eq!(publication.playing_since, Some(40));
    publication.changed(State::Menus, 1, None, None, 50);
    assert_eq!(publication.playing_since, None);
}

#[test]
fn long_text_remains_valid_utf8_within_discord_limit() {
    let endpoint = server(&"界".repeat(MAX_STATE_BYTES));
    let payload =
        serde_json::to_value(State::Playing.activity(1234, Some(&endpoint), None)).unwrap();
    assert!(payload["state"].as_str().unwrap().len() <= MAX_STATE_BYTES);
}
