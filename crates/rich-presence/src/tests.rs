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

#[test]
fn unchanged_frames_do_not_publish_and_reconnect_republishes_latest_state() {
    let mut publication = Publication::default();
    assert!(publication.changed(State::Menus, 0, None));
    assert!(!publication.changed(State::Menus, 0, None));
    assert!(publication.changed(State::Joining, 0, None));
    assert!(publication.changed(State::Playing, 0, None));
    assert!(!publication.changed(State::Playing, 0, None));
    assert!(publication.changed(State::Playing, 1, None));
    assert!(!publication.changed(State::Playing, 1, None));
    assert!(publication.changed(State::Menus, 1, None));
}

#[test]
fn activity_states_keep_launch_time_and_exclude_account_and_join_data() {
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
fn connected_server_activity_includes_host_and_port() {
    let payload =
        serde_json::to_value(State::Playing.activity(1234, Some("play.example.net:19133")))
            .unwrap();
    assert_eq!(payload["state"], "Playing on play.example.net:19133");
}

#[test]
fn server_transfers_publish_once_and_menu_discards_the_endpoint() {
    let mut publication = Publication::default();
    assert!(publication.changed(State::Playing, 1, Some("first.example.net:19133")));
    assert!(!publication.changed(State::Playing, 1, Some("first.example.net:19133")));
    assert!(publication.changed(State::Playing, 1, Some("second.example.net:19134")));
    assert!(!publication.changed(State::Playing, 1, Some("second.example.net:19134")));
    assert!(publication.changed(State::Playing, 2, Some("second.example.net:19134")));
    assert!(publication.changed(State::Menus, 2, None));
    assert!(publication.last.unwrap().2.is_none());
}

#[test]
fn endpoints_always_include_a_port_and_bracket_ipv6() {
    assert_eq!(
        normalize_endpoint("play.example.net"),
        format!("play.example.net:{}", launcher::menu::DEFAULT_PORT)
    );
    assert_eq!(normalize_endpoint("127.0.0.1:19133"), "127.0.0.1:19133");
    assert_eq!(normalize_endpoint("[::1]:19133"), "[::1]:19133");
    assert_eq!(
        normalize_endpoint("::1"),
        format!("[::1]:{}", launcher::menu::DEFAULT_PORT)
    );
}

#[test]
fn long_endpoint_text_remains_valid_utf8_within_discord_limit() {
    let endpoint = "界".repeat(MAX_STATE_BYTES);
    let payload = serde_json::to_value(State::Playing.activity(1234, Some(&endpoint))).unwrap();
    assert!(payload["state"].as_str().unwrap().len() <= MAX_STATE_BYTES);
}
