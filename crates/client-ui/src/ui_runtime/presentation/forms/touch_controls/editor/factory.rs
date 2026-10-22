//! Factory control anchors follow the authored layout, including a utility row's parent.
use serde_json::Value;
use std::collections::BTreeMap;
use ui::mod_hud::Anchor;

/// Derives every control's factory anchor from the shared touch layout.
pub(super) fn anchors() -> BTreeMap<String, Anchor> {
    let layout: Value = serde_json::from_slice(super::LAYOUT).expect("touch layout parses");
    let mut anchors = BTreeMap::new();
    visit(&layout["screen"], Anchor::Center, &mut anchors);
    anchors
}
/// Matches JSON-UI anchor names exhaustively against the editor's anchor enum.
fn json_name(anchor: Anchor) -> &'static str {
    match anchor {
        Anchor::TopLeft => "top_left",
        Anchor::TopCenter => "top_middle",
        Anchor::TopRight => "top_right",
        Anchor::Left => "left_middle",
        Anchor::Center => "center",
        Anchor::Right => "right_middle",
        Anchor::BottomLeft => "bottom_left",
        Anchor::BottomCenter => "bottom_middle",
        Anchor::BottomRight => "bottom_right",
    }
}
/// Carries the nearest authored container anchor into packed button instances.
fn visit(value: &Value, inherited: Anchor, anchors: &mut BTreeMap<String, Anchor>) {
    match value {
        Value::Object(object) => {
            let anchor = object
                .get("anchor_from")
                .and_then(Value::as_str)
                .map(|name| {
                    assert_eq!(
                        object.get("anchor_to").and_then(Value::as_str),
                        Some(name),
                        "touch factory uses matching anchors"
                    );
                    Anchor::ALL
                        .iter()
                        .copied()
                        .find(|anchor| json_name(*anchor) == name)
                        .expect("known touch anchor")
                })
                .unwrap_or(inherited);
            let control = object.get("$control").and_then(Value::as_str).or_else(|| {
                object
                    .get("button_mappings")?
                    .as_array()?
                    .iter()
                    .find_map(|mapping| mapping["to_button_id"].as_str())
            });
            if let Some(name) = control.and_then(|name| name.strip_prefix("cinnabar.touch.")) {
                assert!(
                    anchors.insert(name.into(), anchor).is_none(),
                    "unique touch control"
                );
            }
            for child in object.values() {
                visit(child, anchor, anchors);
            }
        }
        Value::Array(values) => {
            for child in values {
                visit(child, inherited, anchors);
            }
        }
        _ => {}
    }
}
