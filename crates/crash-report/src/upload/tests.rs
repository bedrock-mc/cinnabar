use std::{io::Read, path::PathBuf};

use super::{
    state::{self, Consent, Outcome, Upload},
    *,
};
use crate::{
    folder::{self, Logs},
    report::{Identity, Thread},
    stack::{Frame, Module, Trust},
};

const NOW_MS: u64 = 1_760_443_200_000;
const INSTALL: &str = "a1b2c3d4e5f60718293a4b5c6d7e8f90";
const REPORT: &str = "fedcba9876543210fedcba9876543210";
const IDS: Ids<'static> = Ids {
    install: INSTALL,
    report: REPORT,
};
/// Private values a crashing session can log; none may reach a payload.
const PRIVATE: [&str; 8] = [
    "2535412345678901",
    "SomeGamertag",
    "play.example.net",
    "203.0.113.7",
    "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJ4In0.c2lnbmF0dXJl",
    "/Users/alice",
    "0123456789abcdef",
    "my comment",
];

/// Builds a synthetic report containing deliberately private fields.
fn report(kind: Kind, message: &str) -> Report {
    let identity = Identity {
        session_id: "0123456789abcdef".into(),
        started_at_ms: NOW_MS - 60_000,
        release: "0.1.25".into(),
        commit: "abc123".into(),
        build_profile: "release".into(),
    };
    let mut report = Report::new(kind, message.into(), &identity, NOW_MS);
    report.thread = Thread {
        name: Some("main".into()),
        id: "1".into(),
    };
    report.gpu = Some("Adreno (TM) 619".into());
    report.renderer = Some(serde_json::json!({"backend": "vulkan", "stage": "first_presentation"}));
    report
        .game_state
        .insert("server_address".into(), "play.example.net:19132".into());
    report.game_state.insert("phase".into(), "in_world".into());
    report.user_comment = Some("my comment".into());
    report
}

/// Logs private fixture values alongside a plain diagnostic line.
fn private_log() -> String {
    "CLIENT_SESSION_START session=0123456789abcdef\n\
     INFO signed in as gamertag=SomeGamertag xuid=2535412345678901\n\
     INFO connecting to play.example.net:19132 (203.0.113.7)\n\
     DEBUG token eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJ4In0.c2lnbmF0dXJl\n\
     WARN pack cache at /Users/alice/Library/Cinnabar\n\
     ERROR lost SomeGamertag's connection\n"
        .to_owned()
}

/// Writes the payload fixture through the production folder writer.
fn crash_folder(dir: &std::path::Path, report: &Report, log: &str) -> PathBuf {
    folder::write(
        dir,
        report,
        &Logs {
            client: log.as_bytes().to_vec(),
            core: Vec::new(),
        },
        None,
        NOW_MS,
    )
    .unwrap()
}

/// Decodes a gzip body for assertions on the transmitted content.
fn inflate(body: &[u8]) -> Payload {
    let mut json = Vec::new();
    flate2::read::GzDecoder::new(body)
        .read_to_end(&mut json)
        .unwrap();
    serde_json::from_slice(&json).unwrap()
}

#[test]
fn payloads_leak_no_account_server_or_home_identifiers() {
    let dir = tempfile::tempdir().unwrap();
    let mut report = report(
        Kind::Panic,
        "panicked at crates/client-session/src/join.rs:40:9:\nlost play.example.net for SomeGamertag",
    );
    report.backtrace = "at /Users/alice/src/main.rs\nxuid 2535412345678901".into();
    report.native = Some(serde_json::json!({"address": "0x18", "reason": "SIGSEGV 203.0.113.7"}));
    let folder = crash_folder(dir.path(), &report, &private_log());
    let payload = build(&folder, IDS, Some("/Users/alice")).unwrap();
    let text = String::from_utf8(serde_json::to_vec(&payload).unwrap()).unwrap();
    for private in PRIVATE {
        assert!(!text.contains(private), "`{private}` leaked: {text}");
    }
    assert_eq!(payload.install_id, INSTALL);
    assert_eq!(payload.report_id, REPORT);
    assert_eq!(payload.backend.as_deref(), Some("vulkan"));
    assert_eq!(payload.gpu.as_deref(), Some("Adreno (TM) 619"));
    assert_eq!(payload.report["game_state"]["phase"], "in_world");
    assert_eq!(payload.native.as_ref().unwrap()["address"], "0x18");
    assert!(payload.report.get("session_id").is_none());
    assert!(payload.report.get("user_comment").is_none());
    assert!(
        payload
            .log
            .iter()
            .any(|line| line.contains("lost [redacted]'s connection"))
    );
    assert!(
        payload
            .signature
            .starts_with("panic: crates/client-session/src/join.rs:40: lost [redacted]"),
        "{}",
        payload.signature
    );
}

