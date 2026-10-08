//! Dirty subtree layouts must match a fresh layout after every update.

use std::{cell::Cell, collections::BTreeMap};

use json_ui::{
    LayoutEnv, MeasureCache, ResolvedControl, TextMeasure, TextureMeta, TextureSource, ViewState,
    render_bound, render_bound_cached,
};
use serde_json::json;

#[derive(Default)]
struct Text(Cell<usize>);

impl TextMeasure for Text {
    fn extent(&self, text: &str) -> [f64; 2] {
        self.0.set(self.0.get() + 1);
        [text.len() as f64 * 6.0, 9.0]
    }
}

struct Textures;

impl TextureSource for Textures {
    fn texture(&self, _: &str) -> Option<TextureMeta> {
        None
    }
}

/// A vertical stack with one changing label and one independent panel.
fn tree(text: &str, count: usize) -> ResolvedControl {
    let label = |name: &str, text: &str| ResolvedControl {
        name: name.into(),
        control_type: Some("label".into()),
        properties: BTreeMap::from([
            ("text".into(), json!(text)),
            ("size".into(), json!(["default", "default"])),
        ])
        .into(),
        children: Vec::new(),
        base: None,
        unresolved_base: None,
        factory: None,
    };
    let mut root = label("root", "");
    root.control_type = Some("stack_panel".into());
    root.properties = BTreeMap::from([
        ("orientation".into(), json!("vertical")),
        ("size".into(), json!(["100%cm", "100%c"])),
    ])
    .into();
    root.children = (0..count)
        .map(|index| label(&index.to_string(), if index == 0 { text } else { "fixed" }))
        .collect();
    root
}

#[test]
fn changed_labels_and_collection_shapes_match_cold_layouts() {
    let text = Text::default();
    let env = LayoutEnv {
        text: &text,
        textures: &Textures,
    };
    let mut cache = MeasureCache::default();
    let state = ViewState::default();
    let mut rendered =
        render_bound_cached(tree("one", 3), [480.0, 270.0], &env, &state, &mut cache);
    for (label, count) in [
        ("much longer text", 3),
        ("short", 2),
        ("more", 5),
        ("last", 5),
    ] {
        let next = tree(label, count);
        cache.update_tree(&mut rendered.bound, next.clone());
        rendered = render_bound_cached(rendered.bound, [480.0, 270.0], &env, &state, &mut cache);
        let cold = render_bound(next, [480.0, 270.0], &env, &state);
        assert_eq!(rendered.nodes, cold.nodes);
        assert_eq!(rendered.hits, cold.hits);
        assert_eq!(rendered.report, cold.report);
    }
}

#[test]
fn unchanged_tree_keeps_label_measurements() {
    let text = Text::default();
    let env = LayoutEnv {
        text: &text,
        textures: &Textures,
    };
    let mut cache = MeasureCache::default();
    let state = ViewState::default();
    let mut rendered =
        render_bound_cached(tree("one", 3), [480.0, 270.0], &env, &state, &mut cache);
    text.0.set(0);
    cache.update_tree(&mut rendered.bound, tree("one", 3));
    render_bound_cached(rendered.bound, [480.0, 270.0], &env, &state, &mut cache);
    assert_eq!(text.0.get(), 0);
}

/// A changed visual target or lock state must invalidate cached state masks.
#[test]
fn changed_widget_targets_match_fresh_gated_layouts() {
    let text = Text::default();
    let env = LayoutEnv {
        text: &text,
        textures: &Textures,
    };
    let state = ViewState::default();
    let mut next = tree("default", 2);
    next.control_type = Some("button".into());
    next.properties.clear();
    next.properties.insert("size".into(), json!([100, 40]));
    next.properties.insert("default_control".into(), json!("0"));
    next.properties.insert("hover_control".into(), json!("1"));
    let mut cache = MeasureCache::default();
    let mut rendered =
        json_ui::render_bound_gated(next.clone(), [480.0, 270.0], &env, &state, &mut cache);
    for (default, enabled) in [("1", true), ("0", false), ("0", true)] {
        next.properties
            .insert("default_control".into(), json!(default));
        next.properties.insert("enabled".into(), json!(enabled));
        cache.update_tree(&mut rendered.bound, next.clone());
        rendered =
            json_ui::render_bound_gated(rendered.bound, [480.0, 270.0], &env, &state, &mut cache);
        let cold = json_ui::render_bound_gated(
            next.clone(),
            [480.0, 270.0],
            &env,
            &state,
            &mut MeasureCache::default(),
        );
        assert_eq!(rendered.nodes, cold.nodes);
        assert_eq!(rendered.hits, cold.hits);
    }
}

/// Changed bound styles and replacement children must not retain old placement values.
#[test]
fn changing_placement_properties_match_fresh_draws_and_input() {
    let text = Text::default();
    let env = LayoutEnv {
        text: &text,
        textures: &Textures,
    };
    let state = ViewState::default();
    let mut next = tree("label", 3);
    next.properties.insert("size".into(), json!([80, 40]));
    next.properties
        .insert("clip_state_change_event".into(), json!("clip.changed"));
    next.children[0].control_type = Some("button".into());
    let mut cache = MeasureCache::default();
    let mut rendered = render_bound_cached(next.clone(), [480.0, 270.0], &env, &state, &mut cache);
    for (key, value) in [
        ("visible", json!(false)),
        ("visible", json!("false")),
        ("visible", json!("true")),
        ("alpha", json!(0.25)),
        ("layer", json!(7)),
        ("clips_children", json!(true)),
        ("clip_offset", json!([3, 5])),
        ("allow_clipping", json!(false)),
        ("allow_clipping", json!(true)),
        ("enabled", json!("false")),
        ("#enabled", json!(true)),
        ("propagate_alpha", json!(true)),
    ] {
        next.properties.insert(key.into(), value);
        next.children[0]
            .properties
            .insert("alpha".into(), json!(0.5));
        cache.update_tree(&mut rendered.bound, next.clone());
        rendered = render_bound_cached(rendered.bound, [480.0, 270.0], &env, &state, &mut cache);
        let cold = render_bound(next.clone(), [480.0, 270.0], &env, &state);
        assert_eq!(rendered.nodes, cold.nodes, "draws after {key}");
        assert_eq!(rendered.hits, cold.hits, "input after {key}");
        assert_eq!(
            rendered.report, cold.report,
            "scroll/clip feedback after {key}"
        );
    }
}
