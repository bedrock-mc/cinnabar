use super::*;
use time::Duration;

const SEED: [u8; KEY_BYTES] = [7; KEY_BYTES];
const OTHER_SEED: [u8; KEY_BYTES] = [9; KEY_BYTES];

fn now() -> OffsetDateTime {
    OffsetDateTime::from_unix_timestamp(1_788_220_800).unwrap()
}

fn keys(seed: &[u8]) -> TrustedKeys {
    TrustedKeys::from([("k1".to_owned(), public_key(seed).unwrap())])
}

fn manifest() -> Manifest {
    Manifest {
        schema: SCHEMA,
        channel: "stable".to_owned(),
        version: "0.2.0".to_owned(),
        expires: now() + Duration::hours(1),
        notes_url: String::new(),
        artifacts: BTreeMap::from([(
            "macos-arm64".to_owned(),
            Artifact {
                url: "https://example.test/c.dmg".to_owned(),
                sha256: "a".repeat(64),
                size: 10,
            },
        )]),
    }
}

fn signed(manifest: &Manifest) -> Vec<u8> {
    sign(manifest, "k1", &SEED).unwrap()
}

/// Rewrites the signed payload without re-signing it.
fn tamper(body: &[u8], edit: impl FnOnce(String) -> String) -> Vec<u8> {
    let mut envelope: Envelope = serde_json::from_slice(body).unwrap();
    let payload = String::from_utf8(STANDARD.decode(&envelope.manifest).unwrap()).unwrap();
    envelope.manifest = STANDARD.encode(edit(payload));
    serde_json::to_vec(&envelope).unwrap()
}

#[test]
fn a_trusted_signature_round_trips_the_manifest() {
    let manifest = manifest();
    assert_eq!(
        verify(&signed(&manifest), &keys(&SEED), now()).unwrap(),
        manifest
    );
}

#[test]
fn a_tampered_manifest_is_rejected() {
    let body = tamper(&signed(&manifest()), |payload| {
        payload.replacen("0.2.0", "9.9.9", 1)
    });
    assert!(matches!(
        verify(&body, &keys(&SEED), now()),
        Err(Error::Signature)
    ));
}

#[test]
fn an_invalid_or_foreign_signature_is_rejected() {
    let body = signed(&manifest());
    assert!(matches!(
        verify(&body, &keys(&OTHER_SEED), now()),
        Err(Error::Signature)
    ));
    let mut envelope: Envelope = serde_json::from_slice(&body).unwrap();
    envelope.signature = STANDARD.encode([0; 64]);
    let zeroed = serde_json::to_vec(&envelope).unwrap();
    assert!(matches!(
        verify(&zeroed, &keys(&SEED), now()),
        Err(Error::Signature)
    ));
    envelope.signature = "not base64!".to_owned();
    let garbled = serde_json::to_vec(&envelope).unwrap();
    assert!(matches!(
        verify(&garbled, &keys(&SEED), now()),
        Err(Error::Signature)
    ));
}

#[test]
fn an_unknown_key_id_is_rejected() {
    let body = sign(&manifest(), "k2", &SEED).unwrap();
    assert!(matches!(
        verify(&body, &keys(&SEED), now()),
        Err(Error::UnknownKey(id)) if id == "k2"
    ));
    assert!(matches!(
        verify(&signed(&manifest()), &TrustedKeys::new(), now()),
        Err(Error::UnknownKey(_))
    ));
}

#[test]
fn expired_and_unknown_schema_manifests_are_rejected() {
    let body = signed(&manifest());
    assert!(matches!(
        verify(&body, &keys(&SEED), now() + Duration::hours(2)),
        Err(Error::Expired)
    ));
    let future = Manifest {
        schema: 2,
        ..manifest()
    };
    assert!(matches!(
        verify(&signed(&future), &keys(&SEED), now()),
        Err(Error::Schema(2))
    ));
}

#[test]
fn malformed_input_is_rejected() {
    let trusted = keys(&SEED);
    assert!(matches!(
        verify(b"not json", &trusted, now()),
        Err(Error::Envelope(_))
    ));
    let mut envelope: Envelope = serde_json::from_slice(&signed(&manifest())).unwrap();
    envelope.manifest = "%%%".to_owned();
    let body = serde_json::to_vec(&envelope).unwrap();
    assert!(matches!(
        verify(&body, &trusted, now()),
        Err(Error::ManifestEncoding)
    ));
    // A correctly signed payload that is not a manifest still fails to decode.
    let pair = key_pair(&SEED).unwrap();
    let payload = br#"{"schema":1,"expires":"yesterday"}"#;
    let body = serde_json::to_vec(&Envelope {
        key_id: "k1".to_owned(),
        signature: STANDARD.encode(pair.sign(payload)),
        manifest: STANDARD.encode(payload),
    })
    .unwrap();
    assert!(matches!(
        verify(&body, &trusted, now()),
        Err(Error::Manifest(_))
    ));
}

