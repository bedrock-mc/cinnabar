use serde::Deserialize;

use super::{Error, Learned, MAX_ESCAPE_DEPTH, PLACEHOLDER, Redactor, patterns, rules::RULES_JSON};

/// The Go core's embedded copy of the rule file.
const GO_COPY: &str = include_str!("../../../../core/internal/redact/rules.json");

#[derive(Deserialize)]
struct File {
    placeholder: String,
    rules: Vec<Rule>,
    vectors: Vec<Vector>,
}

#[derive(Deserialize)]
struct Rule {
    name: String,
    pattern: String,
}

#[derive(Deserialize)]
struct Vector {
    name: String,
    #[serde(default)]
    file_name: bool,
    input: String,
    output: String,
}

fn file() -> File {
    serde_json::from_str(RULES_JSON).unwrap()
}

#[test]
fn the_go_copy_matches_the_rule_file() {
    assert!(
        GO_COPY == RULES_JSON,
        "core/internal/redact/rules.json is stale; copy crates/crash-report/data/redaction-rules.json over it"
    );
}

#[test]
fn the_rule_file_compiles_with_the_crate_placeholder() {
    super::rules::compile(RULES_JSON).unwrap();
    assert_eq!(file().placeholder, PLACEHOLDER);
}

/// Rust's `\d`, `\s` and `\w` are Unicode-aware and Go's are ASCII; the file spells classes in
/// ASCII so both languages match the same text.
#[test]
fn rule_patterns_avoid_unicode_dependent_classes() {
    for rule in file().rules {
        let mut chars = rule.pattern.chars();
        while let Some(c) = chars.next() {
            if c == '\\' {
                let next = chars.next().unwrap_or_default();
                assert!(
                    !"dswDSW".contains(next),
                    "rule `{}` uses \\{next}",
                    rule.name
                );
            }
        }
    }
}

#[test]
fn conformance_vectors_pass_and_are_stable() {
    let file = file();
    assert!(!file.vectors.is_empty());
    for vector in file.vectors {
        let apply = if vector.file_name {
            super::patterns_file_name
        } else {
            patterns
        };
        assert_eq!(apply(&vector.input), vector.output, "{}", vector.name);
        assert_eq!(
            apply(&vector.output),
            vector.output,
            "{} redacts its own output again",
            vector.name
        );
        assert_eq!(
            if vector.file_name {
                Redactor::default().file_name(&vector.input).unwrap()
            } else {
                Redactor::default().text(&vector.input).unwrap()
            },
            vector.output,
            "{} through the layered redactor",
            vector.name
        );
    }
}

