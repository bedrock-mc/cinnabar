use super::{decode_text, normalize_jsonc, parse_jsonc};
use serde_json::{Value, json};

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