#[test]
fn the_log_keeps_the_newest_lines() {
    let log: String = (0..500).map(|line| format!("INFO line {line}\n")).collect();
    let payload = payload(&report(Kind::Abort, "killed"), &log, "", None, IDS, None).unwrap();
    assert_eq!(payload.log.len(), MAX_LOG_LINES);
    assert_eq!(payload.log[0], "INFO line 300");
    assert_eq!(payload.log[MAX_LOG_LINES - 1], "INFO line 499");
}

#[test]
fn encoded_payloads_fit_the_upload_limit_by_dropping_old_log_lines() {
    // Random-looking lines barely compress, so the full log cannot fit.
    let mut seed = 0x2545_f491_4f6c_dd1d_u64;
    let log: String = (0..MAX_LOG_LINES)
        .map(|line| {
            let noise: String = (0..600)
                .map(|_| {
                    seed ^= seed << 13;
                    seed ^= seed >> 7;
                    seed ^= seed << 17;
                    char::from(b'a' + (seed % 26) as u8)
                })
                .collect();
            format!("line {line} {noise}\n")
        })
        .collect();
    let mut report = report(Kind::Panic, "panicked at crates/a/src/lib.rs:1:1:\nboom");
    report.backtrace = "frame\n".repeat(2_000);
    let full = payload(&report, &log, "", None, IDS, None).unwrap();
    let body = encode(full.clone()).unwrap();
    assert!(body.len() <= MAX_COMPRESSED_BYTES, "{} bytes", body.len());
    let sent = inflate(&body);
    assert!(
        sent.log.len() < full.log.len(),
        "the full log should not fit"
    );
    assert_eq!(sent.log.last(), full.log.last(), "the newest line survives");
    assert_eq!(sent.signature, full.signature);
}

#[test]
fn unused_modules_go_before_the_payload_is_refused() {
    let modules: Vec<Module> = (0..crate::stack::MAX_MODULES)
        .map(|index| Module {
            name: format!("module-{index}.dll"),
            debug_id: Some(format!("{index:032X}0")),
            code_id: Some(format!("{index:016X}{index:08X}")),
            in_app: index == 0,
        })
        .collect();
    let stack = Stack {
        reason: "EXCEPTION_ACCESS_VIOLATION_WRITE".into(),
        modules,
        frames: vec![Frame {
            module: Some(7),
            offset: "0x10".into(),
            trust: Trust::Context,
            function: None,
        }],
    };
    let mut payload = payload(
        &report(Kind::Native, "fault"),
        "",
        "",
        Some(stack),
        IDS,
        None,
    )
    .unwrap();
    // Random names defeat compression, so all of them cannot fit.
    let mut seed = 0x9e37_79b9_7f4a_7c15_u64;
    for (index, module) in payload
        .stack
        .as_mut()
        .unwrap()
        .modules
        .iter_mut()
        .enumerate()
    {
        let noise: String = (0..200)
            .map(|_| {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                format!("{:x}", seed % 256)
            })
            .collect();
        module.name = format!("module-{index}-{noise}");
    }
    let sent = inflate(&encode(payload).unwrap());
    let stack = sent.stack.unwrap();
    assert_eq!(
        stack.modules.len(),
        1,
        "only the module a frame names stays"
    );
    assert_eq!(stack.frames[0].module, Some(0));
    assert!(stack.modules[0].name.starts_with("module-7-"));
}

#[test]
fn malformed_ids_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let folder = crash_folder(dir.path(), &report(Kind::Abort, "killed"), "");
    for ids in [
        Ids {
            install: "XUID",
            report: REPORT,
        },
        Ids {
            install: INSTALL,
            report: "../../etc",
        },
    ] {
        assert!(build(&folder, ids, None).is_err());
    }
}

