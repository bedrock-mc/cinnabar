//! Generated handler routing and compile-time direction conformance.

use std::{fs, path::Path, process::Command};

use super::generate_at;

const SHARED: &str = r#"#![project(id = "handlers", permissions(messaging))]
#[message(id = "handlers.ping", schema = 2, to_server)]
pub struct Ping { pub request: u32 }
#[message(id = "handlers.pong", schema = 3, to_client, control)]
pub struct Pong { pub request: u32 }
#[message(id = "handlers.state", schema = 1, to_client)]
pub struct State { pub enabled: bool }
"#;

const CLIENT: &str = r#"
use experience_sdk::{client::{self, ClientPart}, entry::client, message::{Message, ToClient}, Value};
mod shared { include!("../out/mod_shared.rs"); }
use shared::{Ping, Pong, State};
struct Client;
static RECEIVED: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
#[client]
impl ClientPart for Client {
    fn init() {}
    /// Records the decoded response for the routing assertion.
    #[on_message]
    fn pong(pong: Pong) { RECEIVED.store(pong.request, std::sync::atomic::Ordering::Relaxed); }
    /// Receives a distinct declared channel.
    #[on_message]
    fn state(state: State) { assert!(state.enabled); }
}
#[test]
fn routes_and_silently_drops_rejected_records() {
    use client::__private::receive;
    receive::<Client>(Pong::CHANNEL.id.into(), serde_json::to_vec(&Pong { request: 42 }.encode().unwrap()).unwrap());
    assert_eq!(RECEIVED.load(std::sync::atomic::Ordering::Relaxed), 42);
    receive::<Client>(State::CHANNEL.id.into(), serde_json::to_vec(&State { enabled: true }.encode().unwrap()).unwrap());
    for (channel, payload) in [
        ("handlers.unknown", b"[]".to_vec()),
        (Pong::CHANNEL.id, b"[]".to_vec()),
        (Pong::CHANNEL.id, serde_json::to_vec(&[Value::Text("wrong type".into())]).unwrap()),
        (Pong::CHANNEL.id, serde_json::to_vec(&[Value::Integer(-1)]).unwrap()),
        (Pong::CHANNEL.id, b"invalid JSON".to_vec()),
    ] {
        receive::<Client>(channel.into(), payload);
        assert_eq!(RECEIVED.load(std::sync::atomic::Ordering::Relaxed), 42);
    }
    assert_eq!(RECEIVED.load(std::sync::atomic::Ordering::Relaxed), 42);
    assert!(Pong::CONTROL);
    assert!(!State::CONTROL);
    let _send: fn(&Ping) -> Result<(), String> = client::send::<Ping>;
}
"#;

const SERVER: &str = r#"
use experience_sdk::{entry::server, server::{Callback, Experience, GuestError, PlayerId}};
mod shared { include!("../out/mod_shared.rs"); }
use shared::{Ping, Pong, State};
struct Server;
#[server]
impl Experience for Server {
    /// Echoes the request through a retained response.
    #[on_message]
    fn ping(ctx: &Callback, player: PlayerId, ping: Ping) -> Result<(), GuestError> {
        ctx.send(&player, &Pong { request: ping.request })?;
        ctx.send(&player, &State { enabled: true })
    }
}
"#;

#[test]
fn generated_handlers_route_and_direction_errors_fail_compilation() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("src")).unwrap();
    fs::create_dir(root.path().join("assets")).unwrap();
    fs::write(root.path().join("src/shared.rs"), SHARED).unwrap();
    generate_at(root.path(), &root.path().join("out")).unwrap();
    fs::write(
        root.path().join("Cargo.toml"),
        format!(
            r#"[package]
name = "handler-conformance"
version = "0.0.0"
edition = "2024"
[dependencies]
experience-sdk = {{ path = {:?}, default-features = false }}
serde_json = "1"
[features]
client = ["experience-sdk/client"]
server = ["experience-sdk/server"]
[workspace]
"#,
            env!("CARGO_MANIFEST_DIR")
        ),
    )
    .unwrap();
    fs::write(root.path().join("src/lib.rs"), CLIENT).unwrap();
    cargo(root.path(), "client", "test", true, "");
    for (source, feature, expected) in [
        (
            CLIENT.replace("fn pong(pong: Pong)", "fn pong(pong: Ping)"),
            "client",
            "ToClient",
        ),
        (
            CLIENT
                .replace("client::send::<Ping>", "client::send::<Pong>")
                .replace("fn(&Ping)", "fn(&Pong)"),
            "client",
            "ToServer",
        ),
        (
            SERVER.replace(
                "fn ping(ctx: &Callback, player: PlayerId, ping: Ping)",
                "fn ping(ctx: &Callback, player: PlayerId, ping: Pong)",
            ),
            "server",
            "ToServer",
        ),
        (
            SERVER.replace(
                "&Pong { request: ping.request }",
                "&Ping { request: ping.request }",
            ),
            "server",
            "ToClient",
        ),
        (
            SERVER.replace(
                "ctx.send(&player, &Pong { request: ping.request })?",
                "ctx.send_control(&player, \"raw\", 1, &[])?",
            ),
            "server",
            "no method named `send_control`",
        ),
        (
            CLIENT
                .replace("fn state(state: State)", "fn state(state: Pong)")
                .replace("assert!(state.enabled)", "let _ = state.request"),
            "client",
            "conflicting implementations",
        ),
    ] {
        fs::write(root.path().join("src/lib.rs"), source).unwrap();
        cargo(root.path(), feature, "check", false, expected);
    }
    fs::write(root.path().join("src/lib.rs"), SERVER).unwrap();
    cargo(root.path(), "server", "check", true, "");
}

/// Checks status and the specific direction diagnostic from a standalone generated guest.
fn cargo(root: &Path, feature: &str, command: &str, success: bool, expected: &str) {
    let output = Command::new("cargo")
        .args([
            command,
            "--offline",
            "--quiet",
            "--all-targets",
            "--features",
            feature,
        ])
        .env("CARGO_TARGET_DIR", root.join("target"))
        .env("CARGO_BUILD_BUILD_DIR", root.join("build"))
        .current_dir(root)
        .output()
        .expect("Cargo is required for handler conformance");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.success(), success, "{stderr}");
    assert!(stderr.contains(expected), "expected {expected}: {stderr}");
}
