use super::*;

#[test]
fn configuration_uses_default_override_and_explicit_disable() {
    assert_eq!(application_id(None), Ok(DEFAULT_APPLICATION_ID));
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
    assert!(publication.changed(State::Menus, 0));
    assert!(!publication.changed(State::Menus, 0));
    assert!(publication.changed(State::Joining, 0));
    assert!(publication.changed(State::Playing, 0));
    assert!(!publication.changed(State::Playing, 0));
    assert!(publication.changed(State::Playing, 1));
    assert!(!publication.changed(State::Playing, 1));
    assert!(publication.changed(State::Menus, 1));
}

#[test]
fn activity_states_keep_launch_time_and_do_not_publish_private_session_data() {
    let mut states = Vec::new();
    for state in [State::Menus, State::Joining, State::Playing] {
        let payload = serde_json::to_value(state.activity(1234)).unwrap();
        assert_eq!(payload["details"], launcher::PRODUCT_NAME);
        assert_eq!(payload["timestamps"]["start"], 1234);
        assert!(payload.get("secrets").is_none());
        assert!(payload.get("party").is_none());
        states.push(payload["state"].as_str().unwrap().to_owned());
    }
    states.sort();
    states.dedup();
    assert_eq!(states.len(), 3);
}