#[test]
fn folders_without_consent_are_never_due() {
    let dir = tempfile::tempdir().unwrap();
    let folder = crash_folder(dir.path(), &report(Kind::Abort, "killed"), "");
    assert!(state::due(dir.path(), u64::MAX).is_empty());
    state::consent(&folder, Consent::Auto, REPORT, NOW_MS).unwrap();
    assert_eq!(state::due(dir.path(), NOW_MS), [folder]);
}

#[test]
fn a_sent_folder_is_never_sent_again() {
    let dir = tempfile::tempdir().unwrap();
    let folder = crash_folder(dir.path(), &report(Kind::Abort, "killed"), "");
    state::consent(&folder, Consent::User, REPORT, NOW_MS).unwrap();
    state::record(&folder, &Outcome::Sent, NOW_MS).unwrap();
    assert!(state::due(dir.path(), u64::MAX).is_empty());
    let again = state::consent(&folder, Consent::User, INSTALL, NOW_MS + 1).unwrap();
    assert_eq!(
        again,
        Upload::Sent {
            report_id: REPORT.into(),
            at_ms: NOW_MS
        }
    );
    assert!(state::record(&folder, &Outcome::Sent, NOW_MS).is_err());
}

#[test]
fn failures_back_off_then_abandon() {
    let dir = tempfile::tempdir().unwrap();
    let folder = crash_folder(dir.path(), &report(Kind::Abort, "killed"), "");
    state::consent(&folder, Consent::Auto, REPORT, NOW_MS).unwrap();
    let mut now = NOW_MS;
    for backoff in state::BACKOFF_MS {
        let Upload::Pending { next_at_ms, .. } =
            state::record(&folder, &Outcome::Retry { after_ms: None }, now).unwrap()
        else {
            panic!("still pending");
        };
        assert_eq!(next_at_ms, now + backoff);
        assert!(state::due(dir.path(), next_at_ms - 1).is_empty());
        assert_eq!(state::next_due(dir.path()), Some(next_at_ms));
        now = next_at_ms;
    }
    assert!(matches!(
        state::record(&folder, &Outcome::Retry { after_ms: None }, now).unwrap(),
        Upload::Abandoned { .. }
    ));
    assert!(state::due(dir.path(), u64::MAX).is_empty());
    // Only the player's own Send report tries again, under the same id.
    assert!(matches!(
        state::consent(&folder, Consent::Auto, INSTALL, now).unwrap(),
        Upload::Abandoned { .. }
    ));
    let revived = state::consent(&folder, Consent::User, INSTALL, now).unwrap();
    assert!(matches!(revived, Upload::Pending { ref report_id, .. } if report_id == REPORT));
}

#[test]
fn a_requested_retry_delay_is_honoured_up_to_the_longest_backoff() {
    let dir = tempfile::tempdir().unwrap();
    let folder = crash_folder(dir.path(), &report(Kind::Abort, "killed"), "");
    state::consent(&folder, Consent::User, REPORT, NOW_MS).unwrap();
    let state = state::record(
        &folder,
        &Outcome::Retry {
            after_ms: Some(u64::MAX),
        },
        NOW_MS,
    )
    .unwrap();
    assert!(
        matches!(state, Upload::Pending { next_at_ms, .. } if next_at_ms == NOW_MS + 86_400_000)
    );
    let rejected = state::record(&folder, &Outcome::Rejected("schema".into()), NOW_MS).unwrap();
    assert!(matches!(rejected, Upload::Abandoned { .. }));
}

#[test]
fn one_sender_holds_a_folder_until_its_claim_is_dropped() {
    let dir = tempfile::tempdir().unwrap();
    let folder = crash_folder(dir.path(), &report(Kind::Abort, "killed"), "");
    let claim = state::claim(&folder, NOW_MS).unwrap().unwrap();
    assert!(state::claim(&folder, NOW_MS + 1).unwrap().is_none());
    assert!(
        state::claim(&folder, u64::MAX).unwrap().is_none(),
        "time cannot steal a live lock"
    );
    assert!(state::consent(&folder, Consent::User, REPORT, NOW_MS).is_err());
    drop(claim);
    assert!(state::claim(&folder, NOW_MS).unwrap().is_some());
}