#[test]
fn debug_quoted_windows_paths_are_scrubbed_at_a_decoded_layer() {
    let redacted = Redactor::default()
        .text(r#"path="C:\\Users\\bob\\logs""#)
        .unwrap();
    assert!(!redacted.contains("bob"), "leaked username: {redacted}");
    assert!(redacted.contains("logs"), "{redacted}");
}

#[test]
fn backslash_paths_that_need_no_redaction_keep_their_escapes() {
    let backtrace = r"at C:\work\cinnabar\crates\bridge\src\frame.rs:44:13";
    assert_eq!(Redactor::default().text(backtrace).unwrap(), backtrace);
}

#[test]
fn excessive_escape_depth_is_an_error() {
    let mut text = "gamertag=\"Fixture Player\"".to_owned();
    for _ in 0..=MAX_ESCAPE_DEPTH {
        text = serde_json::to_string(&text).unwrap();
    }
    assert_eq!(Redactor::default().text(&text), Err(Error::EscapeDepth));
    assert_eq!(Learned::default().text(&text), Err(Error::EscapeDepth));
}

#[test]
fn learned_values_are_removed_in_every_spelling() {
    let secret = "FIXTURE_PRIVATE_TOKEN\"with\\slash";
    let name = "Escaped \"Fixture\" Player";
    let payload = serde_json::json!({"access_token": secret, "gamertag": name}).to_string();
    let escaped = serde_json::to_string(&payload).unwrap();
    let mut learned = Learned::default();
    learned.text(&format!("error={escaped}")).unwrap();
    let redactor = learned.redactor();
    let unicode: String = name
        .chars()
        .map(|c| format!("\\u{:04x}", c as u32))
        .collect();
    for text in [
        format!("unlabelled {secret} later"),
        format!("encoded {} later", serde_json::to_string(name).unwrap()),
        format!("unicode {unicode} later"),
        format!("double {}", serde_json::to_string(&escaped).unwrap()),
    ] {
        let redacted = redactor.text(&text).unwrap();
        assert!(redacted.contains("later") || redacted.starts_with("double"));
        for private in ["FIXTURE_PRIVATE_TOKEN", "Escaped", "Fixture", "u0045"] {
            assert!(!redacted.contains(private), "leaked {private}: {redacted}");
        }
    }
}

#[test]
fn authorization_credentials_are_learned_without_their_scheme() {
    let mut learned = Learned::default();
    learned
        .text("headers Authorization: Basic dXNlcjpzZWNyZXQ= accept=json")
        .unwrap();
    let redacted = learned
        .redactor()
        .text("retrying with dXNlcjpzZWNyZXQ= now")
        .unwrap();
    assert_eq!(redacted, "retrying with [redacted] now");
}

#[test]
fn learned_values_include_the_players_home_and_ansi_styled_fields() {
    let mut learned = Learned::default();
    learned.value("D:\\Profiles\\ann");
    learned
        .text("\x1b[3mgamertag\x1b[0m\x1b[2m=\x1b[0m\"Private Player\"")
        .unwrap();
    let redactor = learned.redactor();
    assert_eq!(
        redactor
            .text("logs at d:\\profiles\\ANN\\logs; Private Player joined")
            .unwrap(),
        "logs at [redacted]\\logs; [redacted] joined"
    );
}

#[test]
fn json_keeps_structure_and_replaces_private_keys_whole() {
    let mut report = serde_json::json!({
        "message": "panicked at crates/client-runtime/src/auth.rs:40:9: refresh rejected {\"token\":[\"first\",\"second\"],\"status\":401}",
        "gamertag": "Other Player",
        "nested": [{"log": "Other Player connected from 203.0.113.7:19132"}],
        "release": "0.1.0",
    });
    let mut learned = Learned::default();
    learned.json(&report);
    learned.text(report["message"].as_str().unwrap()).unwrap();
    learned.redactor().json(&mut report).unwrap();
    assert_eq!(report["gamertag"], PLACEHOLDER);
    assert_eq!(report["release"], "0.1.0");
    assert_eq!(
        report["nested"][0]["log"],
        "[redacted] connected from [redacted]"
    );
    let message = report["message"].as_str().unwrap();
    assert!(message.contains("refresh rejected") && message.contains("401"));
    for private in ["first", "second"] {
        assert!(!message.contains(private), "leaked {private}: {message}");
    }
}

#[test]
fn json_keys_that_redact_to_the_same_key_are_an_error() {
    let mut value = serde_json::json!({"203.0.113.7": 1, "203.0.113.8": 2});
    assert_eq!(Redactor::default().json(&mut value), Err(Error::MergedKeys));
}

/// Every sharing path must remove these private values while preserving useful context.
#[test]
fn diagnostics_audit_private_values_never_survive() {
    for (input, secret) in [
        ("token=fixture-token failed", "fixture-token"),
        ("Authorization: Bearer fixture-token", "fixture-token"),
        ("xuid=2535412345678901 connected", "2535412345678901"),
        ("gamertag=\"Fixture Player\" joined", "Fixture Player"),
        ("connect play.example.log:19132", "play.example.log"),
        ("connect localhost:9", "localhost"),
        ("connect 7host:19132", "7host"),
        ("connect 203.0.113.7:19132", "203.0.113.7"),
        ("connect [2001:db8::1]:19132", "2001:db8::1"),
        ("at /Users/FixturePlayer/logs", "FixturePlayer"),
        (r"at C:\Users\FixturePlayer\logs", "FixturePlayer"),
    ] {
        for output in [patterns(input), Redactor::default().text(input).unwrap()] {
            assert!(!output.contains(secret), "leaked {secret}: {output}");
        }
    }
}

/// Filename exemptions apply only to bare filenames; every explicit endpoint is private.
#[test]
fn diagnostics_audit_every_filename_exemption_rejects_ports() {
    let spec: serde_json::Value = serde_json::from_str(RULES_JSON).unwrap();
    for extension in spec["file_extensions"].as_array().unwrap() {
        let file = format!("fixture.{}", extension.as_str().unwrap());
        assert_eq!(patterns(&file), file);
        for port in [1, 9, 80, 19132, 65535] {
            assert_eq!(patterns(&format!("{file}:{port}")), PLACEHOLDER);
        }
    }
    for source in [
        "src/fixture.rs:9",
        "fixture.rs:9:2",
        r"src\fixture.go:9",
        "fixture.go:9:2",
    ] {
        assert_eq!(patterns(source), source);
    }
}

#[test]
fn module_filenames_still_remove_learned_private_values() {
    let mut learned = Learned::default();
    learned.text("gamertag=FixturePlayer").unwrap();
    assert_eq!(
        learned.redactor().file_name("libFixturePlayer.so").unwrap(),
        "lib[redacted].so"
    );
}
