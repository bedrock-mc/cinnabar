use super::{decode_text, normalize_jsonc, parse_jsonc, prepare_jsonc};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use std::cell::Cell;

thread_local! {
    pub(super) static PREPARE_CALLS: Cell<usize> = const { Cell::new(0) };
}

/// Compares serialized results and diagnostics against the original preparation path.
fn assert_matches_prepared<T: DeserializeOwned + Serialize>(bytes: &[u8]) {
    let before = prepare_jsonc(bytes).and_then(|prepared| serde_json::from_slice::<T>(&prepared));
    let after = parse_jsonc::<T>(bytes);
    let encoded = |result: Result<T, serde_json::Error>| {
        result
            .map(|value| serde_json::to_vec(&value).unwrap())
            .map_err(|error| error.to_string())
    };
    assert_eq!(encoded(after), encoded(before), "input: {bytes:?}");
}

#[test]
fn fast_and_prepared_paths_have_identical_values_and_errors() {
    for bytes in [
        br#"{"name":"example","nested":{"a":[1,true,null]}}"#.as_slice(),
        br#"{"a":1,"a":2,"nested":{"b":3,"b":4}}"#,
        br#"{"text":"\\\" // /* ,}","unicode":"\u00e9"}"#,
        "{\"text\":\"日本語 §r café\"}".as_bytes(),
        b"\xef\xbb\xbf{\"a\":1}",
        b"{\"text\":\"\xff\xc3\xa9\xc3\"}",
        br#"{/* block */"a":[1,2,],// line
        }"#,
        b"\xef\xbb\xbf{/* comment */\"text\":\"\xa7r\",}",
        br#"[-9223372036854775808,9223372036854775807,18446744073709551615]"#,
        br#"[-9223372036854775809,18446744073709551616,9007199254740993]"#,
        br#"[-0,-0.0,1.0000000000000002,2.2250738585072014e-308,5e-324,1.7976931348623157e308]"#,
        b"1e309",
        b"1e-999",
        b"{} {}",
        b"{\"a\":tr/* comment */ue}",
        b"{,}",
        b"[1,,]",
        b"{\"a\":\"open}",
        b"{/* open",
        b"\xff",
        b"",
    ] {
        assert_matches_prepared::<Value>(bytes);
        let before = prepare_jsonc(bytes)
            .and_then(|prepared| serde_json::from_slice::<serde::de::IgnoredAny>(&prepared))
            .map(|_| ())
            .map_err(|error| error.to_string());
        let after = parse_jsonc::<serde::de::IgnoredAny>(bytes)
            .map(|_| ())
            .map_err(|error| error.to_string());
        assert_eq!(after, before, "ignored input: {bytes:?}");
    }
}

#[test]
fn typed_fields_keep_duplicate_validation_and_ignored_values() {
    #[derive(Deserialize, Serialize)]
    struct Document {
        name: String,
    }
    for bytes in [
        br#"{"name":"first"}"#.as_slice(),
        br#"{"name":"first","name":"second"}"#,
        br#"{"name":"first",/* comment */"name":"second",}"#,
        b"{\"name\":\"\xff\"}",
        b"{\"name\":\"first\",\"ignored\":\"\xff\"}",
        br#"{"name":"first","ignored":1e309}"#,
        br#"{"name":false,"ignored":}"#,
    ] {
        assert_matches_prepared::<Document>(bytes);
    }
}

#[derive(Debug, PartialEq, Serialize)]
struct ByteString(Vec<u8>);

impl<'de> Deserialize<'de> for ByteString {
    /// Reads the bytes of a JSON string, including legacy text after decoding.
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = ByteString;

            /// Describes the JSON value accepted by this visitor.
            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a byte string")
            }

            /// Copies the decoded JSON string bytes for comparison.
            fn visit_bytes<E: serde::de::Error>(self, value: &[u8]) -> Result<ByteString, E> {
                Ok(ByteString(value.to_vec()))
            }
        }
        deserializer.deserialize_bytes(Visitor)
    }
}

#[test]
fn byte_strings_still_decode_invalid_utf8_as_legacy_text() {
    let bytes = b"\"\xff\"";
    assert_eq!(
        serde_json::from_slice::<ByteString>(bytes).unwrap().0,
        [0xff]
    );
    assert_matches_prepared::<ByteString>(bytes);
    assert_eq!(parse_jsonc::<ByteString>(bytes).unwrap().0, "ÿ".as_bytes());
}

#[test]
fn strict_json_skips_preparation() {
    PREPARE_CALLS.set(0);
    for bytes in [
        br#"{"name":"example","text":"// /* ,}","name":"last"}"#.as_slice(),
        br#"[null,true,18446744073709551615,1.0000000000000002,-0.0]"#,
        "\"日本語 §r café\"".as_bytes(),
    ] {
        parse_jsonc::<Value>(bytes).unwrap();
    }
    assert_eq!(PREPARE_CALLS.get(), 0);
    parse_jsonc::<Value>(b"{/* comment */\"a\":[1,],}").unwrap();
    assert_eq!(PREPARE_CALLS.get(), 1);
}

#[test]
fn latin1_manifest_name_keeps_formatting_codes() {
    let value: Value = parse_jsonc(b"{\"header\":{\"name\":\"\xa7r\xa74Example\"}}").unwrap();
    assert_eq!(value["header"]["name"], "§r§4Example");
}

#[test]
fn valid_utf8_survives_beside_legacy_bytes() {
    let mut bytes = "{\"name\":\"日本語 ".as_bytes().to_vec();
    bytes.extend_from_slice(b"\xa7r caf\xe9\"}");
    let value: Value = parse_jsonc(&bytes).unwrap();
    assert_eq!(value["name"], "日本語 §r café");
    assert_eq!(decode_text("§r日本語".as_bytes()), "§r日本語");
}

#[test]
fn material_accepts_both_comment_styles_and_preserves_string_tokens() {
    let value: Value = parse_jsonc(
        br#"{
        // Material declaration
        "materials": { /* Inline note */ "example:ui": {"path":"x//y/*z*/"} }
    }"#,
    )
    .unwrap();
    assert_eq!(value["materials"]["example:ui"]["path"], "x//y/*z*/");
}

#[test]
fn trailing_commas_after_values_are_accepted() {
    let value: Value = parse_jsonc(br#"{"a":[1,2, /* tail */],}"#).unwrap();
    assert_eq!(value, json!({"a":[1,2]}));
    for text in [b"{,}".as_slice(), b"[,]", b"[1,,]", br#"{"a":,}"#] {
        assert!(normalize_jsonc(text).is_none());
    }
}

#[test]
fn bom_is_accepted_and_escaped_quotes_keep_comments_inside_strings() {
    let value: Value = parse_jsonc(b"\xef\xbb\xbf{\"text\":\"\\\" // x,}\"}").unwrap();
    assert_eq!(value["text"], "\" // x,}");
}

#[test]
fn malformed_syntax_is_rejected_and_comments_separate_tokens() {
    for text in [
        b"{} {}".as_slice(),
        b"{\"a\":1 /* open",
        b"{\"a\":\"open}",
        b"{\"a\":tr/* comment */ue}",
        b"{\"a\":1/* comment */2}",
        b"\xff",
    ] {
        assert!(normalize_jsonc(text).is_none());
    }
    let error = parse_jsonc::<Value>(b"{/* first\nsecond */\n\"a\":}\n").unwrap_err();
    assert_eq!(error.line(), 3);
}