#[test]
fn names_are_redacted_before_a_long_log_line_is_cut() {
    let log = format!("gamertag=SomeGamertag\n{}SomeGamertag\n", " ".repeat(995));
    let payload = payload(&report(Kind::Panic, "boom"), &log, "", None, IDS, None).unwrap();
    assert!(!serde_json::to_string(&payload).unwrap().contains("SomeG"));
}

#[test]
fn native_names_and_metadata_use_the_same_learned_redaction() {
    let mut report = report(Kind::Native, "SIGSEGV");
    report.gpu = Some("SomeGamertag GPU".into());
    report.native = Some(
        serde_json::json!({ "fault": { "module": "/Users/alice/SomeGamertag.dll", "offset": "0x10" } }),
    );
    let stack = Stack {
        reason: "SIGSEGV".into(),
        modules: vec![Module {
            name: "/Users/alice/SomeGamertag.dll".into(),
            debug_id: Some("aabb".into()),
            code_id: None,
            in_app: true,
        }],
        frames: vec![Frame {
            module: Some(0),
            offset: "0x1234".into(),
            trust: Trust::Context,
            function: Some("SomeGamertag_function".into()),
        }],
    };
    let payload = payload(
        &report,
        &private_log(),
        "",
        Some(stack),
        IDS,
        Some("/Users/alice"),
    )
    .unwrap();
    let text = serde_json::to_string(&payload).unwrap();
    for private in PRIVATE {
        assert!(!text.contains(private), "private native metadata leaked");
    }
}

#[test]
fn explicit_consent_upgrades_a_pending_automatic_retry_without_changing_its_id() {
    let dir = tempfile::tempdir().unwrap();
    let folder = crash_folder(dir.path(), &report(Kind::Abort, "killed"), "");
    state::consent(&folder, Consent::Auto, REPORT, NOW_MS).unwrap();
    state::record(&folder, &Outcome::Retry { after_ms: None }, NOW_MS).unwrap();
    let user = state::consent(&folder, Consent::User, INSTALL, NOW_MS + 1).unwrap();
    assert!(
        matches!(user, Upload::Pending { consent: Consent::User, ref report_id,
        attempts: 1, next_at_ms } if report_id == REPORT && next_at_ms == NOW_MS + 1)
    );
    assert_eq!(state::due(dir.path(), NOW_MS + 1), [folder]);
}

#[test]
fn a_log_that_cannot_be_fully_learned_blocks_the_whole_upload() {
    let dir = tempfile::tempdir().unwrap();
    let report = report(Kind::Panic, "SomeGamertag failed");
    let mut log = "gamertag=SomeGamertag".to_owned();
    for _ in 0..crate::redact::MAX_ESCAPE_DEPTH + 3 {
        log = serde_json::to_string(&log).unwrap();
    }
    let folder = crash_folder(dir.path(), &report, &log);
    assert_eq!(
        build(&folder, IDS, None).unwrap_err().kind(),
        std::io::ErrorKind::InvalidData
    );
}

#[test]
fn failed_learning_in_any_report_member_blocks_cross_field_leaks() {
    let mut escaped = "gamertag=Alice".to_owned();
    for _ in 0..crate::redact::MAX_ESCAPE_DEPTH + 3 {
        escaped = serde_json::to_string(&escaped).unwrap();
    }
    for field in [
        "message",
        "backtrace",
        "span",
        "core",
        "renderer",
        "native",
        "stack",
    ] {
        let mut report = report(Kind::Panic, "boom");
        let mut core = "";
        let mut stack = None;
        match field {
            "message" => report.message = escaped.clone(),
            "backtrace" => report.backtrace = escaped.clone(),
            "span" => report.span_trace = Some(escaped.clone()),
            "core" => core = &escaped,
            "renderer" => report.renderer = Some(serde_json::json!({ "opaque": escaped })),
            "native" => report.native = Some(serde_json::json!({ "reason": escaped })),
            "stack" => {
                stack = Some(crate::stack::Stack {
                    reason: escaped.clone(),
                    ..Default::default()
                })
            }
            _ => unreachable!(),
        }
        let error =
            payload(&report, "Alice disconnected", core, stack, IDS, None).expect_err(field);
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData, "{field}");
    }
}

