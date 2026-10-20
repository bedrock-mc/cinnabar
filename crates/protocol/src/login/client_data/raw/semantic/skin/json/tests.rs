use super::{MAX_NESTING_DEPTH, object_or_null};

/// The actual generated Go bound includes the root object and never enters a recursive Value tree.
#[test]
fn decoded_skin_json_preserves_the_go_nesting_boundary() {
    let object = |arrays| {
        format!(
            "{{\"value\":{}0{}}}",
            "[".repeat(arrays),
            "]".repeat(arrays),
        )
    };
    let maximum = object(MAX_NESTING_DEPTH - 1);
    assert!(object_or_null(maximum.as_bytes()));
    let too_deep = object(MAX_NESTING_DEPTH);
    assert!(!object_or_null(too_deep.as_bytes()));
}

/// Raw JSON grammar alone admits unrepresentable numbers; the consumed map must retain this refusal.
#[test]
fn decoded_skin_json_rejects_overflow_malformed_and_nonobject_values() {
    for accepted in [
        br#"{"number":1e308,"string":"1e309 [ \" ]"}"#.as_slice(),
        b"null",
    ] {
        assert!(object_or_null(accepted));
    }
    for refused in [
        br#"{"number":1e309}"#.as_slice(),
        br#"{"number":[0,]}"#,
        b"{} null",
        b"[]",
        b"0",
    ] {
        assert!(!object_or_null(refused));
    }
}
