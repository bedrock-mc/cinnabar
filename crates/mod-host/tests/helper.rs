//! The developer helper process: a failed callback answers with an error reply and the helper
//! keeps running, a failed start answers with its reason, and the helper's stderr reaches the
//! client.
#![cfg(feature = "execution")]

use std::{
    collections::BTreeSet,
    path::Path,
    sync::Once,
    time::{Duration, Instant},
};

use mod_host::helper::{CallFailure, Dispatch, Event, FailureKind, Helper, Reply};
use server_experience::{
    manifest::{Permission, Scope},
    policy::{DEVELOPER_ENV, MAX_GUEST_MEMORY, MAX_MESSAGE_BYTES},
    runtime::{CALLBACK_FUEL, Capabilities, Command, Principal},
    screen,
};

/// Lets this process launch developer helpers. Every test sets the same value before any helper
/// starts, and nothing else here reads the environment concurrently.
fn developer() {
    static SET: Once = Once::new();
    // SAFETY: set once, before any test thread reads the environment.
    SET.call_once(|| unsafe { std::env::set_var(DEVELOPER_ENV, "1") });
}

/// The terminal test client part, built for wasm32 into a target directory of its own inside
/// this test's, and turned into a component.
fn terminal_component() -> Vec<u8> {
    let exe = std::env::current_exe().unwrap();
    let target = exe
        .ancestors()
        .nth(3)
        .unwrap()
        .join(worktree_dir("mod-host-guests"));
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let output = std::process::Command::new(cargo)
        .current_dir(&root)
        .args(["build", "--locked", "--target", "wasm32-unknown-unknown"])
        .args(["-p", "experience-terminal-client", "--target-dir"])
        .arg(&target)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "building the terminal client part failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let module =
        std::fs::read(target.join("wasm32-unknown-unknown/debug/experience_terminal_client.wasm"))
            .unwrap();
    wit_component::ComponentEncoder::default()
        .module(&module)
        .unwrap()
        .validate(true)
        .encode()
        .unwrap()
}

fn spawn(component: Vec<u8>) -> Helper {
    developer();
    let owner = Principal {
        session: "session".into(),
        bundle: "terminal".into(),
        generation: 1,
    };
    let capabilities = Capabilities {
        scope: Scope {
            permissions: BTreeSet::from([Permission::ModalUi]),
            origins: BTreeSet::new(),
            memory_bytes: MAX_GUEST_MEMORY,
            gpu_bytes: 0,
        },
        assets: BTreeSet::new(),
        templates: BTreeSet::new(),
        channels: Vec::new(),
        actions: BTreeSet::new(),
        max_message_bytes: MAX_MESSAGE_BYTES as u32,
    };
    Helper::spawn_developer(
        Path::new(env!("CARGO_BIN_EXE_mod-host")),
        component,
        owner,
        capabilities,
        1,
    )
    .unwrap()
}

/// The helper's next reply, waiting at most 30 seconds (the helper compiles the component).
fn reply(helper: &mut Helper) -> Reply {
    let since = Instant::now();
    loop {
        if let Some(result) = helper.poll() {
            return result.unwrap();
        }
        assert!(since.elapsed() < Duration::from_secs(30), "no reply");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn call(helper: &mut Helper, channel: &str) -> Reply {
    helper
        .dispatch(Dispatch {
            event: Event::Message {
                channel: channel.into(),
                record: b"[]".to_vec(),
            },
            epoch: 1,
            gui: None,
        })
        .unwrap();
    reply(helper)
}

fn failure(reply: Reply) -> CallFailure {
    match reply {
        Reply::Failed(failure) => failure,
        Reply::Committed { transaction, .. } => panic!("committed {transaction:?}"),
    }
}

fn count(reply: Reply) -> i64 {
    match reply {
        Reply::Committed { transaction, fuel } => match transaction.commands.as_slice() {
            _ if fuel == 0 || fuel >= CALLBACK_FUEL => panic!("used {fuel} fuel"),
            [Command::Collection { rows, .. }] => match rows[0]["#count"] {
                screen::Value::Integer(count) => count,
                ref other => panic!("{other:?}"),
            },
            other => panic!("{other:?}"),
        },
        Reply::Failed(failure) => panic!("failed {failure:?}"),
    }
}

/// The helper's stderr so far, waiting until a line contains `needle`.
fn wait_log(helper: &mut Helper, needle: &str) -> Vec<String> {
    let since = Instant::now();
    let mut lines = Vec::new();
    loop {
        lines.extend(helper.drain_log());
        if lines.iter().any(|line| line.contains(needle)) {
            return lines;
        }
        assert!(
            since.elapsed() < Duration::from_secs(10),
            "no stderr line with {needle:?} in {lines:?}"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// A panic and a fuel exhaustion each answer with an error reply naming the bundle, the callback
/// and the cause; the helper keeps running, and the next call runs on a fresh instance. The
/// helper's restart notice reaches the client through its stderr.
#[test]
fn failed_callbacks_answer_with_their_reason_and_the_helper_runs_on() {
    let mut helper = spawn(terminal_component());
    assert!(matches!(reply(&mut helper), Reply::Committed { .. }));
    assert_eq!(count(call(&mut helper, "terminal.count")), 1);
    assert_eq!(count(call(&mut helper, "terminal.count")), 2);

    let panic = failure(call(&mut helper, "terminal.panic"));
    assert_eq!(panic.kind, FailureKind::Panic);
    assert_eq!(
        (panic.bundle.as_str(), panic.callback.as_str()),
        ("terminal", "dispatch")
    );
    assert!(panic.reason.contains("unreachable"), "{}", panic.reason);
    assert_eq!(count(call(&mut helper, "terminal.count")), 1);
    wait_log(&mut helper, "restarted after a failed callback");

    let fuel = failure(call(&mut helper, "terminal.spin"));
    assert_eq!(fuel.kind, FailureKind::Fuel);
    assert_eq!(fuel.fuel, Some(CALLBACK_FUEL));
    assert!(fuel.reason.contains("fuel"), "{}", fuel.reason);
    assert_eq!(count(call(&mut helper, "terminal.count")), 1);
}

/// A component that imports what the host does not provide fails to start, and the start's
/// reply says why instead of the helper vanishing.
#[test]
fn a_failed_start_answers_with_its_reason() {
    let component = wat::parse_str(
        r#"(component (import "cinnabar:missing/world-access@1.0.0" (instance (export "f" (func)))))"#,
    )
    .unwrap();
    let mut helper = spawn(component);
    let failure = failure(reply(&mut helper));
    assert_eq!(failure.kind, FailureKind::Startup);
    assert_eq!(failure.callback, "init");
    assert_eq!(failure.fuel, None);
    assert!(
        failure
            .reason
            .contains("cinnabar:missing/world-access@1.0.0"),
        "{}",
        failure.reason
    );
}

/// `name` keyed by this worktree. Cargo judges freshness by modification time alone and its
/// dep-info paths are relative to the workspace, so two worktrees building into one target would
/// silently reuse each other's guests; keying the guests' target directory by worktree keeps
/// each worktree's guests its own.
fn worktree_dir(name: &str) -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    env!("CARGO_MANIFEST_DIR").hash(&mut hasher);
    format!("{name}-{:016x}", hasher.finish())
}
