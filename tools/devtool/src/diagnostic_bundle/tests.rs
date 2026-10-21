use super::*;

/// A crash folder's report member, as `run` names it.
const CRASH_REPORT: &str = "crashes/20251014-120000-0123456789abcdef/report.json";

/// Bundles `entries` and returns every archive member's text, concatenated.
fn bundled(entries: &[(String, String)]) -> String {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("bundle.zip");
    bundle(&path, entries).unwrap();
    let mut zip = zip::ZipArchive::new(fs::File::open(path).unwrap()).unwrap();
    let mut all = String::new();
    for index in 0..zip.len() {
        zip.by_index(index)
            .unwrap()
            .read_to_string(&mut all)
            .unwrap();
    }
    all
}

#[test]
fn authorization_values_of_any_scheme_are_removed_whole() {
    let all = bundled(&[
            (
                "client.log".into(),
                concat!(
                    "DEBUG http: request headers Authorization: Basic dXNlcjpzZWNyZXQ= accept=json\n",
                    "DEBUG proxy: Authorization=\"Basic cGFzczp3b3Jk\"\n",
                    "DEBUG xsts: authorization=XBL3.0 x=2535412345678901;eyJhbGciOiJSUzI1NiJ9.eyJ4dWkiOltdfQ.c2ln\n",
                )
                .into(),
            ),
            (
                "core.log".into(),
                "retrying with dXNlcjpzZWNyZXQ= and cGFzczp3b3Jk and XBL3.0 x=2535412345678901;eyJhbGciOiJSUzI1NiJ9.eyJ4dWkiOltdfQ.c2ln\n".into(),
            ),
        ]);
    assert!(all.contains("accept=json") && all.contains("retrying with"));
    for private in [
        "dXNlcjpzZWNyZXQ",
        "cGFzczp3b3Jk",
        "2535412345678901",
        "eyJhbGciOiJSUzI1NiJ9",
    ] {
        assert!(!all.contains(private), "leaked {private}: {all}");
    }
}

#[test]
fn json_embedded_in_crash_strings_removes_every_private_array_element() {
    let report = serde_json::json!({
        "source": "client",
        "message": r#"panicked at crates/client-runtime/src/auth.rs:40:9: refresh rejected {"token":["first-secret","second-secret"],"status":401}"#,
        "log_tail": "level=WARN msg=\"refresh failed\" body={\"token\":[\"first-secret\",\"second-secret\"]}\n",
    });
    let all = bundled(&[(CRASH_REPORT.into(), report.to_string())]);
    assert!(all.contains("refresh rejected") && all.contains("401"));
    for private in ["first-secret", "second-secret"] {
        assert!(!all.contains(private), "leaked {private}: {all}");
    }
}

#[test]
fn windows_panic_locations_and_backtraces_keep_their_backslashes() {
    let message = "panicked at crates\\client-runtime\\src\\net\\session.rs:212:17:\ncalled `Option::unwrap()` on a `None` value";
    let backtrace = r"   0: std::backtrace_rs::backtrace::dbghelp64::trace
             at /rustc/90b35a6239c3d8bdabc530a6a0816f7ff89a0aaf/library\std\src\..\..\backtrace\src\backtrace\dbghelp64.rs:91
   1: bridge::frame::decode
             at C:\work\cinnabar\crates\bridge\src\frame.rs:44:13
   2: client_runtime::net::session::run
             at C:\work\cinnabar\crates\client-runtime\src\net\session.rs:212:17
";
    let report = serde_json::json!({
        "source": "client",
        "message": message,
        "backtrace": backtrace,
        "os": "windows",
    });
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("bundle.zip");
    bundle(&path, &[(CRASH_REPORT.into(), report.to_string())]).unwrap();
    let mut zip = zip::ZipArchive::new(fs::File::open(path).unwrap()).unwrap();
    let mut text = String::new();
    zip.by_name(CRASH_REPORT)
        .unwrap()
        .read_to_string(&mut text)
        .unwrap();
    let value: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(value["message"], message);
    assert_eq!(value["backtrace"], backtrace);
}

