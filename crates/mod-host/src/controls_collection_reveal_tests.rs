use super::*;
use crate::ModGrants;
use cinnabar::extension::panel::Host as _;
use serde_json::json;
use std::sync::Arc;

/// An admitted native collection with immutable items and no control authority of its own.
fn panel() -> Panel {
    serde_json::from_value(
        json!({"title":"Collection", "toggle_key":"F10", "dark":true,
        "controls":[], "surface":{"screen":"list.root", "document":json!({
            "namespace":"list", "root":{"type":"panel", "controls":[
                {"tiles":{"type":"panel", "cinnabar_collection":"modules"}}]}}).to_string(),
        "collections":{"modules":{"definition":{"columns":1,"viewport":[108,80],
            "template":{"screen":"tile.root", "document":json!({"namespace":"tile",
                "root":{"type":"panel", "size":[100,40]}}).to_string()},
            "items":vec![json!({});33]}, "order":(0..33).collect::<Vec<_>>()}}}}),
    )
    .unwrap()
}

/// Admission is transactional, uses the selected route and preserves one-time request identity.
#[test]
fn collection_reveal_host_validates_and_commits_requests_atomically() {
    let mut state = State::new(
        ModGrants {
            controls: true,
            ..Default::default()
        },
        String::new(),
        Default::default(),
    );
    state
        .set_content(serde_json::to_string(&panel()).unwrap())
        .unwrap()
        .unwrap();
    state.controls.commit();
    let request = json!({"collection_order":{"modules":[32,8]},
        "collection_reveal":{"collection":"modules", "item":8}})
    .to_string();
    state.update_content(request.clone()).unwrap().unwrap();
    assert!(
        state
            .controls
            .panel
            .as_ref()
            .unwrap()
            .surface
            .as_ref()
            .unwrap()
            .collections["modules"]
            .reveal
            .is_none()
    );
    state.controls.commit();
    let accepted = state.controls.panel.clone().unwrap();
    let first = accepted.surface.as_ref().unwrap().collections["modules"]
        .reveal
        .as_ref()
        .unwrap();
    assert_eq!(first.target.item, 8);
    for invalid in [
        json!({"collection":"missing","item":0}),
        json!({"collection":"modules","item":33}),
        json!({"collection":"modules","item":-1}),
        json!({"collection":"modules","item":1.5}),
        json!({"collection":"modules","item":u64::from(u32::MAX)+1}),
        json!({"collection":"modules"}),
        json!({"collection":"modules","item":1,"extra":true}),
    ] {
        state.controls.begin_frame();
        assert!(
            state
                .update_content(
                    json!({"collection_order":{"modules":[0]},
            "collection_reveal":invalid})
                    .to_string()
                )
                .unwrap()
                .is_err()
        );
        state.controls.commit();
        assert_eq!(state.controls.panel.as_ref(), Some(&accepted));
    }
    state.controls.begin_frame();
    state.update_content("{}".into()).unwrap().unwrap();
    state.controls.commit();
    assert_eq!(state.controls.panel.as_ref(), Some(&accepted));
    state.update_content(request).unwrap().unwrap();
    state.controls.commit();
    let repeated = state
        .controls
        .panel
        .as_ref()
        .unwrap()
        .surface
        .as_ref()
        .unwrap()
        .collections["modules"]
        .reveal
        .as_ref()
        .unwrap();
    assert_eq!(first.target, repeated.target);
    assert!(!Arc::ptr_eq(first, repeated));
    assert_ne!(state.controls.panel.as_ref(), Some(&accepted));
    state.grants.controls = false;
    assert!(
        state
            .update_content(
                json!({"collection_reveal":{"collection":"modules","item":0}}).to_string()
            )
            .unwrap()
            .is_err()
    );
}
