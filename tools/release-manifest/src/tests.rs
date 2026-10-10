use super::*;
use update_manifest::TrustedKeys;

const SEED: [u8; KEY_BYTES] = [3; KEY_BYTES];

fn args(list: &[&str]) -> Vec<String> {
    list.iter().map(|arg| (*arg).to_owned()).collect()
}

fn seed() -> String {
    STANDARD.encode(SEED)
}

#[test]
fn a_signed_manifest_verifies_with_the_artifact_digest() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("c.dmg");
    std::fs::write(&file, b"payload").unwrap();
    let artifact = format!("macos-arm64=https://example.test/c.dmg={}", file.display());
    let mut out = Vec::new();
    run(
        &args(&[
            "sign",
            "-version",
            "1.2.3",
            "--notes-url=https://example.test/n",
            "-artifact",
            &artifact,
        ]),
        Some(&seed()),
        &mut out,
    )
    .unwrap();
    assert_eq!(out.last(), Some(&b'\n'));
    let keys = TrustedKeys::from([("k1".to_owned(), update_manifest::public_key(&SEED).unwrap())]);
    let manifest = update_manifest::verify(&out, &keys, OffsetDateTime::now_utc()).unwrap();
    let artifact = &manifest.artifacts["macos-arm64"];
    assert_eq!(
        (manifest.version.as_str(), manifest.channel.as_str()),
        ("1.2.3", "stable")
    );
    assert_eq!(manifest.notes_url, "https://example.test/n");
    assert_eq!(artifact.size, 7);
    assert_eq!(
        artifact.sha256,
        "239f59ed55e737c77147cf55ad0c1b030b6d7ee748a7426952f9b852d5a935e5"
    );
    assert!(manifest.expires > OffsetDateTime::now_utc() + Duration::days(29));
    let verdict = update_manifest::evaluate(&manifest, "stable", "macos-arm64", "1.2.2").unwrap();
    assert!(verdict.available);
}

#[test]
fn signing_requires_a_key_and_an_artifact() {
    let sign = args(&["sign", "-version", "1.0.0"]);
    assert!(
        run(&sign, None, &mut Vec::new())
            .unwrap_err()
            .contains(KEY_ENV)
    );
    assert!(run(&sign, Some("short"), &mut Vec::new()).is_err());
    let missing = run(&sign, Some(&seed()), &mut Vec::new()).unwrap_err();
    assert!(missing.contains("-artifact"), "{missing}");
    let malformed = args(&["sign", "-artifact", "macos-arm64=https://x"]);
    assert!(run(&malformed, Some(&seed()), &mut Vec::new()).is_err());
    assert!(
        run(
            &args(&["sign", "-bogus", "1"]),
            Some(&seed()),
            &mut Vec::new()
        )
        .is_err()
    );
    assert!(run(&args(&["publish"]), None, &mut Vec::new()).is_err());
    assert!(run(&[], None, &mut Vec::new()).is_err());
}

#[test]
fn validity_accepts_hour_minute_second_lifetimes() {
    assert_eq!(parse_duration("720h").unwrap(), Duration::days(30));
    assert_eq!(
        parse_duration("1h30m15s").unwrap(),
        Duration::seconds(5_415)
    );
    for bad in ["", "0h", "h", "10", "5d", "1h-2m", "1é"] {
        assert!(parse_duration(bad).is_err(), "{bad} accepted");
    }
}

#[test]
fn keygen_prints_a_matching_key_pair() {
    let mut out = Vec::new();
    run(&args(&["keygen"]), None, &mut out).unwrap();
    let text = String::from_utf8(out).unwrap();
    let value = |prefix: &str| {
        let line = text.lines().find(|line| line.starts_with(prefix)).unwrap();
        STANDARD.decode(line.rsplit(' ').next().unwrap()).unwrap()
    };
    let (public, private) = (value("public"), value("private"));
    assert_eq!(
        update_manifest::public_key(&private).unwrap().as_slice(),
        public
    );
}