#[test]
fn ansi_styled_tracing_fields_are_learned_and_redacted() {
    let styled = "\x1b[2m2026-10-12T12:34:56.789012Z\x1b[0m \x1b[32m INFO\x1b[0m \x1b[2mclient_session::login\x1b[0m\x1b[2m:\x1b[0m signed in \x1b[3mgamertag\x1b[0m\x1b[2m=\x1b[0m\"Private Player\" \x1b[3mtoken\x1b[0m\x1b[2m=\x1b[0m\"secret value\"\n";
    let all = bundled(&[
        ("client.log".into(), styled.into()),
        ("core.log".into(), "Private Player connected\n".into()),
    ]);
    assert!(all.contains("signed in") && all.contains("connected"));
    for private in ["Private", "Player", "secret", "value"] {
        assert!(!all.contains(private), "leaked {private}: {all}");
    }
}

#[test]
fn debug_quoted_windows_paths_are_scrubbed_before_a_second_decode() {
    let all = bundled(&[(
        "client.log".into(),
        "INFO client::logs: opened path=\"C:\\\\Users\\\\bob\\\\logs\"\n".into(),
    )]);
    assert!(all.contains("logs"));
    assert!(!all.contains("ob"), "leaked username: {all}");
}

#[test]
fn home_folders_with_spaces_are_redacted_whole_and_everywhere() {
    let all = bundled(&[
            (
                "client.log".into(),
                concat!(
                    "INFO client::paths: data dir C:\\Users\\Fixture Person\\AppData\\Roaming\\cinnabar\n",
                    "INFO client::paths: home=\"C:\\\\Users\\\\Fixture Person\"\n",
                    "INFO client::paths: profile '/Users/Fixture Person'\n",
                )
                .into(),
            ),
            ("core.log".into(), "Fixture Person connected\n".into()),
        ]);
    assert!(all.contains("AppData") && all.contains("connected"));
    for private in ["Fixture", "Person"] {
        assert!(!all.contains(private), "leaked {private}: {all}");
    }
}
#[test]
fn excessive_escape_depth_never_publishes_a_bundle() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("bundle.zip");
    let mut text = "gamertag=\"Fixture Player\"".to_owned();
    for _ in 0..=redact::MAX_ESCAPE_DEPTH {
        text = serde_json::to_string(&text).unwrap();
    }
    assert!(bundle(&path, &[("client.log".into(), text)]).is_err());
    assert!(!path.exists());
}

#[test]
fn escaped_payloads_remove_learned_secrets_and_unlabelled_identities_everywhere() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("bundle.zip");
    let secret = "FIXTURE_PRIVATE_TOKEN\"with\\slash";
    let name = "Escaped \"Fixture\" Player";
    let payload = serde_json::json!({"access_token": secret, "gamertag": name}).to_string();
    let escaped = serde_json::to_string(&payload).unwrap();
    let double = serde_json::to_string(&escaped).unwrap();
    let unicode: String = name
        .chars()
        .map(|c| format!("\\u{:04x}", c as u32))
        .collect();
    bundle(&path, &[
            ("client.log".into(), format!("error={escaped}\nerror={double}\nlater {name} connected\n")),
            ("core.log".into(), format!("unlabelled {secret}\nencoded {}\nunicode {unicode}\n", serde_json::to_string(name).unwrap())),
            (CRASH_REPORT.into(), serde_json::json!({"message": double, "later": format!("{name} connected {secret}"), (name): "connected"}).to_string()),
        ]).unwrap();
    let mut zip = zip::ZipArchive::new(fs::File::open(path).unwrap()).unwrap();
    let mut all = String::new();
    for index in 0..zip.len() {
        zip.by_index(index)
            .unwrap()
            .read_to_string(&mut all)
            .unwrap();
    }
    assert!(all.contains("connected"));
    for private in ["FIXTURE_PRIVATE_TOKEN", "Escaped", "Fixture", &unicode] {
        assert!(!all.contains(private), "leaked fixture value: {all}");
    }
}

