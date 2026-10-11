//! Incremental binds rebuild only the subtrees that read a changed key.

use std::sync::Arc;

use serde_json::{Value, json};

use crate::{BindState, DataSource, EmptyLibrary, ResolvedControl, Scalar};

fn control(
    name: &str,
    kind: &str,
    properties: Value,
    children: Vec<ResolvedControl>,
) -> ResolvedControl {
    let Value::Object(properties) = properties else {
        unreachable!("control properties are an object");
    };
    ResolvedControl {
        name: name.into(),
        control_type: Some(kind.into()),
        base: None,
        unresolved_base: None,
        properties: properties
            .into_iter()
            .collect::<std::collections::BTreeMap<_, _>>()
            .into(),
        children,
        factory: None,
    }
}

/// A label showing the screen global `name`.
fn reader(control_name: &str, name: &str) -> ResolvedControl {
    control(
        control_name,
        "label",
        json!({ "text": name, "bindings": [{ "binding_name": name }] }),
        Vec::new(),
    )
}

#[test]
fn an_unread_change_leaves_a_subtree_reading_many_keys_standing() {
    // Enough distinct reads that a fixed-size summary of them would claim every key.
    const READERS: usize = 2_000;
    let wide = control(
        "wide",
        "panel",
        json!({}),
        (0..READERS)
            .map(|index| reader(&format!("reader{index}"), &format!("#value{index}")))
            .collect(),
    );
    let root = Arc::new(control(
        "root",
        "panel",
        json!({}),
        vec![wide, reader("narrow", "#other")],
    ));
    let data = |other: &str| {
        let mut data = DataSource::new();
        for index in 0..READERS {
            data.set_global(format!("#value{index}"), Scalar::Text("shown".into()));
        }
        data.set_global("#other", Scalar::Text(other.into()));
        Arc::new(data)
    };
    let mut state = BindState::new();
    // The second bind settles what the first created.
    for other in ["first", "first", "second"] {
        crate::bind_incremental(&root, &data(other), &EmptyLibrary, &mut state);
    }
    let rebuilt = state.rebuilt_paths();
    assert!(
        rebuilt.iter().any(|path| path == "/root/narrow"),
        "{rebuilt:?}"
    );
    assert!(
        !rebuilt.iter().any(|path| path.starts_with("/root/wide")),
        "{rebuilt:?}"
    );
}
