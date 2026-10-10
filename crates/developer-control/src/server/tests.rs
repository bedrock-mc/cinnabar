use std::time::Duration;

use serde_json::json;

use super::*;
use crate::{client::Controller, endpoint::Endpoint, protocol::request_line};

fn scratch(label: &str) -> PathBuf {
    std::env::temp_dir()
        .join(format!("cinnabar-control-{label}-{}", std::process::id()))
        .join("endpoint.json")
}

#[test]
fn wrong_tokens_are_rejected_before_parsing_and_close_the_connection() {
    let (sender, receiver) = crossbeam_channel::unbounded();
    let line = request_line(4, "guess", &Command::Quit);
    let (id, outcome, close) = handle_line(&line, "secret", &sender);
    assert_eq!(id, json!(4));
    assert_eq!(outcome, Err("unauthorized".into()));
    assert!(close);
    let (_, outcome, close) = handle_line(r#"{"id":5,"cmd":"quit"}"#, "secret", &sender);
    assert_eq!(outcome, Err("unauthorized".into()));
    assert!(close);
    let (_, outcome, close) = handle_line(
        r#"{"id":6,"token":"guess","cmd":"no_such_command"}"#,
        "secret",
        &sender,
    );
    assert_eq!(outcome, Err("unauthorized".into()));
    assert!(close);
    assert!(
        receiver.try_recv().is_err(),
        "nothing reached the game loop"
    );
}

#[test]
fn authenticated_commands_reach_the_game_loop_and_get_its_reply() {
    let path = scratch("roundtrip");
    let server = ControlServer::start(&path).unwrap();
    let endpoint = Endpoint::read(&path).unwrap();
    assert_eq!(endpoint.pid, std::process::id());
    let game_loop = std::thread::spawn(move || {
        loop {
            let pending = server.drain().next();
            if let Some(pending) = pending {
                let echoed = format!("{:?}", pending.command);
                pending.reply.send(Ok(json!({ "saw": echoed })));
                return server;
            }
            test_time::idle();
        }
    });
    let mut controller = Controller::connect(&path).unwrap();
    let reply = controller
        .call(&Command::State, Duration::from_secs(5))
        .unwrap();
    assert_eq!(reply["saw"], "State");
    let server = game_loop.join().unwrap();
    drop(server);
    assert!(!path.exists(), "the endpoint file is removed on shutdown");
}

#[test]
fn a_forged_endpoint_cannot_drive_the_client() {
    let path = scratch("forged");
    let server = ControlServer::start(&path).unwrap();
    let mut forged = Endpoint::read(&path).unwrap();
    forged.token = crate::endpoint::random_token();
    let mut controller = Controller::connect_to(&forged).unwrap();
    let error = controller
        .call(&Command::Quit, Duration::from_secs(5))
        .unwrap_err();
    assert_eq!(error, "unauthorized");
    assert!(server.drain().next().is_none());
}

#[test]
fn bad_commands_from_authenticated_callers_keep_the_connection() {
    let (sender, _receiver) = crossbeam_channel::unbounded();
    let (_, outcome, close) = handle_line(
        r#"{"id":1,"token":"secret","cmd":"no_such_command"}"#,
        "secret",
        &sender,
    );
    assert!(outcome.unwrap_err().starts_with("bad command"));
    assert!(!close);
}