#[test]
fn bundles_tagged_core_sessions_and_their_rotated_join_timeline() {
    let directory = tempfile::tempdir().unwrap();
    let logs = directory.path().join("logs");
    fs::create_dir(&logs).unwrap();
    fs::write(
        logs.join("client.log"),
        "CLIENT_SESSION_START session=0123456789abcdef timestamp_ms=2\n",
    )
    .unwrap();
    fs::write(
        logs.join("core.log.1"),
        "CORE_SESSION_START timestamp_ms=1 session=previous\nstale\nCORE_SESSION_START timestamp_ms=3 session=0123456789abcdef\nupstream first elapsed_ms=4\n",
    )
    .unwrap();
    fs::write(
        logs.join("core.log"),
        "CORE_SESSION_START timestamp_ms=5 session=0123456789abcdef\r\nupstream second elapsed_ms=6\n",
    )
    .unwrap();
    run(&["--data-root".into(), directory.path().display().to_string()]).unwrap();
    let members = archive_members(directory.path());
    for member in ["core.log", "join-timeline.txt"] {
        let text = &members[member];
        assert!(text.contains("upstream first"), "missing {member}: {text}");
        assert!(text.contains("upstream second"), "missing {member}: {text}");
        assert!(!text.contains("stale"));
    }
}

#[test]
fn rotated_core_logs_keep_all_current_session_events() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("core.log");
    fs::write(path.with_file_name("core.log.1"), "CORE_SESSION_START timestamp_ms=1\nstale\nCORE_SESSION_START timestamp_ms=3\nupstream first elapsed_ms=4\n").unwrap();
    fs::write(
        &path,
        "CORE_SESSION_START timestamp_ms=5\nupstream second elapsed_ms=6\n",
    )
    .unwrap();
    let text = core_session_tail(&path, "CLIENT_SESSION_START timestamp_ms=2\n").unwrap();
    assert!(text.contains("first"), "lost earlier core run: {text}");
    assert!(text.contains("second"));
    assert!(!text.contains("stale"));
    let mut current = fs::OpenOptions::new().append(true).open(&path).unwrap();
    current.write_all(&vec![b'x'; LOG_LIMIT as usize]).unwrap();
    current
        .write_all(b"\nupstream tail elapsed_ms=7\n")
        .unwrap();
    drop(current);
    let bounded = core_session_tail(&path, "CLIENT_SESSION_START timestamp_ms=2\n").unwrap();
    assert_eq!(bounded.len() as u64, LOG_LIMIT);
    assert!(bounded.contains("upstream tail"));
    assert!(!bounded.contains("first"));
    fs::remove_file(&path).unwrap();
    assert!(
        core_session_tail(&path, "CLIENT_SESSION_START timestamp_ms=2\n")
            .unwrap()
            .contains("first")
    );
}

#[test]
fn crash_json_strings_are_decoded_redacted_and_serialized() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("bundle.zip");
    let report = serde_json::json!({"log_tail": "gamertag=\"Private Player\" token=\"secret value\"", "nested": [{"message": "Private Player connected"}], "gamertag": "Other Player"});
    bundle(
        &path,
        &[
            (CRASH_REPORT.into(), report.to_string()),
            ("core.log".into(), "Other Player connected".into()),
        ],
    )
    .unwrap();
    let mut zip = zip::ZipArchive::new(fs::File::open(path).unwrap()).unwrap();
    let mut text = String::new();
    zip.by_name(CRASH_REPORT)
        .unwrap()
        .read_to_string(&mut text)
        .unwrap();
    let value: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert!(
        value["nested"][0]["message"]
            .as_str()
            .unwrap()
            .contains("connected")
    );
    for secret in ["Private", "Player", "secret", "value", "Other"] {
        assert!(!text.contains(secret), "leaked {secret}");
    }
    text.clear();
    zip.by_name("core.log")
        .unwrap()
        .read_to_string(&mut text)
        .unwrap();
    assert!(!text.contains("Other Player"));
}

