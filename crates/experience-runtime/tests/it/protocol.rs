use std::io::{Cursor, ErrorKind};
use std::path::PathBuf;

use experience_runtime::hex;
use experience_runtime::limits::MAX_FRAME_BYTES;
use experience_runtime::protocol::{
    Cause, Face, FailKind, Mining, Request, Response, Scalar, Texture, fixtures, read_frame,
    write_frame,
};
use serde::{Deserialize, Serialize};

/// The shape of the `enums` fixture, decoded with the protocol's own enum types.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Enums {
    faces: Vec<Face>,
    causes: Vec<Cause>,
    fail_kinds: Vec<FailKind>,
}

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tools/localserver/experience/testdata/protocol")
}

fn round_trip<T>(name: &str, json: &str)
where
    T: serde::Serialize + serde::de::DeserializeOwned,
{
    let message: T = serde_json::from_str(json).unwrap_or_else(|e| panic!("{name}: {e}"));
    let mut frame = Vec::new();
    write_frame(&mut frame, &message).unwrap();
    let length = u32::from_le_bytes(frame[..4].try_into().unwrap()) as usize;
    assert_eq!(length, frame.len() - 4, "{name}: length prefix");
    let mut reader = Cursor::new(frame);
    let decoded: T = read_frame(&mut reader).unwrap().expect("one frame");
    assert!(
        read_frame::<T>(&mut reader).unwrap().is_none(),
        "{name}: trailing data"
    );
    let pretty = serde_json::to_string_pretty(&decoded).unwrap() + "\n";
    assert_eq!(pretty, json, "{name}: round trip changed the message");
}

#[test]
fn frame_round_trips_every_fixture() {
    let mut saw_enums = false;
    for (name, json) in fixtures() {
        if name == "limits" {
            continue;
        }
        if name == "enums" {
            let enums: Enums = serde_json::from_str(&json).expect("enums fixture decodes");
            let pretty = serde_json::to_string_pretty(&enums).unwrap() + "\n";
            assert_eq!(pretty, json, "enums: round trip changed the fixture");
            saw_enums = true;
            continue;
        }
        if name.starts_with("request_") {
            round_trip::<Request>(name, &json);
        } else if name.starts_with("response_") {
            round_trip::<Response>(name, &json);
        } else {
            panic!("fixture {name} is neither a request nor a response");
        }
    }
    assert!(saw_enums, "fixtures() must include the enums fixture");
}

#[test]
fn oversized_frame_is_rejected() {
    let reason = "x".repeat(MAX_FRAME_BYTES);
    let mut out = Vec::new();
    let err = write_frame(&mut out, &Response::LoadFailed { reason }).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::InvalidInput);
    assert!(
        out.is_empty(),
        "nothing may be written for a rejected frame"
    );

    let mut frame = (MAX_FRAME_BYTES as u32 + 1).to_le_bytes().to_vec();
    frame.extend_from_slice(b"{}");
    let err = read_frame::<Request>(&mut Cursor::new(frame)).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::InvalidData);
}

#[test]
fn truncated_frame_is_an_error() {
    let mut frame = Vec::new();
    write_frame(&mut frame, &Request::Shutdown {}).unwrap();

    let partial_length = frame[..2].to_vec();
    let err = read_frame::<Request>(&mut Cursor::new(partial_length)).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::UnexpectedEof);

    let partial_body = frame[..frame.len() - 1].to_vec();
    let err = read_frame::<Request>(&mut Cursor::new(partial_body)).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::UnexpectedEof);
}

#[test]
fn clean_eof_is_none() {
    let empty: &[u8] = &[];
    assert!(
        read_frame::<Request>(&mut Cursor::new(empty))
            .unwrap()
            .is_none()
    );
}

#[test]
fn unknown_field_is_rejected() {
    let frame_of = |json: &str| {
        let mut frame = (json.len() as u32).to_le_bytes().to_vec();
        frame.extend_from_slice(json.as_bytes());
        frame
    };
    let known = frame_of(r#"{"type":"load","dir":"/srv/experiences/benergistics"}"#);
    assert!(
        read_frame::<Request>(&mut Cursor::new(known))
            .unwrap()
            .is_some()
    );

    let extra = frame_of(r#"{"type":"load","dir":"/srv/experiences/benergistics","extra":1}"#);
    let err = read_frame::<Request>(&mut Cursor::new(extra)).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::InvalidData);

    let nested = r#"{"slot":"*","path":"/srv/a.png","extra":true}"#;
    assert!(serde_json::from_str::<Texture>(nested).is_err());

    let shutdown = frame_of(r#"{"type":"shutdown","extra":1}"#);
    let err = read_frame::<Request>(&mut Cursor::new(shutdown)).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::InvalidData);

    let unbreakable = r#"{"type":"unbreakable","extra":1}"#;
    assert!(serde_json::from_str::<Mining>(unbreakable).is_err());

    let scalar = r#"{"type":"integer","value":1,"extra":1}"#;
    assert!(serde_json::from_str::<Scalar>(scalar).is_err());
}

/// A scalar is adjacently tagged like the client's wire `Scalar`: its value is required and must
/// fit its type.
#[test]
fn scalar_value_is_typed_and_required() {
    let decoded = |json: &str| serde_json::from_str::<Scalar>(json).ok();
    assert_eq!(
        decoded(r#"{"type":"choice","value":65535}"#),
        Some(Scalar::Choice(u16::MAX))
    );
    assert_eq!(
        decoded(r#"{"type":"integer","value":-9223372036854775808}"#),
        Some(Scalar::Integer(i64::MIN))
    );
    for bad in [
        r#"{"type":"choice","value":65536}"#,
        r#"{"type":"bool","value":1}"#,
        r#"{"type":"text"}"#,
        r#"{"type":"text","value":null}"#,
        r#"{"type":"float","value":1.5}"#,
    ] {
        assert_eq!(decoded(bad), None, "{bad} decoded");
    }
}

#[test]
fn hex_rejects_uppercase_and_odd() {
    assert_eq!(hex::encode(&[0x00, 0xab, 0x7f, 0xff]), "00ab7fff");
    assert_eq!(
        hex::decode("00ab7fff").unwrap(),
        vec![0x00, 0xab, 0x7f, 0xff]
    );
    assert_eq!(hex::decode("").unwrap(), Vec::<u8>::new());
    assert!(hex::decode("AB").is_err(), "uppercase");
    assert!(hex::decode("aB").is_err(), "mixed case");
    assert!(hex::decode("abc").is_err(), "odd length");
    assert!(hex::decode("zz").is_err(), "non-hex");
    assert!(hex::decode("é").is_err(), "non-ascii");
}

#[test]
fn checked_in_fixtures_match() {
    let dir = fixture_dir();
    let regenerate = "regenerate with `cargo run -p experience-runtime --locked -- write-fixtures tools/localserver/experience/testdata/protocol`";
    let generated = fixtures();
    for (name, json) in &generated {
        let path = dir.join(format!("{name}.json"));
        let on_disk = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("{}: {e}; {regenerate}", path.display()))
            .replace("\r\n", "\n");
        assert_eq!(&on_disk, json, "{} is stale; {regenerate}", path.display());
    }
    let mut checked_in: Vec<String> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{}: {e}; {regenerate}", dir.display()))
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    checked_in.sort();
    let mut expected: Vec<String> = generated
        .iter()
        .map(|(name, _)| format!("{name}.json"))
        .collect();
    expected.sort();
    assert_eq!(
        checked_in, expected,
        "unexpected fixture files; {regenerate}"
    );
}
