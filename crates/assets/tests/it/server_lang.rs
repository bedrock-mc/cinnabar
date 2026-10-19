use std::sync::Arc;

use assets::{MAX_SERVER_LANG_INPUT_BYTES, ServerLangOverlay};

/// Reads a complete optional language table from the supplied bytes.
fn overlay(bytes: &[u8]) -> Option<Arc<ServerLangOverlay>> {
    ServerLangOverlay::read(bytes.len(), |target| {
        assert_eq!(target.len(), bytes.len());
        target.copy_from_slice(bytes);
        true
    })
}

#[test]
fn session_table_preserves_base_grammar_and_last_duplicate() {
    let table =
        overlay(b"\xef\xbb\xbf## ignored\n b =first\na=one\nb=last\t# note\nmalformed\n=empty\n")
            .unwrap();
    assert_eq!(table.lookup("a"), Some("one"));
    assert_eq!(table.lookup("b"), Some("last"));
    assert_eq!(table.lookup("B"), None);
    assert_eq!(table.lookup("missing"), None);
    assert!(!format!("{table:?}").contains("last"));
}

#[test]
fn rejected_input_does_not_invoke_reader_or_publish_partial_text() {
    assert!(
        ServerLangOverlay::read(MAX_SERVER_LANG_INPUT_BYTES + 1, |_| panic!(
            "reader invoked"
        ))
        .is_none()
    );
    assert!(overlay(b"a=valid\nb=\xff").is_none());
    assert!(ServerLangOverlay::read(5, |_| false).is_none());
}

#[test]
fn session_table_accepts_more_than_4096_distinct_entries() {
    let input = (0..4097)
        .map(|index| format!("key{index}=value{index}\n"))
        .collect::<String>();
    let table = overlay(input.as_bytes()).unwrap();
    for index in 0..4097 {
        assert_eq!(
            table.lookup(&format!("key{index}")),
            Some(format!("value{index}").as_str())
        );
    }
}

#[test]
fn session_table_accepts_a_257_byte_key() {
    let key = "k".repeat(257);
    let table = overlay(format!("{key}=value\nsentinel=kept").as_bytes()).unwrap();
    assert_eq!(table.lookup(&key), Some("value"));
    assert_eq!(table.lookup("sentinel"), Some("kept"));
}

#[test]
fn session_table_accepts_a_1025_byte_value() {
    let value = "v".repeat(1025);
    let table = overlay(format!("key={value}\nsentinel=kept").as_bytes()).unwrap();
    assert_eq!(table.lookup("key"), Some(value.as_str()));
    assert_eq!(table.lookup("sentinel"), Some("kept"));
}

#[test]
fn session_table_keeps_the_last_of_4097_duplicate_entries() {
    let mut input = "key=old\n".repeat(4096);
    input.push_str("key=final\n");
    let table = overlay(input.as_bytes()).unwrap();
    assert_eq!(table.lookup("key"), Some("final"));
}

#[test]
fn snapshots_share_immutable_entries() {
    let old = overlay(b"key=old").unwrap();
    let retained = Arc::clone(&old);
    let new = overlay(b"key=new").unwrap();
    drop(old);
    assert_eq!(retained.lookup("key"), Some("old"));
    assert_eq!(new.lookup("key"), Some("new"));
}