#[test]
fn escaped_crash_fields_never_strip_backslashes_from_unicode_escaped_identities() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("bundle.zip");
    let report = serde_json::json!({
        "source": "client",
        "message": "panicked at crates/client-runtime/src/session.rs:212:17:\nlogin rejected",
        "log_tail": "time=2026-10-12T12:34:56.789Z level=INFO msg=\"upstream login\" gamertag=\"Private Player\"\n",
    });
    let unicode: String = "Private Player"
        .chars()
        .map(|c| format!("\\u{:04x}", c as u32))
        .collect();
    bundle(
        &path,
        &[
            (CRASH_REPORT.into(), report.to_string()),
            (
                "core.log".into(),
                format!("level=INFO msg=\"player {unicode} connected\"\n"),
            ),
        ],
    )
    .unwrap();
    let mut zip = zip::ZipArchive::new(fs::File::open(path).unwrap()).unwrap();
    let mut all = String::new();
    for index in 0..zip.len() {
        zip.by_index(index)
            .unwrap()
            .read_to_string(&mut all)
            .unwrap();
    }
    assert!(all.contains("connected"));
    for private in ["Private", "Player", "u0050", "u0072"] {
        assert!(!all.contains(private), "leaked {private}: {all}");
    }
}

#[test]
fn rust_backtraces_keep_symbols_and_source_locations_while_addresses_are_redacted() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("bundle.zip");
    let rustc = "/rustc/90b35a6239c3d8bdabc530a6a0816f7ff89a0aaf/library";
    let message = "panicked at crates/launcher-host/src/lifecycle/crash.rs:65:9:\ncalled `Option::unwrap()` on a `None` value";
    let backtrace = format!(
        "   0: std::backtrace_rs::backtrace::libunwind::trace
             at {rustc}/std/src/../../backtrace/src/backtrace/libunwind.rs:116:5
   1: launcher_host::lifecycle::crash::install_panic_hook::{{{{closure}}}}
             at ./crates/launcher-host/src/lifecycle/crash.rs:35:9
   2: <alloc::boxed::Box<F,A> as core::ops::function::Fn<Args>>::call
             at {rustc}/alloc/src/boxed.rs:1984:9
   3: std::panicking::rust_panic_with_hook
             at {rustc}/std/src/panicking.rs:841:13
   4: core::panicking::panic_fmt
             at {rustc}/core/src/panicking.rs:75:14
   5: <u32 as core::ops::Add>::add
   6: launcher_host::lifecycle::crash::write_report::{{{{closure}}}}::{{{{closure}}}}
   7: cpal::host::coreaudio::macos::audio_unit_from_device
   8: client_audio::output::open
             at ./crates/client-audio/src/output.rs:88:41
"
    );
    let log_tail = "time=2026-10-12T12:34:56.789Z level=WARN msg=\"dial failed\" err=\"dial udp 203.0.113.7:19132 -> [2001:db8::7]:19132: i/o timeout; resolved play.example.invalid:19132 via fe80::1%en0; fallback [fd00::]:19132 or ::1\"\n";
    let report = serde_json::json!({
        "source": "client",
        "message": message,
        "backtrace": backtrace,
        "log_tail": log_tail,
        "release": "0.1.0",
        "os": "macos",
        "arch": "aarch64",
    });
    bundle(&path, &[(CRASH_REPORT.into(), report.to_string())]).unwrap();
    let mut zip = zip::ZipArchive::new(fs::File::open(path).unwrap()).unwrap();
    let mut text = String::new();
    zip.by_name(CRASH_REPORT)
        .unwrap()
        .read_to_string(&mut text)
        .unwrap();
    let value: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(value["backtrace"], backtrace.as_str());
    assert_eq!(value["message"], message);
    let log_tail = value["log_tail"].as_str().unwrap();
    assert!(
        log_tail.contains("12:34:56.789Z"),
        "time redacted: {log_tail}"
    );
    for private in [
        "203.0.113",
        "2001:db8",
        "fe80",
        "fd00",
        "::1",
        "play.example",
    ] {
        assert!(!log_tail.contains(private), "leaked {private}: {log_tail}");
    }
}

