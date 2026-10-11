//! Runs the capture command with bounded fixture processes instead of a client or Tracy.
#[cfg(unix)]
#[test]
fn exited_client_reports_startup_failure_instead_of_waiting_for_the_collector() {
    use std::{fs, os::unix::fs::PermissionsExt, process::Command};
    let temp = tempfile::tempdir().unwrap();
    let bin = temp.path().join("bin");
    fs::create_dir(&bin).unwrap();
    fs::create_dir_all(temp.path().join("target/debug")).unwrap();
    for (path, source) in [
        (
            bin.join("make"),
            r#"#!/usr/bin/env python3
import json, pathlib
print(json.dumps({'reason':'compiler-artifact','target':{'name':'bedrock-client','kind':['bin']},'executable':str(pathlib.Path('target/debug/bedrock-client').resolve())}))
"#,
        ),
        (
            temp.path().join("target/debug/bedrock-client"),
            "#!/bin/sh\nexit 37\n",
        ),
        (bin.join("tracy-csvexport"), "#!/bin/sh\nexit 0\n"),
        (
            bin.join("tracy-capture"),
            "#!/usr/bin/env python3\nimport signal,sys\nif '--help' not in sys.argv: signal.pause()\n",
        ),
    ] {
        fs::write(&path, source).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let result = Command::new(env!("CARGO_BIN_EXE_devtool"))
        .current_dir(temp.path())
        .env("PROFILE", "dev")
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("CARGO_BUILD_TARGET_DIR")
        .env_remove("CARGO_BUILD_TARGET")
        .env(
            "PATH",
            format!("{}:{}", bin.display(), std::env::var("PATH").unwrap()),
        )
        .args(["capture", "--seconds", "1", "--out", "output"])
        .output()
        .unwrap();
    assert!(!result.status.success());
    let error = String::from_utf8_lossy(&result.stderr);
    assert!(
        error.contains("client exited") && error.contains("37"),
        "lost startup failure: {error}"
    );
}

#[cfg(unix)]
#[test]
fn capture_connects_to_its_new_client_when_the_default_port_is_occupied() {
    use std::{fs, net::TcpListener, os::unix::fs::PermissionsExt, process::Command};
    const DEFAULT_TRACY_PORT: u16 = 8086;
    let _unrelated = match TcpListener::bind(("127.0.0.1", DEFAULT_TRACY_PORT)) {
        Ok(listener) => Some(listener),
        Err(error) if error.kind() == std::io::ErrorKind::AddrInUse => None,
        Err(error) => panic!("cannot occupy the default Tracy port: {error}"),
    };
    let temp = tempfile::tempdir().unwrap();
    let bin = temp.path().join("bin");
    fs::create_dir(&bin).unwrap();
    fs::create_dir_all(temp.path().join("target/debug")).unwrap();
    assert!(
        Command::new("python3")
            .args(["-c", "import os; os.mkfifo('client-ready')"])
            .current_dir(temp.path())
            .status()
            .unwrap()
            .success()
    );
    for (path, source) in [
        (
            bin.join("make"),
            r#"#!/usr/bin/env python3
import json, pathlib
print(json.dumps({'reason':'compiler-artifact','target':{'name':'bedrock-client','kind':['bin']},'executable':str(pathlib.Path('target/debug/bedrock-client').resolve())}))
"#,
        ),
        (
            temp.path().join("target/debug/bedrock-client"),
            r#"#!/usr/bin/env python3
import os, signal, socket
port = int(os.environ['TRACY_PORT'])
assert port != int(os.environ['UNRELATED_TRACY_PORT'])
with socket.socket() as server:
    server.settimeout(10)
    server.bind(('127.0.0.1', port))
    server.listen(1)
    with open('client-ready', 'w') as ready:
        ready.write('ready')
    connection, _ = server.accept()
    with connection:
        connection.sendall(b'new-capture-client')
        signal.pause()
"#,
        ),
        (
            bin.join("tracy-capture"),
            r#"#!/usr/bin/env python3
import os, pathlib, socket, sys
if '--help' in sys.argv:
    sys.exit(0)
port = int(sys.argv[sys.argv.index('-p') + 1])
assert port != int(os.environ['UNRELATED_TRACY_PORT'])
with open('client-ready') as ready:
    assert ready.read() == 'ready'
connection = socket.create_connection(('127.0.0.1', port), timeout=5)
with connection:
    identity = connection.recv(128)
assert identity == b'new-capture-client'
pathlib.Path(sys.argv[sys.argv.index('-o') + 1]).write_bytes(identity)
"#,
        ),
        (
            bin.join("tracy-csvexport"),
            r#"#!/usr/bin/env python3
import sys
if '--help' in sys.argv:
    sys.exit(0)
sep = sys.argv[sys.argv.index('-s') + 1]
if '-u' in sys.argv:
    rows = [['name', 'src_file', 'src_line', 'ns_since_start', 'exec_time_ns', 'thread', 'value'],
            ['present_frames', 'present.rs', '9', '0', '1', '1', 'frame=0, late=false'],
            ['present_frames', 'present.rs', '9', '16666667', '1', '1', '']]
else:
    rows = [['name', 'src_file', 'src_line', 'total_ns', 'total_perc', 'counts', 'mean_ns', 'min_ns', 'max_ns', 'std_ns'],
            ['fixture<A, B>', 'fixture.rs', '3', '100', '0.1', '1', '100', '100', '100', '0']]
print('\n'.join(sep.join(row) for row in rows))
"#,
        ),
    ] {
        fs::write(&path, source).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let result = Command::new(env!("CARGO_BIN_EXE_devtool"))
        .current_dir(temp.path())
        .env("PROFILE", "dev")
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("CARGO_BUILD_TARGET_DIR")
        .env_remove("CARGO_BUILD_TARGET")
        .env("TRACY_PORT", DEFAULT_TRACY_PORT.to_string())
        .env("UNRELATED_TRACY_PORT", DEFAULT_TRACY_PORT.to_string())
        .env(
            "PATH",
            format!("{}:{}", bin.display(), std::env::var("PATH").unwrap()),
        )
        .args(["capture", "--seconds", "1", "--out", "output"])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(
        fs::read(temp.path().join("output/capture.tracy")).unwrap(),
        b"new-capture-client"
    );
    assert!(temp.path().join("output/summary.json").is_file());
}

#[cfg(unix)]
#[test]
fn capture_launches_the_emitted_executable_and_refuses_cargo_output_overrides() {
    use std::{fs, os::unix::fs::PermissionsExt, process::Command};
    let overrides = [
        None,
        Some("CARGO_TARGET_DIR"),
        Some("CARGO_BUILD_TARGET_DIR"),
        Some("CARGO_BUILD_TARGET"),
    ];
    for (profile, set) in ["dev", "play", "release"]
        .into_iter()
        .flat_map(|profile| overrides.map(|set| (profile, set)))
    {
        let temp = tempfile::tempdir().unwrap();
        let bin = temp.path().join("bin");
        fs::create_dir(&bin).unwrap();
        for (path, source) in [
            (
                bin.join("make"),
                r#"#!/usr/bin/env python3
import json, os, pathlib
pathlib.Path('built').touch()
profile = os.environ['PROFILE']
executable = (pathlib.Path('target') / ('debug' if profile == 'dev' else profile) / 'bedrock-client').resolve()
executable.parent.mkdir(parents=True, exist_ok=True)
executable.write_text('#!/bin/sh\nexit 37\n')
executable.chmod(0o755)
print(json.dumps({'reason':'compiler-artifact','target':{'name':'mod-host','kind':['bin']},'executable':'wrong-helper'}))
print(json.dumps({'reason':'compiler-artifact','target':{'name':'bedrock-client','kind':['bin']},'executable':str(executable)}))
"#,
            ),
            (bin.join("tracy-csvexport"), "#!/bin/sh\nexit 0\n"),
            (
                bin.join("tracy-capture"),
                "#!/usr/bin/env python3\nimport signal,sys\nif '--help' not in sys.argv: signal.pause()\n",
            ),
        ] {
            fs::write(&path, source).unwrap();
            fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
        }
        let mut command = Command::new(env!("CARGO_BIN_EXE_devtool"));
        command
            .current_dir(temp.path())
            .env("PROFILE", profile)
            .env_remove("CARGO_TARGET_DIR")
            .env_remove("CARGO_BUILD_TARGET_DIR")
            .env_remove("CARGO_BUILD_TARGET")
            .env(
                "PATH",
                format!("{}:{}", bin.display(), std::env::var("PATH").unwrap()),
            )
            .args(["capture", "--seconds", "1", "--out", "output"]);
        if let Some(name) = set {
            command.env(name, temp.path().join("custom-output"));
        }
        let result = command.output().unwrap();
        assert!(!result.status.success());
        let error = String::from_utf8_lossy(&result.stderr);
        if let Some(name) = set {
            assert!(error.contains(name), "profile={profile}: {error}");
            assert!(
                !temp.path().join("built").exists(),
                "built before refusing {name}"
            );
        } else {
            assert!(
                error.contains("client exited") && error.contains("37"),
                "profile={profile}: {error}"
            );
        }
    }
}

#[cfg(unix)]
#[test]
fn supervisor_only_sigterm_stops_the_client_and_collector() {
    use std::{
        fs,
        io::{BufRead, BufReader},
        os::unix::fs::PermissionsExt,
        process::{Command, Stdio},
    };
    let temp = tempfile::tempdir().unwrap();
    let bin = temp.path().join("bin");
    fs::create_dir(&bin).unwrap();
    fs::create_dir_all(temp.path().join("target/debug")).unwrap();
    // Each fixture announces its PID on the supervisor's inherited stdout, then waits for a signal.
    let waiter = |role: &str| {
        format!(
            "#!/usr/bin/env python3\nimport os, signal, sys\nif '--help' in sys.argv: sys.exit(0)\nprint('{role}-ready', os.getpid(), flush=True)\nsignal.pause()\n"
        )
    };
    for (path, source) in [
        (
            bin.join("make"),
            r#"#!/usr/bin/env python3
import json, pathlib
print(json.dumps({'reason':'compiler-artifact','target':{'name':'bedrock-client','kind':['bin']},'executable':str(pathlib.Path('target/debug/bedrock-client').resolve())}))
"#
            .to_owned(),
        ),
        (temp.path().join("target/debug/bedrock-client"), waiter("client")),
        (bin.join("tracy-capture"), waiter("collector")),
        (bin.join("tracy-csvexport"), "#!/bin/sh\nexit 0\n".to_owned()),
    ] {
        fs::write(&path, source).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let mut supervisor = Command::new(env!("CARGO_BIN_EXE_devtool"))
        .current_dir(temp.path())
        .env("PROFILE", "dev")
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("CARGO_BUILD_TARGET_DIR")
        .env_remove("CARGO_BUILD_TARGET")
        .env(
            "PATH",
            format!("{}:{}", bin.display(), std::env::var("PATH").unwrap()),
        )
        .args(["capture", "--seconds", "600", "--out", "output"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut fixtures = Vec::new();
    for line in BufReader::new(supervisor.stdout.take().unwrap()).lines() {
        let line = line.unwrap();
        if let Some(pid) = line
            .strip_prefix("client-ready ")
            .or_else(|| line.strip_prefix("collector-ready "))
        {
            fixtures.push(pid.to_owned());
            if fixtures.len() == 2 {
                break;
            }
        }
    }
    assert!(
        Command::new("kill")
            .args(["-TERM", &supervisor.id().to_string()])
            .status()
            .unwrap()
            .success()
    );
    let output = supervisor.wait_with_output().unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("cancelled"));
    for pid in fixtures {
        let alive = Command::new("kill")
            .args(["-0", &pid])
            .stderr(Stdio::null())
            .status()
            .unwrap()
            .success();
        assert!(!alive, "fixture {pid} outlived its supervisor");
    }
}
