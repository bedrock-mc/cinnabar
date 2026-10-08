use serde_json::{Value, json};

use super::*;

fn label(properties: Value) -> ResolvedControl {
    ResolvedControl {
        name: "label".into(),
        control_type: Some("label".into()),
        properties: properties
            .as_object()
            .unwrap()
            .iter()
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect(),
        base: None,
        unresolved_base: None,
        children: Vec::new(),
        factory: None,
    }
}

// 1.26.50's font-size table is 0.5 / 1 / 2 / 4; `medium` is not a size.
#[test]
fn font_sizes_follow_the_client_table() {
    let scale = |size: &str| font_scale(&label(json!({ "font_size": size })));
    assert_eq!(
        ["small", "normal", "large", "extra_large", "medium"].map(scale),
        [0.5, 1.0, 2.0, 4.0, 1.0]
    );
    let scaled = label(json!({ "font_size": "large", "font_scale_factor": 1.5 }));
    assert_eq!(font_scale(&scaled), 3.0);
}

// A disabled label draws `locked_color` at `locked_alpha`.
#[test]
fn disabled_labels_use_locked_styling() {
    let control = label(json!({
        "color": [0, 1, 0],
        "locked_color": [1, 0, 0],
        "locked_alpha": 0.25
    }));
    assert_eq!(color(&control, true), [0, 255, 0, 255]);
    assert_eq!(color(&control, false), [255, 0, 0, 64]);
    let plain = label(json!({ "color": [0, 1, 0] }));
    assert_eq!(color(&plain, false), [0, 255, 0, 255]);
}

// A bound payload beginning with `#` is literal text with a real extent.
#[test]
fn hash_prefixed_payload_is_literal() {
    assert_eq!(text(&label(json!({ "text": "#abc" }))), "#abc");
}

// A `label_cycler` draws its first `text_labels` entry.
#[test]
fn label_cycler_shows_its_first_label() {
    let mut cycler = label(json!({ "text_labels": ["A", "B"] }));
    cycler.control_type = Some("label_cycler".into());
    assert!(is_label(&cycler));
    assert_eq!(text(&cycler), "A");
}

#[test]
fn options_carry_padding_hyphen_and_fonts() {
    let control = label(json!({
        "line_padding": 4,
        "hide_hyphen": true,
        "font_type": "MinecraftTen",
        "backup_font_type": "UIFont",
        "enable_profanity_filter": true
    }));
    let options = options(&control);
    assert_eq!(options.line_padding, 4.0);
    assert!(options.hide_hyphen && options.profanity_filter);
    assert_eq!(options.font_type.as_deref(), Some("MinecraftTen"));
    assert_eq!(options.backup_font_type.as_deref(), Some("UIFont"));
}