#[test]
fn native_signal_facts_without_a_tombstone_keep_distinct_signatures() {
    let mut first = report(Kind::Native, "Fatal signal");
    first.native =
        Some(serde_json::json!({ "signal": 11, "signal_code": -6, "fault_address": 24 }));
    let first = payload(&first, "", "", None, IDS, None).unwrap();
    assert_eq!(
        first.native.as_ref().unwrap()["reason"],
        "signal 11 / code -6"
    );
    assert_eq!(first.native.as_ref().unwrap()["address"], "0x18");
    let mut second = report(Kind::Native, "Fatal signal");
    second.native = Some(serde_json::json!({ "signal": 6, "signal_code": 1, "fault_address": 24 }));
    let second = payload(&second, "", "", None, IDS, None).unwrap();
    assert_ne!(first.signature, second.signature);
}

#[test]
fn learned_names_cannot_change_fixed_report_fields_or_enum_values() {
    for user in ["me", "i", "vul"] {
        let mut report = report(Kind::Runtime, &format!("failure for {user}"));
        report.thread.name = Some(format!("worker for {user}"));
        report.renderer = Some(serde_json::json!({ "backend": user }));
        let payload = payload(
            &report,
            &format!("cache at /home/{user}/client"),
            "",
            None,
            IDS,
            None,
        )
        .unwrap();
        assert_eq!(
            payload.report["kind"],
            serde_json::to_value(report.kind).unwrap()
        );
        let message = payload.report["message"]
            .as_str()
            .expect("fixed message key changed");
        assert!(message.contains(crate::redact::PLACEHOLDER));
        assert!(!message.contains(user));
        let name = payload.report["thread"]["name"]
            .as_str()
            .expect("fixed thread name key changed");
        assert!(!name.contains(user));
        assert!(!payload.backend.as_ref().unwrap().contains(user));
    }
}

#[test]
fn redaction_expansion_keeps_nested_metadata_within_the_same_wire_bounds() {
    let mut report = report(Kind::Panic, &"m".repeat(500));
    let metadata = "i".repeat(40);
    report.gpu = Some(metadata.clone());
    report.driver = Some(metadata.clone());
    report.release = metadata.clone();
    report.thread.name = Some(metadata);
    let payload = payload(&report, "cache at /home/i/client", "", None, IDS, None).unwrap();
    let wire = serde_json::to_value(&payload).unwrap();
    for field in ["gpu", "driver", "release"] {
        assert_eq!(
            payload.report[field], wire[field],
            "nested {field} escaped its wire bound"
        );
    }
    assert_eq!(payload.report["thread"]["name"], wire["gpu"]);
    assert!(
        payload.report["message"].as_str().unwrap().len() > wire["gpu"].as_str().unwrap().len()
    );
}

#[test]
fn windows_fallback_exception_codes_survive_without_a_minidump() {
    use windows_sys::Win32::Foundation::{STATUS_ACCESS_VIOLATION, STATUS_STACK_OVERFLOW};
    let access = format!("0x{:08X}", STATUS_ACCESS_VIOLATION as u32);
    let overflow = format!("0x{:08X}", STATUS_STACK_OVERFLOW as u32);
    let mut first = report(Kind::Native, "Unhandled Windows exception");
    first.native = Some(serde_json::json!({ "exception_code": access,
        "fault": { "module": "client.exe", "offset": "0x20", "address": "0x1020" } }));
    let first = payload(&first, "", "", None, IDS, None).unwrap();
    assert_eq!(
        first.native.as_ref().unwrap()["reason"],
        format!("Windows exception {access}")
    );
    assert_eq!(first.native.as_ref().unwrap()["fault"]["offset"], "0x20");
    assert!(first.signature.contains(&access));
    let mut second = report(Kind::Native, "Unhandled Windows exception");
    second.native = Some(serde_json::json!({ "exception_code": overflow }));
    let second = payload(&second, "", "", None, IDS, None).unwrap();
    assert_ne!(first.signature, second.signature);
}
