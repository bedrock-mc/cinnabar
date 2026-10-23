use super::*;
#[cfg(unix)]
use crate::local_worlds::test_support::write_script;
use crate::local_worlds::test_support::{scratch, spec};
#[cfg(unix)]
use launcher::local_worlds::world::Generator;
#[cfg(unix)]
use std::path::Path;

#[test]
fn console_acknowledgements_require_the_exact_response() {
    let ack = OutputAck::default();
    let (sender, replied) = crossbeam_channel::bounded(1);
    *lock(&ack.0) = Some(("applied", sender));
    for line in [
        "unrelated applied",
        "applied extra",
        "not applied",
        "[INFO] unrelated",
    ] {
        ack.observe(line);
        assert!(matches!(replied.try_recv(), Err(TryRecvError::Empty)));
    }
    ack.observe("[timestamp INFO] applied");
    assert_eq!(replied.try_recv(), Ok(()));
    ack.observe("applied");
    assert_eq!(replied.try_recv(), Ok(()));
}

/// Uses the real child output reader and command wait without a game installation.
#[cfg(unix)]
#[test]
fn console_confirmation_ends_on_reply_exit_cancel_or_timeout() {
    let dir = scratch();
    let binary = dir.path().join("console");
    write_script(
        &binary,
        br#"#!/bin/sh
echo ready
while IFS= read -r line; do
  case "$line" in
    apply) echo '[INFO] applied' ;;
    exit) exit 0 ;;
    stop) exit 0 ;;
  esac
done
"#,
    );
    let start = || {
        let spawned = spawn(
            Command::new(&binary),
            None,
            true,
            ready_line,
            "console fixture".into(),
            None,
        )
        .expect("spawn");
        await_ready(spawned, test_time::DEFAULT_TIMEOUT, &Cancel::never()).expect("ready")
    };
    let process = start();
    process
        .send_confirmed(
            "apply",
            "applied",
            test_time::DEFAULT_TIMEOUT,
            &Cancel::never(),
        )
        .expect("output confirmation");
    let (handle, cancel) = super::super::cancel_pair();
    drop(handle);
    assert!(matches!(
        process.send_confirmed("apply", "applied", test_time::DEFAULT_TIMEOUT, &cancel),
        Err(WorldError::Cancelled)
    ));
    assert!(
        process
            .send_confirmed("silent", "applied", Duration::ZERO, &Cancel::never())
            .is_err()
    );
    process.stop(test_time::DEFAULT_TIMEOUT).expect("stop");
    let process = start();
    assert!(
        process
            .send_confirmed(
                "exit",
                "applied",
                test_time::DEFAULT_TIMEOUT,
                &Cancel::never()
            )
            .is_err()
    );
    process.stop(test_time::DEFAULT_TIMEOUT).expect("stopped");
}

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

/// The server gets flat terrain and the saved game mode.
#[cfg(unix)]
#[test]
fn flat_terrain_and_game_mode_reach_the_server() {
    let dir = scratch();
    let spec = spec(dir.path());
    let instance = runner(dir.path(), FAKE_SERVER)
        .start(&spec, &Cancel::never())
        .expect("start");
    let args = std::fs::read_to_string(dir.path().join("server.args")).expect("argument log");
    let args: Vec<&str> = args.lines().collect();
    for (flag, value) in [("-generator", "flat"), ("-game-mode", "survival")] {
        let at = args.iter().position(|arg| *arg == flag).expect(flag);
        assert_eq!(args.get(at + 1), Some(&value), "{flag} in {args:?}");
    }
    instance.stop(test_time::DEFAULT_TIMEOUT).expect("stop");
}

/// A Dragonfly stand-in that, started with `-standby`, logs its start line before serving.
#[cfg(unix)]
const FAKE_STANDBY_SERVER: &[u8] = b"#!/bin/sh\nif [ \"$1\" = -standby ]; then IFS= read -r line; printf '%s\\n' \"$line\" > \"$0.start\"; fi\nprintf '%s\\n' \"$@\" > \"$0.args\"\necho ready\nwhile IFS= read -r line; do\n  [ \"$line\" = stop ] && exit 0\ndone\n";

/// A prepared standby server is handed the world instead of a new process being started.
#[cfg(unix)]
#[test]
fn a_prepared_standby_server_takes_the_next_world() {
    let dir = scratch();
    let runner = runner(dir.path(), FAKE_STANDBY_SERVER);
    runner.prepare();
    let instance = runner
        .start(&spec(dir.path()), &Cancel::never())
        .expect("start");
    assert!(
        instance.address().starts_with("127.0.0.1:"),
        "{}",
        instance.address()
    );
    let line = std::fs::read_to_string(dir.path().join("server.start")).expect("start line");
    let args: Vec<String> =
        serde_json::from_str(line.trim().strip_prefix("start ").expect("a start line"))
            .expect("JSON arguments");
    let addr = args.iter().position(|arg| arg == "-addr").expect("-addr");
    assert_eq!(args[addr + 1], instance.address());
    instance.stop(test_time::DEFAULT_TIMEOUT).expect("stop");

    // The standby is used once; the next start launches its own server.
    let again = runner
        .start(&spec(dir.path()), &Cancel::never())
        .expect("second start");
    let logged = std::fs::read_to_string(dir.path().join("server.args")).expect("argument log");
    assert!(logged.lines().any(|arg| arg == "-dir"), "{logged}");
    again.stop(test_time::DEFAULT_TIMEOUT).expect("stop");
}

