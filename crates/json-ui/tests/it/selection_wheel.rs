//! Native radial-widget and indexed controller contracts, without a carrier.

use std::{collections::BTreeMap, sync::Arc};

use json_ui::{
    BindState, DataSource, EmptyLibrary, HitKind, LayoutEnv, ResolvedControl, Scalar, TextMeasure,
    TextureMeta, TextureSource, ViewState, bind, bind_stateful, emit, emit_gated, hit_regions,
    layout,
};
use serde_json::{Value, json};

fn control(
    name: &str,
    kind: &str,
    props: Value,
    children: Vec<ResolvedControl>,
) -> ResolvedControl {
    ResolvedControl {
        name: name.into(),
        control_type: Some(kind.into()),
        base: None,
        unresolved_base: None,
        properties: props
            .as_object()
            .unwrap()
            .iter()
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect::<BTreeMap<_, _>>()
            .into(),
        children,
        factory: None,
    }
}

#[test]
fn repeated_controls_bind_their_own_index_and_refresh_independently() {
    let labels = (0..4)
        .map(|index| {
            control(
                "repeated_label",
                "label",
                json!({
                    "property_bag": {"#index": index}, "text": "#name",
                    "bindings": [{"binding_name":"#name"},
                        {"binding_name":"(not #empty)","binding_name_override":"#visible"}]
                }),
                vec![],
            )
        })
        .collect();
    let root = Arc::new(control("panel", "panel", json!({}), labels));
    let mut data = DataSource::new();
    data.set_global("#name", Scalar::Text("Fallback".into()));
    data.set_global("#empty", Scalar::Bool(false));
    data.set_indexed_global(0, "#name", Scalar::Text("First".into()));
    data.set_indexed_global(2, "#name", Scalar::Text("Third".into()));
    data.set_indexed_global(2, "#empty", Scalar::Bool(true));
    let mut state = BindState::new();
    let (bound, notes) = bind_stateful(&root, &data, &EmptyLibrary, &mut state);
    assert!(notes.is_empty());
    assert_eq!(
        bound
            .children
            .iter()
            .map(|child| child.properties.get("text").unwrap())
            .collect::<Vec<_>>(),
        [
            &json!("First"),
            &json!("Fallback"),
            &json!("Third"),
            &json!("Fallback")
        ]
    );
    assert_eq!(
        bound.children[2].properties.get("visible"),
        Some(&json!(false))
    );
    assert_eq!(
        bound.children[0].properties.get("visible"),
        Some(&json!(true))
    );

    data.set_indexed_global(2, "#name", Scalar::Text("Changed".into()));
    data.set_indexed_global(2, "#empty", Scalar::Bool(false));
    let (bound, _) = bind_stateful(&root, &data, &EmptyLibrary, &mut state);
    assert_eq!(
        bound.children[2].properties.get("text"),
        Some(&json!("Changed"))
    );
    assert_eq!(
        bound.children[2].properties.get("visible"),
        Some(&json!(true))
    );
    assert_eq!(
        bound.children[0].properties.get("text"),
        Some(&json!("First"))
    );
}

#[test]
fn own_views_read_indexed_answers_and_invalid_indices_use_globals() {
    let mut data = DataSource::new();
    data.set_global("#name", Scalar::Text("Screen".into()));
    data.set_indexed_global(0, "#name", Scalar::Text("Indexed".into()));
    for (index, expected) in [
        (json!(0), "Indexed"),
        (json!(-1), "Screen"),
        (json!(0.5), "Screen"),
        (json!(u64::MAX), "Screen"),
        (json!(null), "Screen"),
    ] {
        let label = control(
            "label",
            "label",
            json!({
                "property_bag": {"#index": index}, "text":"#name",
                "bindings":[{"binding_type":"view","source_property_name":"#name",
                    "target_property_name":"#name"}]
            }),
            vec![],
        );
        assert_eq!(
            bind(&label, &data, &EmptyLibrary).properties.get("text"),
            Some(&json!(expected))
        );
    }
}

struct Measures;
impl TextMeasure for Measures {
    fn extent(&self, _: &str) -> [f64; 2] {
        [0.0, 0.0]
    }
}
impl TextureSource for Measures {
    fn texture(&self, _: &str) -> Option<TextureMeta> {
        Some(TextureMeta::plain([16.0, 16.0]))
    }
}
fn env() -> LayoutEnv<'static> {
    LayoutEnv {
        text: &Measures,
        textures: &Measures,
    }
}

