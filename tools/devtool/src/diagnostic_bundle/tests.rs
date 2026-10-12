use super::*;

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
    let all = bundled(&[("crashes/report-0.json".into(), report.to_string())]);
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
    bundle(
        &path,
        &[("crashes/report-0.json".into(), report.to_string())],
    )
    .unwrap();
    let mut zip = zip::ZipArchive::new(fs::File::open(path).unwrap()).unwrap();
    let mut text = String::new();
    zip.by_name("crashes/report-0.json")
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
    let unlearned = redact(r#"path="C:\\Users\\bob\\logs""#, &[]).unwrap();
    assert!(!unlearned.contains("ob"), "leaked username: {unlearned}");
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
fn home_folder_usernames_keep_apostrophes() {
    for text in ["opened /Users/D'Angelo/logs", "home is /Users/D'Angelo"] {
        let redacted = redact(text, &[]).unwrap();
        assert!(!redacted.contains("Angelo"), "leaked username: {redacted}");
    }
    let quoted = redact("profile '/Users/D'Angelo/logs'", &[]).unwrap();
    assert_eq!(quoted, "profile '[redacted]/logs'");
}

#[test]
fn ipv6_addresses_before_a_prose_colon_are_redacted() {
    let redacted = redact("dial failed via 2001:db8::7: timeout", &[]).unwrap();
    assert_eq!(redacted, "dial failed via [redacted]: timeout");
}

#[test]
fn excessive_escape_depth_never_publishes_a_bundle() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("bundle.zip");
    let mut text = "gamertag=\"Fixture Player\"".to_owned();
    for _ in 0..=MAX_ESCAPE_DEPTH {
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
            ("crashes/report-0.json".into(), serde_json::json!({"message": double, "later": format!("{name} connected {secret}"), (name): "connected"}).to_string()),
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
            ("crashes/report-0.json".into(), report.to_string()),
            ("core.log".into(), "Other Player connected".into()),
        ],
    )
    .unwrap();
    let mut zip = zip::ZipArchive::new(fs::File::open(path).unwrap()).unwrap();
    let mut text = String::new();
    zip.by_name("crashes/report-0.json")
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
            ("crashes/report-0.json".into(), report.to_string()),
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
    bundle(
        &path,
        &[("crashes/report-0.json".into(), report.to_string())],
    )
    .unwrap();
    let mut zip = zip::ZipArchive::new(fs::File::open(path).unwrap()).unwrap();
    let mut text = String::new();
    zip.by_name("crashes/report-0.json")
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
        latest_session(text, "CLIENT_SESSION_START"),
        "CLIENT_SESSION_START timestamp_ms=2\nlast\n"
    );
    assert_eq!(
        recent_core_session(
            "CORE_SESSION_START timestamp_ms=1\nold\nCORE_SESSION_START timestamp_ms=3\nnew",
            latest_session(text, "CLIENT_SESSION_START")
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