#[test]
fn long_current_core_session_keeps_its_tail_and_join_timeline() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir(temp.path().join("logs")).unwrap();
    fs::write(
        temp.path().join("logs/client.log"),
        "CLIENT_SESSION_START timestamp_ms=2\n",
    )
    .unwrap();
    let mut core = fs::File::create(temp.path().join("logs/core.log")).unwrap();
    core.write_all(
        b"CORE_SESSION_START timestamp_ms=1\nstale\nCORE_SESSION_START timestamp_ms=3\n",
    )
    .unwrap();
    for _ in 0..(LOG_LIMIT / 128 + 1) {
        writeln!(core, "{}", "x".repeat(128)).unwrap();
    }
    core.write_all(b"upstream StartGame elapsed_ms=12\n")
        .unwrap();
    drop(core);
    run(&["--data-root".into(), temp.path().display().to_string()]).unwrap();
    let path = fs::read_dir(temp.path().join("diag"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let mut zip = zip::ZipArchive::new(fs::File::open(path).unwrap()).unwrap();
    for name in ["core.log", "join-timeline.txt"] {
        let mut text = String::new();
        zip.by_name(name)
            .unwrap()
            .read_to_string(&mut text)
            .unwrap();
        assert!(text.contains("StartGame"), "missing {name}");
        assert!(!text.contains("stale"));
        assert!(text.len() as u64 <= LOG_LIMIT);
    }
}

#[test]
fn archive_contains_only_redacted_members_including_repeated_names() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("bundle.zip");
    let entries = vec![("client.log".into(), r#"gamertag="Private Player" email=alice@example.invalid XUID=2535400000000000 token="secret value" target=private-host:19132 addr=2001:db8::1 url=https://play.example.invalid/path 192.168.1.2:19132"#.into()), ("core.log".into(), "Private Player connected".into())];
    bundle(&path, &entries).unwrap();
    let mut zip = zip::ZipArchive::new(fs::File::open(path).unwrap()).unwrap();
    let mut result = String::new();
    for index in 0..zip.len() {
        zip.by_index(index)
            .unwrap()
            .read_to_string(&mut result)
            .unwrap();
    }
    for secret in [
        "Private Player",
        "alice",
        "2535400000000000",
        "secret value",
        "private-host",
        "2001:db8",
        "play.example",
        "192.168",
    ] {
        assert!(!result.contains(secret), "leaked {secret}: {result}");
    }
    assert!(result.contains("connected"));
}
#[test]
fn newest_session_excludes_previous_launches() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("core.log");
    let recent_core_session = |core: &str, client: &str| {
        fs::write(&path, core).unwrap();
        core_session_tail(&path, client).unwrap()
    };
    let text = "old\nCLIENT_SESSION_START timestamp_ms=1\nfirst\nCLIENT_SESSION_START timestamp_ms=2\nlast\n";
    assert_eq!(
        section::latest(text, section::CLIENT_SESSION_START),
        "CLIENT_SESSION_START timestamp_ms=2\nlast\n"
    );
    assert_eq!(
        recent_core_session(
            "CORE_SESSION_START timestamp_ms=1\nold\nCORE_SESSION_START timestamp_ms=3\nnew",
            section::latest(text, section::CLIENT_SESSION_START)
        ),
        "CORE_SESSION_START timestamp_ms=3\nnew"
    );
    assert_eq!(
        recent_core_session(
            "CORE_SESSION_START timestamp_ms=1\nold",
            "CLIENT_SESSION_START timestamp_ms=2\ncurrent"
        ),
        ""
    );
    assert_eq!(
        recent_core_session(
            "CORE_SESSION_START timestamp_ms=1\nold",
            "legacy client log"
        ),
        "CORE_SESSION_START timestamp_ms=1\nold"
    );
}

fn crash(crash_dir: &Path, session_id: &str, message: &str, now_ms: u64) -> PathBuf {
    let identity = crash_report::report::Identity {
        session_id: session_id.into(),
        started_at_ms: now_ms - 1_000,
        release: "1.2.3".into(),
        commit: "abc123".into(),
        build_profile: "release".into(),
    };
    let report = crash_report::report::Report::new(
        crash_report::report::Kind::Panic,
        message.into(),
        &identity,
        now_ms,
    );
    let logs = folder::Logs {
        client: format!("CLIENT_SESSION_START session={session_id}\n{message} client\n")
            .into_bytes(),
        core: format!("CORE_SESSION_START timestamp_ms=1\n{message} core\n").into_bytes(),
    };
    folder::write(crash_dir, &report, &logs, None, now_ms).unwrap()
}

fn archive_members(root: &Path) -> std::collections::BTreeMap<String, String> {
    let path = fs::read_dir(root.join("diag"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let mut zip = zip::ZipArchive::new(fs::File::open(path).unwrap()).unwrap();
    (0..zip.len())
        .map(|index| {
            let mut member = zip.by_index(index).unwrap();
            let mut text = String::new();
            member.read_to_string(&mut text).unwrap();
            (member.name().to_owned(), text)
        })
        .collect()
}

#[test]
fn bundles_only_the_crash_folders_of_the_latest_session() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir(temp.path().join("logs")).unwrap();
    fs::write(
        temp.path().join("logs/client.log"),
        "CLIENT_SESSION_START session=00000000000000aa timestamp_ms=1000\nold\nCLIENT_SESSION_START version=1 session=00000000000000bb timestamp_ms=5000000\ncurrent\n",
    )
    .unwrap();
    let crash_dir = temp.path().join("crashes");
    let previous = crash(&crash_dir, "00000000000000aa", "earlier panic", 2_000_000);
    let current = crash(&crash_dir, "00000000000000bb", "current panic", 6_000_000);
    fs::write(
        crash_dir.join("crash-1-00000000000000000000.json"),
        r#"{"message":"legacy"}"#,
    )
    .unwrap();
    run(&["--data-root".into(), temp.path().display().to_string()]).unwrap();
    let members = archive_members(temp.path());
    let name = current.file_name().unwrap().to_str().unwrap();
    let report: serde_json::Value =
        serde_json::from_str(&members[&format!("crashes/{name}/report.json")]).unwrap();
    assert_eq!(report["message"], "current panic");
    assert_eq!(report["kind"], "panic");
    assert!(members[&format!("crashes/{name}/client.log")].contains("current panic client"));
    assert!(members[&format!("crashes/{name}/core.log")].contains("current panic core"));
    let previous = previous.file_name().unwrap().to_str().unwrap();
    assert!(members.keys().all(|member| !member.contains(previous)));
    let all: String = members.values().cloned().collect();
    assert!(!all.contains("earlier panic"));
    assert!(!all.contains("legacy"));
}

#[test]
fn headers_without_a_session_id_select_crashes_by_time() {
    let temp = tempfile::tempdir().unwrap();
    let crash_dir = temp.path().join("crashes");
    let before = crash(&crash_dir, "00000000000000aa", "before", 1_000_000);
    let same_second = crash(&crash_dir, "00000000000000bb", "same second", 5_000_000);
    let after = crash(&crash_dir, "00000000000000cc", "after", 9_000_000);
    let header = "CLIENT_SESSION_START version=1 commit=abc timestamp_ms=5000400";
    assert_eq!(
        session_crashes(&crash_dir, Some(header)),
        [after.clone(), same_second.clone()]
    );
    assert_eq!(
        session_crashes(&crash_dir, None),
        [after, same_second, before]
    );
}

/// A continued session includes its rotated segments and excludes previous launches.
#[test]
fn bundle_reassembles_rotated_segments_of_the_current_session() {
    let temp = tempfile::tempdir().unwrap();
    let logs = temp.path().join("logs");
    fs::create_dir(&logs).unwrap();
    let previous = "CLIENT_SESSION_START session=old timestamp_ms=1\nprevious launch\n";
    let header = "CLIENT_SESSION_START session=current timestamp_ms=2\n";
    for (name, text) in [
        ("client-19700101-000000.log", previous.to_owned()),
        (
            "client-19700101-000000-2.log",
            format!("{header}first segment token=fixture-secret\n"),
        ),
        (
            "client-19700101-000000-10.log",
            format!("{header}second segment\n"),
        ),
        (
            "client.log",
            format!("{header}last segment fixture-secret\n"),
        ),
        ("client-notes.log", "unrelated file\n".to_owned()),
    ] {
        fs::write(logs.join(name), text).unwrap();
    }
    run(&["--data-root".into(), temp.path().display().to_string()]).unwrap();
    let members = archive_members(temp.path());
    let client = &members["client.log"];
    let first = client
        .find("first segment")
        .expect("missing rotated segment");
    let second = client
        .find("second segment")
        .expect("missing rotated segment");
    let last = client.find("last segment").unwrap();
    assert!(first < second && second < last);
    assert!(!client.contains("previous launch"));
    assert!(!client.contains("unrelated file"));
    assert!(!client.contains("fixture-secret"));
}

#[test]
fn diagnostics_audit_core_archives_follow_identity_and_rotation() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("core.log");
    for (name, id, event) in [
        (
            "core-20260101-000000.log",
            "own",
            "upstream first elapsed_ms=1",
        ),
        (
            "core-20260101-000000-2.log",
            "own",
            "upstream second elapsed_ms=2",
        ),
        (
            "core-20260101-000000-10.log",
            "own",
            "upstream third elapsed_ms=3",
        ),
        ("core.log", "other", "other session"),
    ] {
        fs::write(
            temp.path().join(name),
            format!("CORE_SESSION_START timestamp_ms=2 session={id}\n{event}\n"),
        )
        .unwrap();
    }
    let text =
        core_session_tail(&path, "CLIENT_SESSION_START timestamp_ms=2 session=own\n").unwrap();
    assert!(
        text.contains("first") && text.contains("second") && text.contains("third"),
        "{text}"
    );
    assert!(text.find("second") < text.find("third"), "{text}");
    assert!(!text.contains("other session"), "{text}");
}

#[test]
fn diagnostics_audit_large_client_logs_keep_their_archive_identity() {
    let temp = tempfile::tempdir().unwrap();
    let header = "CLIENT_SESSION_START timestamp_ms=2 session=own\n";
    fs::write(
        temp.path().join("client-20260101-000000.log"),
        format!("{header}earlier session event\n"),
    )
    .unwrap();
    fs::write(
        temp.path().join("client.log"),
        format!("{header}{}\nlatest event\n", "x".repeat(LOG_LIMIT as usize)),
    )
    .unwrap();
    let text = client_session_logs(temp.path()).unwrap();
    assert!(text.starts_with(header), "missing session identity");
    assert!(text.contains("earlier session event"));
    assert!(text.ends_with("latest event\n"));
}

/// Legacy append-only logs must keep the latest session's identity outside the retained tail.
#[test]
fn oversized_multisession_logs_recover_the_latest_header() {
    for newline in ["\n", "\r\n"] {
        let temp = tempfile::tempdir().unwrap();
        let first = "CLIENT_SESSION_START timestamp_ms=1 session=first";
        let middle = "CLIENT_SESSION_START timestamp_ms=2 session=middle";
        let latest = "CLIENT_SESSION_START timestamp_ms=3 session=latest";
        fs::write(
            temp.path().join("client.log"),
            format!("{first}{newline}{}{newline}{middle}{newline}old event{newline}{latest}{newline}{}{newline}latest event{newline}",
                "old payload".repeat(section::HEADER_BYTES), "x".repeat(LOG_LIMIT as usize)),
        ).unwrap();
        fs::write(
            temp.path().join("client-20260101-000000.log"),
            format!("{first}{newline}wrong archived event{newline}"),
        )
        .unwrap();
        fs::write(
            temp.path().join("client-20260101-000001.log"),
            format!("{latest}{newline}latest archived event{newline}"),
        )
        .unwrap();
        let text = client_session_logs(temp.path()).unwrap();
        assert_eq!(text.lines().next(), Some(latest));
        assert!(text.contains("latest archived event"));
        assert!(!text.contains("wrong archived event"));
        assert!(!text.contains("old event"));
        assert!(text.ends_with(&format!("latest event{newline}")));
    }
}

/// Diagnostic export retains the request-to-present records from both process logs.
#[test]
fn join_timeline_includes_client_and_core_diagnostic_stages() {
    let client = "unrelated menu log\njoin_profile: join milestone join_profile_id=7 stage=request_accepted elapsed_ms=0\njoin_profile: join finished join_profile_id=7 stage=first_world_presented elapsed_ms=4500";
    let core = "unrelated account log\njoin_profile_id=7 stage=authentication_ready core_elapsed_ms=200\nmsg=upstream connected elapsed_ms=900";
    let timeline = super::join_timeline(client, core);
    for stage in [
        "request_accepted",
        "first_world_presented",
        "authentication_ready",
        "upstream connected",
    ] {
        assert!(timeline.contains(stage), "missing {stage}");
    }
    assert!(!timeline.contains("unrelated"));
}
