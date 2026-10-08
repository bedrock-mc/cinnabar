use std::sync::Arc;

use assets::{MAX_SERVER_LANG_INPUT_BYTES, ServerLangOverlay};

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
    assert!(overlay(format!("a={}", "x".repeat(1025)).as_bytes()).is_none());
    assert!(overlay(format!("{}=v", "x".repeat(257)).as_bytes()).is_none());
    assert!(overlay("a=v\n".repeat(4097).as_bytes()).is_none());
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
