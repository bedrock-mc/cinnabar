use super::*;
#[cfg(unix)]
use crate::local_worlds::test_support::write_script;
use crate::local_worlds::test_support::{scratch, spec};
#[cfg(unix)]
use launcher::local_worlds::world::Generator;
#[cfg(unix)]
use std::path::Path;

/// A Dragonfly stand-in: logs its arguments, prints `ready`, and stops on `stop` or stdin EOF.
#[cfg(unix)]
const FAKE_SERVER: &[u8] = b"#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$0.args\"\necho ready\nwhile IFS= read -r line; do\n  [ \"$line\" = stop ] && exit 0\ndone\n";

#[cfg(unix)]
fn runner(dir: &Path, body: &[u8]) -> ProcessRunner {
    let binary = dir.join("server");
    write_script(&binary, body);
    let mut runner = ProcessRunner::new(binary);
    runner.start_timeout = test_time::DEFAULT_TIMEOUT;
    runner
}

#[cfg(unix)]
#[test]
fn a_started_server_pauses_and_stops() {
    let dir = scratch();
    let instance = runner(dir.path(), FAKE_SERVER)
        .start(&spec(dir.path()), &Cancel::never())
        .expect("start");
    assert!(
        instance.address().starts_with("127.0.0.1:"),
        "{}",
        instance.address()
    );
    instance.set_paused(true).expect("pause");
    instance.stop(test_time::DEFAULT_TIMEOUT).expect("stop");
    assert!(
        matches!(
            instance.exited().try_recv(),
            Err(TryRecvError::Disconnected)
        ),
        "the exit is not reported after the stop"
    );
}

#[cfg(unix)]
#[test]
fn a_crash_during_startup_fails_the_start() {
    let dir = scratch();
    let started =
        runner(dir.path(), b"#!/bin/sh\nexit 3\n").start(&spec(dir.path()), &Cancel::never());
    let error = started.err().expect("a crashed server");
    assert!(
        error.to_string().contains("exited during startup"),
        "{error}"
    );
}

#[cfg(unix)]
#[test]
fn a_server_that_never_gets_ready_times_out() {
    let dir = scratch();
    let mut silent = runner(
        dir.path(),
        b"#!/bin/sh\nwhile IFS= read -r line; do :; done\n",
    );
    silent.start_timeout = Duration::from_millis(200);
    let error = silent
        .start(&spec(dir.path()), &Cancel::never())
        .err()
        .expect("never ready");
    assert!(
        error.to_string().contains("did not become ready"),
        "{error}"
    );
}

#[cfg(unix)]
#[test]
fn cancelling_a_start_kills_the_server() {
    let dir = scratch();
    let silent = runner(
        dir.path(),
        b"#!/bin/sh\nwhile IFS= read -r line; do :; done\n",
    );
    let (handle, cancel) = crate::local_worlds::cancel_pair();
    drop(handle);
    let started = silent.start(&spec(dir.path()), &cancel);
    assert!(matches!(started, Err(WorldError::Cancelled)));
}

#[test]
fn a_missing_binary_fails_the_start() {
    let dir = scratch();
    let missing = ProcessRunner::new(dir.path().join("missing"));
    assert!(missing.start(&spec(dir.path()), &Cancel::never()).is_err());
}

/// The server gets the saved generator and the signed seed.
#[cfg(unix)]
#[test]
fn the_generator_and_signed_seed_reach_the_server() {
    let dir = scratch();
    let mut spec = spec(dir.path());
    spec.world.generator = Generator::Normal;
    let instance = runner(dir.path(), FAKE_SERVER)
        .start(&spec, &Cancel::never())
        .expect("start");
    let args = std::fs::read_to_string(dir.path().join("server.args")).expect("argument log");
    let args: Vec<&str> = args.lines().collect();
    for (flag, value) in [
        ("-generator", "normal"),
        ("-seed", "-7"),
        ("-game-mode", "survival"),
    ] {
        let at = args.iter().position(|arg| *arg == flag).expect(flag);
        assert_eq!(args.get(at + 1), Some(&value), "{flag} in {args:?}");
    }
    instance.stop(test_time::DEFAULT_TIMEOUT).expect("stop");
}