/// Discarding ends a waiting standby server at once.
#[cfg(unix)]
#[test]
fn a_discarded_standby_server_exits() {
    let dir = scratch();
    let runner = runner(dir.path(), FAKE_STANDBY_SERVER);
    runner.prepare();
    let exited = lock(&runner.standby)
        .as_ref()
        .expect("a standby server")
        .process
        .exited();
    runner.discard_standby();
    assert!(lock(&runner.standby).is_none());
    test_time::eventually("the standby to exit", || {
        matches!(exited.try_recv(), Err(TryRecvError::Disconnected))
    });
}

#[cfg(unix)]
type RecordedEndings = Arc<Mutex<Vec<Ending>>>;

/// Launches `binary` with a hook that records how it ended.
#[cfg(unix)]
fn launch_recorded(
    binary: &Path,
    timeout: Duration,
    cancel: &Cancel,
) -> (Result<Arc<Process>, WorldError>, RecordedEndings) {
    let endings = Arc::new(Mutex::new(Vec::new()));
    let recorded = Arc::clone(&endings);
    let launched = launch(
        Command::new(binary),
        Launch {
            address: String::new(),
            timeout,
            can_pause: false,
            ready: ready_line,
            label: "hook fixture".into(),
            on_exit: Some(Box::new(move |ending| lock(&recorded).push(ending))),
        },
        cancel,
    );
    (launched, endings)
}

/// Each launch reports its ending once, before its exit is observable or its failed launch returns.
#[cfg(unix)]
#[test]
fn every_launch_reports_how_it_ended_before_its_exit() {
    let dir = scratch();
    let script = |name: &str, body: &[u8]| {
        let path = dir.path().join(name);
        write_script(&path, body);
        path
    };
    let server = script("server", FAKE_SERVER);
    let (launched, endings) =
        launch_recorded(&server, test_time::DEFAULT_TIMEOUT, &Cancel::never());
    let process = launched.expect("ready");
    assert!(lock(&endings).is_empty());
    process.stop(test_time::DEFAULT_TIMEOUT).expect("stop");
    assert_eq!(*lock(&endings), [Ending::Clean]);

    let crash_on_stop = script(
        "crash-on-stop",
        b"#!/bin/sh\necho ready\nwhile IFS= read -r line; do\n  [ \"$line\" = stop ] && exit 134\ndone\n",
    );
    let (launched, endings) =
        launch_recorded(&crash_on_stop, test_time::DEFAULT_TIMEOUT, &Cancel::never());
    let process = launched.expect("ready");
    let _ = process.stop(test_time::DEFAULT_TIMEOUT);
    assert_eq!(*lock(&endings), [Ending::Failed]);

    let crash = script("crash", b"#!/bin/sh\nexit 3\n");
    let (launched, endings) = launch_recorded(&crash, test_time::DEFAULT_TIMEOUT, &Cancel::never());
    assert!(launched.is_err());
    assert_eq!(*lock(&endings), [Ending::Failed]);

    let silent = script(
        "silent",
        b"#!/bin/sh\nwhile IFS= read -r line; do :; done\n",
    );
    let (launched, endings) = launch_recorded(&silent, Duration::ZERO, &Cancel::never());
    assert!(launched.is_err(), "a start timeout");
    assert_eq!(*lock(&endings), [Ending::Failed]);

    let (handle, cancel) = crate::local_worlds::cancel_pair();
    drop(handle);
    let (launched, endings) = launch_recorded(&silent, test_time::DEFAULT_TIMEOUT, &cancel);
    assert!(matches!(launched, Err(WorldError::Cancelled)));
    assert_eq!(*lock(&endings), [Ending::Cancelled]);

    let (launched, endings) = launch_recorded(
        &dir.path().join("missing"),
        test_time::DEFAULT_TIMEOUT,
        &Cancel::never(),
    );
    assert!(launched.is_err());
    assert_eq!(*lock(&endings), [Ending::Unstarted]);
}

/// A cancelled start that ended any way other than its kill, such as a crash, is a failure.
#[cfg(unix)]
#[test]
fn a_cancellation_does_not_hide_a_crash() {
    use std::os::unix::process::ExitStatusExt;

    let killed = ExitStatus::from_raw(libc::SIGKILL);
    let aborted = ExitStatus::from_raw(libc::SIGABRT);
    let exited = ExitStatus::from_raw(134 << 8);
    assert_eq!(ending(Some(killed), true), Ending::Cancelled);
    assert_eq!(ending(Some(killed), false), Ending::Failed);
    for crash in [aborted, exited] {
        assert_eq!(ending(Some(crash), true), Ending::Failed);
    }
    assert_eq!(ending(None, true), Ending::Failed);
    assert_eq!(ending(Some(ExitStatus::from_raw(0)), true), Ending::Clean);
}

/// Natural saves must not be extended with flat terrain by the built-in server.
#[cfg(unix)]
#[test]
fn natural_terrain_is_rejected_before_starting_a_process() {
    let dir = scratch();
    let mut spec = spec(dir.path());
    spec.world.generator = Generator::Normal;
    let error = runner(dir.path(), FAKE_SERVER)
        .start(&spec, &Cancel::never())
        .err()
        .expect("natural terrain is unavailable");
    assert_eq!(
        error.public_message(),
        format!("invalid world settings: {NATURAL_TERRAIN_REQUIRES_BDS}")
    );
    assert!(!dir.path().join("server.args").exists());
}
