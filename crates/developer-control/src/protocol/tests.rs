use serde_json::json;

use super::*;
use crate::camera::Easing;

#[test]
fn sign_in_fixture_accepts_only_fixed_states_without_account_data() {
    assert_eq!(
        parse(json!({ "cmd": "sign_in_fixture", "state": "opened" })).unwrap(),
        Command::SignInFixture {
            state: SignInFixtureState::Opened
        }
    );
    assert!(
        parse(json!({ "cmd": "sign_in_fixture", "state": "opened", "code": "secret" })).is_err()
    );
    assert!(parse(json!({ "cmd": "sign_in_fixture", "state": "real_account" })).is_err());
}

fn parse(value: serde_json::Value) -> Result<Command, String> {
    let mut value = value;
    value["token"] = json!("t");
    Request::parse(&value.to_string())
        .map_err(|error| error.message)?
        .command()
}

#[test]
fn commands_parse_from_their_wire_form() {
    assert_eq!(
        parse(json!({ "cmd": "connect", "address": "127.0.0.1:19132" })).unwrap(),
        Command::Connect {
            address: "127.0.0.1:19132".into()
        }
    );
    assert_eq!(
        parse(json!({ "cmd": "chat", "text": "/showcase souls" })).unwrap(),
        Command::Chat {
            text: "/showcase souls".into()
        }
    );
    let Command::Input(input) = parse(json!({
        "cmd": "input", "hold": ["Digit1"], "press": ["key.attack"], "press_frames": 3,
        "move": { "forward": 1 }, "sprint": true, "hotbar": 2,
        "look": { "yaw": 90, "pitch": -10, "frames": 30 }
    }))
    .unwrap() else {
        panic!("not input");
    };
    assert_eq!(input.hold, ["Digit1"]);
    assert_eq!(input.press_frames, Some(3));
    assert_eq!(input.movement.unwrap().forward, 1.0);
    assert_eq!(input.look.unwrap().frames, 30);
    let Command::CameraPath(path) = parse(json!({
        "cmd": "camera_path", "easing": "linear",
        "keyframes": [{ "t": 0, "position": [0, 70, 0], "yaw": 0, "pitch": 10 }]
    }))
    .unwrap() else {
        panic!("not a camera path");
    };
    assert_eq!(path.easing, Easing::Linear);
    let Command::RecordStart(record) =
        parse(json!({ "cmd": "record_start", "path": "clip.mp4" })).unwrap()
    else {
        panic!("not record_start");
    };
    assert_eq!(record.fps, DEFAULT_FPS);
    assert!(record.fixed_clock && record.audio);
    assert_eq!(
        parse(json!({ "cmd": "wait_for", "condition": { "chunks_loaded": { "radius": 4 } } }))
            .unwrap(),
        Command::WaitFor {
            condition: Condition::ChunksLoaded { radius: 4 },
            timeout_ms: None
        }
    );
    assert_eq!(parse(json!({ "cmd": "quit" })).unwrap(), Command::Quit);
}

#[test]
fn pointer_wheel_and_test_cape_parse_from_wire() {
    let Command::Input(input) = parse(json!({
        "cmd": "input", "pointer": {"x": 10, "y": 20}, "wheel": {"y": -1}, "press": ["MouseLeft"]
    }))
    .unwrap() else {
        panic!("not input")
    };
    assert_eq!(input.pointer, Some(Pointer { x: 10.0, y: 20.0 }));
    assert_eq!(
        input.wheel,
        Some(Wheel {
            y: -1.0,
            ..Wheel::default()
        })
    );
    assert!(parse(json!({"cmd": "input", "pointer": {"x": 1, "y": 2, "z": 3}})).is_err());
    assert!(parse(json!({"cmd": "input", "wheel": {"unit": "invalid"}})).is_err());
    assert_eq!(
        parse(json!({"cmd": "test_cape", "enabled": false})).unwrap(),
        Command::TestCape { enabled: false }
    );
    assert_eq!(
        parse(json!({"cmd": "test_accounts", "enabled": true})).unwrap(),
        Command::TestAccounts { enabled: true }
    );
}

#[test]
fn unknown_commands_and_fields_are_rejected() {
    assert!(parse(json!({ "cmd": "fly_to_moon" })).is_err());
    assert!(parse(json!({ "cmd": "input", "jumpp": true })).is_err());
    assert!(parse(json!({ "cmd": "connect" })).is_err());
}

#[test]
fn envelopes_without_tokens_are_unauthenticated() {
    let missing = Request::parse(r#"{"id":3,"cmd":"state"}"#).unwrap_err();
    assert_eq!(missing.id, json!(3));
    assert!(missing.unauthenticated);
    assert!(Request::parse("not json").unwrap_err().unauthenticated);
    assert!(Request::parse("[1]").unwrap_err().unauthenticated);
}

#[test]
fn request_and_reply_lines_round_trip() {
    let line = request_line(9, "secret", &Command::State);
    let request = Request::parse(&line).unwrap();
    assert_eq!(
        (request.id.clone(), request.token.clone()),
        (json!(9), "secret".into())
    );
    assert_eq!(request.command().unwrap(), Command::State);
    assert_eq!(
        parse_reply(r#"{"id":9,"ok":true,"result":{"x":1}}"#).unwrap(),
        (json!(9), Ok(json!({ "x": 1 })))
    );
    assert_eq!(
        parse_reply(r#"{"id":9,"ok":false,"error":"nope"}"#).unwrap(),
        (json!(9), Err("nope".into()))
    );
}