fn wheel() -> ResolvedControl {
    let names = ["base", "top", "right", "bottom", "left"];
    control(
        "wheel",
        "selection_wheel",
        json!({
            "size":[200,100], "inner_radius":0.35, "outer_radius":1.0, "slice_count":4,
            "property_bag":{"#hover_slice":0}, "focus_enabled":true,
            "select_button_name":"button.choose", "hover_button_name":"button.over",
            "analog_button_name":"button.analog",
            "button_mappings":[{"from_button_id":"button.menu_select",
                "to_button_id":"button.choose","mapping_type":"pressed"}],
            "state_controls":names.map(|name| json!({"control_name":name}))
        }),
        names
            .map(|name| {
                control(
                    name,
                    "image",
                    json!({
                        "size":["100%","100%"], "texture":name, "visible":false
                    }),
                    vec![],
                )
            })
            .into(),
    )
}

#[test]
fn radial_hit_uses_the_inscribed_circle_and_native_sector_order() {
    let bound = bind(&wheel(), &DataSource::new(), &EmptyLibrary);
    let laid = layout(&bound, [200.0, 100.0], &env());
    let hits = hit_regions(&laid);
    let hit = hits
        .iter()
        .find(|hit| hit.kind == HitKind::SelectionWheel)
        .unwrap();
    assert!(hit.takes_focus());
    assert_eq!(hit.pressed.as_deref(), Some("button.choose"));
    let meta = hit.widget.selection_wheel.as_ref().unwrap();
    assert_eq!(meta.hover_button.as_deref(), Some("button.over"));
    assert_eq!(meta.hovered_slice, None);
    for (point, index) in [
        ([100.0, 0.0], 0),
        ([150.0, 50.0], 1),
        ([100.0, 100.0], 2),
        ([50.0, 50.0], 3),
    ] {
        assert_eq!(
            meta.slice_at_rect([0.0, 0.0, 200.0, 100.0], point),
            Some(index)
        );
    }
    assert_eq!(meta.slice_at([0.0, -0.35]), None);
    assert_eq!(meta.slice_at([0.0, -0.3501]), Some(0));
    assert_eq!(meta.slice_at([0.0, 0.0]), None);
    assert_eq!(meta.slice_at([f64::NAN, 0.0]), None);
    assert_eq!(
        meta.slice_at_rect([0.0, 0.0, 200.0, 100.0], [175.0, 50.0]),
        None
    );
    assert_eq!(meta.slice_at_rect([0.0, 0.0, 0.0, 100.0], [0.0, 0.0]), None);
    let mut six = meta.clone();
    six.slice_count = 6;
    for index in 0..six.slice_count {
        let angle = std::f64::consts::TAU * index as f64 / six.slice_count as f64;
        assert_eq!(
            six.slice_at([angle.sin() * 0.8, -angle.cos() * 0.8]),
            Some(index)
        );
    }
}

#[test]
fn only_the_selected_state_draws_in_normal_and_cached_emission() {
    let root = Arc::new(wheel());
    let mut data = DataSource::new();
    let mut state = BindState::new();
    for (slice, shown) in [
        (-1, "base"),
        (0, "top"),
        (1, "right"),
        (2, "bottom"),
        (3, "left"),
        (-1, "base"),
    ] {
        data.set_control_value("wheel", "#hover_slice", Scalar::Int(slice));
        let (bound, notes) = bind_stateful(&root, &data, &EmptyLibrary, &mut state);
        assert!(notes.is_empty());
        let laid = layout(&bound, [200.0, 100.0], &env());
        let drawn = emit(&laid, &env());
        assert_eq!(
            drawn
                .iter()
                .map(|node| node.name.as_str())
                .collect::<Vec<_>>(),
            [shown]
        );
        let drawn = emit_gated(&laid, &env());
        assert_eq!(
            drawn
                .iter()
                .filter(|node| node.shown(&ViewState::default()))
                .map(|node| node.name.as_str())
                .collect::<Vec<_>>(),
            [shown]
        );
    }
}