#[test]
fn version_order_matches_the_release_rules() {
    for (candidate, current, want) in [
        ("0.2.0", "0.1.9", true),
        ("0.1.9", "0.2.0", false),
        ("1.0.0", "1.0.0", false),
        ("1.0.0", "1.0.0-beta", true),
        ("1.0.0-beta", "1.0.0", false),
        ("v1.10.0", "1.9.9", true),
    ] {
        assert_eq!(
            newer(candidate, current).unwrap(),
            want,
            "{candidate} vs {current}"
        );
    }
    for malformed in [
        "1.0",
        "1.0.0.0",
        "1..0",
        "1.+2.0",
        "x.y.z",
        "1.0.4294967296",
    ] {
        assert!(newer(malformed, "1.0.0").is_err(), "{malformed} accepted");
    }
}

#[test]
fn evaluation_offers_only_newer_builds_for_this_platform() {
    let manifest = manifest();
    let verdict = evaluate(&manifest, "stable", "macos-arm64", "0.1.9").unwrap();
    assert!(verdict.available);
    assert_eq!(verdict.latest, "0.2.0");
    assert_eq!(verdict.artifact.unwrap().size, 10);
    for current in ["0.2.0", "0.3.0"] {
        let verdict = evaluate(&manifest, "stable", "macos-arm64", current).unwrap();
        assert!(!verdict.available && verdict.artifact.is_none());
    }
    let elsewhere = evaluate(&manifest, "stable", "linux-x86_64", "0.1.9").unwrap();
    assert!(!elsewhere.available);
    assert!(matches!(
        evaluate(&manifest, "nightly", "macos-arm64", "0.1.9"),
        Err(Error::Channel { .. })
    ));
}

#[test]
fn an_unsafe_artifact_is_never_offered() {
    let mut manifest = manifest();
    let artifact = manifest.artifacts.get_mut("macos-arm64").unwrap();
    artifact.url = "http://example.test/c.dmg".to_owned();
    assert!(matches!(
        evaluate(&manifest, "stable", "macos-arm64", "0.1.9"),
        Err(Error::ArtifactUrl(_))
    ));
    let bad = [
        Artifact {
            sha256: "g".repeat(64),
            ..manifest.artifacts["macos-arm64"].clone()
        },
        Artifact {
            size: 0,
            ..manifest.artifacts["macos-arm64"].clone()
        },
    ];
    let https = |artifact: Artifact| Artifact {
        url: "https://example.test/c.dmg".to_owned(),
        ..artifact
    };
    let [digest, size] = bad.map(https);
    assert!(matches!(
        validate_artifact(&digest),
        Err(Error::ArtifactDigest)
    ));
    assert!(matches!(validate_artifact(&size), Err(Error::ArtifactSize)));
}

#[test]
fn trusted_key_lists_parse_and_reject_bad_entries() {
    let public = public_key(&SEED).unwrap();
    let list = format!(
        " k1:{} , ,k2:{}",
        STANDARD.encode(public),
        STANDARD.encode([1; 32])
    );
    let parsed = parse_keys(&list).unwrap();
    assert_eq!(parsed["k1"], public);
    assert_eq!(parsed.len(), 2);
    assert!(parse_keys("").unwrap().is_empty());
    let short = format!("k1:{}", STANDARD.encode([1; 31]));
    for bad in ["k1:bad", "k1", ":AAAA", short.as_str()] {
        assert!(parse_keys(bad).is_err(), "{bad} accepted");
    }
}

#[test]
fn the_wire_format_keeps_its_field_names() {
    let body: serde_json::Value = serde_json::from_slice(&signed(&manifest())).unwrap();
    for field in ["key_id", "signature", "manifest"] {
        assert!(body.get(field).is_some(), "envelope lost {field}");
    }
    let payload = STANDARD.decode(body["manifest"].as_str().unwrap()).unwrap();
    let manifest: serde_json::Value = serde_json::from_slice(&payload).unwrap();
    assert_eq!(manifest["expires"], "2026-09-01T01:00:00Z");
    assert_eq!(manifest["artifacts"]["macos-arm64"]["size"], 10);
    assert!(manifest.get("notes_url").is_none());
    // Fractional and offset timestamps and null artifacts from earlier signers still parse.
    let older = br#"{"schema":1,"channel":"stable","version":"1.0.0","expires":"2026-09-01T03:00:00.5+02:00","artifacts":null}"#;
    let parsed: Manifest = serde_json::from_slice(older).unwrap();
    assert_eq!(
        parsed.expires,
        now() + Duration::hours(1) + Duration::milliseconds(500)
    );
    assert!(parsed.artifacts.is_empty());
}

#[test]
fn signing_needs_a_full_seed() {
    assert!(matches!(
        sign(&manifest(), "k1", &[]),
        Err(Error::SigningKey)
    ));
    assert!(matches!(public_key(&[1; 31]), Err(Error::SigningKey)));
    assert_ne!(generate_seed().unwrap(), generate_seed().unwrap());
}
