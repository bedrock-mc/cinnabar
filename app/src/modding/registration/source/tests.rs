use super::*;
use std::sync::Arc;

#[test]
fn unchanged_registration_polls_share_sources_and_companions_but_observe_changes() {
    let root = tempfile::tempdir().unwrap();
    let entry = Entry {
        component: root.path().join("source.wasm"),
        font: Some(root.path().join("font")),
        ui: Some(root.path().join("ui")),
        enabled: true,
        grants: mod_host::ModGrants {
            controls: true,
            ..Default::default()
        },
        ..Default::default()
    };
    std::fs::write(&entry.component, b"source").unwrap();
    std::fs::write(entry.font.as_ref().unwrap(), b"font").unwrap();
    std::fs::write(entry.ui.as_ref().unwrap(), b"panel").unwrap();
    let mut observer = Observer::default();
    let first = observer.source_snapshot(entry.clone()).unwrap();
    observer.files.retain_observed();
    for _ in 0..8 {
        let again = observer.source_snapshot(entry.clone()).unwrap();
        assert_eq!(first.identity, again.identity);
        assert!(Arc::ptr_eq(&first.component, &again.component));
        assert!(Arc::ptr_eq(
            first.font.as_ref().unwrap(),
            again.font.as_ref().unwrap()
        ));
        assert!(Arc::ptr_eq(
            first.ui.as_ref().unwrap(),
            again.ui.as_ref().unwrap()
        ));
        observer.files.retain_observed();
    }
    for path in [
        &entry.component,
        entry.font.as_ref().unwrap(),
        entry.ui.as_ref().unwrap(),
    ] {
        let original = std::fs::read(path).unwrap();
        let mut changed = original.clone();
        changed[0] ^= 1;
        std::fs::write(path, changed).unwrap();
        test_time::eventually("changed registration source is observed", || {
            first.identity != observer.source_snapshot(entry.clone()).unwrap().identity
        });
        std::fs::write(path, original).unwrap();
    }
    let before_denial = observer.source_snapshot(entry.clone()).unwrap();
    let mut denied = entry.clone();
    denied.grants.controls = false;
    let denied = observer.source_snapshot(denied).unwrap();
    assert_ne!(first.identity, denied.identity);
    assert!(denied.font.is_none());
    assert!(denied.ui.is_none());
    assert!(Arc::ptr_eq(&before_denial.component, &denied.component));
    let mut requested = entry;
    requested.reload += 1;
    assert_ne!(
        first.identity,
        observer.source_snapshot(requested).unwrap().identity
    );
}
