use serde_json::json;

use crate::ResolvedControl;
use crate::emit::RectOut;
use crate::input::{FocusContainer, FocusMeta, InputComponent};

/// A button whose properties may be replaced after its input metadata is read.
fn button() -> ResolvedControl {
    ResolvedControl {
        name: "button".to_owned(),
        control_type: Some("button".to_owned()),
        base: None,
        unresolved_base: None,
        properties: [(
            "button_mappings".to_owned(),
            json!([{
                "from_button_id": "button.cancel", "to_button_id": "button.old",
                "mapping_type": "global"
            }]),
        )]
        .into(),
        children: Vec::new(),
        factory: None,
    }
}

#[test]
fn changed_properties_replace_cached_flags_and_routes() {
    let mut control = button();
    let original = control.clone();
    assert!(!InputComponent::read(&control).modal);
    assert_eq!(
        InputComponent::global_target(&control, "button.cancel").as_deref(),
        Some("button.old")
    );
    control.properties.insert("modal".to_owned(), json!(true));
    control.properties.insert(
        "button_mappings".to_owned(),
        json!([{
            "from_button_id": "button.cancel", "to_button_id": "button.new",
            "mapping_type": "global"
        }]),
    );
    assert!(InputComponent::read(&control).modal);
    assert_eq!(
        InputComponent::global_target(&control, "button.cancel").as_deref(),
        Some("button.new")
    );
    assert!(!InputComponent::read(&original).modal);
    assert_eq!(
        InputComponent::global_target(&original, "button.cancel").as_deref(),
        Some("button.old")
    );
}

#[test]
fn cached_focus_rules_use_the_current_ancestor_geometry() {
    let control = button();
    let mut parent = button();
    parent
        .properties
        .insert("focus_container".to_owned(), json!(true));
    for y in [0.0, 100.0] {
        let rect = RectOut {
            x: 0.0,
            y,
            w: 20.0,
            h: 20.0,
        };
        let container = FocusContainer::read(&parent, "parent", rect).unwrap();
        let focus = FocusMeta::read(&control, &[container]).unwrap();
        assert_eq!(focus.containers.len(), 1);
        assert_eq!(focus.containers[0].rect, rect);
    }
}
