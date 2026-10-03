//! The shared screen harness: a synthetic tree bound over its components,
//! laid out under its view state, and driven through the dispatcher.

use std::collections::BTreeMap;

use json_ui::{
    ButtonInput, DataSource, Dispatch, Dispatcher, EmptyLibrary, HitRegion, InputMode, LaidOut,
    LayoutEnv, PointerInput, ResolvedControl, ScreenEvent, TextMeasure, TextureMeta, TextureSource,
    ViewState, bind, hit_regions, layout_with,
};
use serde_json::{Value, json};

pub(crate) struct ZeroText;
impl TextMeasure for ZeroText {
    fn extent(&self, _text: &str) -> [f64; 2] {
        [0.0, 0.0]
    }
}

pub(crate) struct NoTextures;
impl TextureSource for NoTextures {
    fn texture(&self, _path: &str) -> Option<TextureMeta> {
        None
    }
}

pub(crate) fn env() -> LayoutEnv<'static> {
    LayoutEnv {
        text: &ZeroText,
        textures: &NoTextures,
    }
}

pub(crate) fn ctrl(
    name: &str,
    kind: &str,
    props: Value,
    children: Vec<ResolvedControl>,
) -> ResolvedControl {
    let mut properties: BTreeMap<String, Value> = match props {
        Value::Object(map) => map.into_iter().collect(),
        _ => BTreeMap::new(),
    };
    properties
        .entry("anchor_from".to_owned())
        .or_insert(json!("top_left"));
    properties
        .entry("anchor_to".to_owned())
        .or_insert(json!("top_left"));
    ResolvedControl {
        name: name.to_owned(),
        control_type: Some(kind.to_owned()),
        base: None,
        unresolved_base: None,
        properties: properties.into(),
        children,
        factory: None,
    }
}

pub(crate) fn page(children: Vec<ResolvedControl>) -> ResolvedControl {
    ctrl("root", "panel", json!({ "size": [200, 200] }), children)
}

pub(crate) fn find<'a>(node: &'a LaidOut<'a>, name: &str) -> &'a LaidOut<'a> {
    fn walk<'a>(node: &'a LaidOut<'a>, name: &str) -> Option<&'a LaidOut<'a>> {
        if node.control.name == name {
            return Some(node);
        }
        node.children.iter().find_map(|child| walk(child, name))
    }
    walk(node, name).unwrap_or_else(|| panic!("missing {name}"))
}

/// A screen bound over `components`, laid out under `view`, and its regions.
pub(crate) struct Screen {
    pub(crate) root: ResolvedControl,
    pub(crate) view: ViewState,
    pub(crate) dispatcher: Dispatcher,
}

impl Screen {
    pub(crate) fn new(root: ResolvedControl) -> Self {
        Screen {
            root,
            view: ViewState::default(),
            dispatcher: Dispatcher::default(),
        }
    }

    pub(crate) fn bound(&self) -> ResolvedControl {
        let mut data = DataSource::new();
        data.set_components(self.view.components.clone());
        bind(&self.root, &data, &EmptyLibrary)
    }

    pub(crate) fn regions(&self) -> Vec<HitRegion> {
        let bound = self.bound();
        let (laid, _) = layout_with(&bound, [200.0, 200.0], &env(), &self.view);
        hit_regions(&laid)
    }

    pub(crate) fn visible(&self, name: &str) -> bool {
        let bound = self.bound();
        let (laid, _) = layout_with(&bound, [200.0, 200.0], &env(), &self.view);
        find(&laid, name).visible
    }

    pub(crate) fn hover(&mut self, point: [f64; 2]) -> Dispatch {
        let regions = self.regions();
        let input = PointerInput {
            point: Some(point),
            held: false,
            mode: InputMode::Mouse,
            now: 0.0,
        };
        self.dispatcher.pointer(&regions, &mut self.view, input)
    }

    pub(crate) fn press(&mut self, id: &str, down: bool, point: [f64; 2], now: f64) -> Dispatch {
        self.press_in(id, down, point, now, InputMode::Mouse)
    }

    pub(crate) fn press_in(
        &mut self,
        id: &str,
        down: bool,
        point: [f64; 2],
        now: f64,
        mode: InputMode,
    ) -> Dispatch {
        let regions = self.regions();
        let input = ButtonInput {
            id,
            down,
            point: Some(point),
            mode,
            now,
        };
        self.dispatcher.button(&regions, &mut self.view, input)
    }

    /// Hover `point` then press and release `button.menu_select` there.
    pub(crate) fn click(&mut self, point: [f64; 2]) -> Vec<ScreenEvent> {
        self.hover(point);
        let mut events = self.press("button.menu_select", true, point, 0.0).events;
        events.extend(self.press("button.menu_select", false, point, 0.05).events);
        events
    }
}

/// Targets raised by hover mappings, which arrive in the Up state rather than as presses.
pub(crate) fn hover_ids(events: &[ScreenEvent]) -> Vec<String> {
    events
        .iter()
        .filter_map(|event| match event {
            ScreenEvent::Button(button) if !button.down && !button.interacted => {
                Some(button.id.clone())
            }
            _ => None,
        })
        .collect()
}

pub(crate) fn button_ids(events: &[ScreenEvent]) -> Vec<String> {
    events
        .iter()
        .filter_map(|event| match event {
            ScreenEvent::Button(button) if button.down => Some(button.id.clone()),
            _ => None,
        })
        .collect()
}

pub(crate) fn mapped(mappings: Value) -> ResolvedControl {
    ctrl(
        "b",
        "button",
        json!({ "size": [40, 20], "button_mappings": mappings }),
        vec![],
    )
}

pub(crate) fn toggle(name: &str, props: Value) -> ResolvedControl {
    let mut base = json!({
        "size": [20, 20], "toggle_name": "group",
        "checked_control": "on", "unchecked_control": "off",
        "button_mappings": [
            { "from_button_id": "button.menu_select", "to_button_id": "button.menu_select", "mapping_type": "pressed" }
        ]
    });
    if let (Value::Object(base), Value::Object(extra)) = (&mut base, props) {
        base.extend(extra);
    }
    ctrl(
        name,
        "toggle",
        base,
        vec![
            ctrl(&format!("{name}_on"), "panel", json!({}), vec![]),
            ctrl(&format!("{name}_off"), "panel", json!({}), vec![]),
        ],
    )
}

pub(crate) fn toggle_child_names(control: &mut ResolvedControl) {
    let name = control.name.clone();
    control
        .properties
        .insert("checked_control".into(), json!(format!("{name}_on")));
    control
        .properties
        .insert("unchecked_control".into(), json!(format!("{name}_off")));
}

pub(crate) fn slider(props: Value, children: Vec<ResolvedControl>) -> ResolvedControl {
    let mut base = json!({
        "size": [100, 20], "slider_name": "value", "slider_track_button": "button.slider_track",
        "button_mappings": [
            { "from_button_id": "button.menu_select", "to_button_id": "button.slider_track", "mapping_type": "pressed", "button_up_right_of_first_refusal": true }
        ]
    });
    if let (Value::Object(base), Value::Object(extra)) = (&mut base, props) {
        base.extend(extra);
    }
    ctrl("s", "slider", base, children)
}

pub(crate) fn slider_values(events: &[ScreenEvent]) -> Vec<(f64, Option<usize>)> {
    events
        .iter()
        .filter_map(|event| match event {
            ScreenEvent::Slider { value, step, .. } => Some((*value, *step)),
            _ => None,
        })
        .collect()
}

pub(crate) fn edit_box(props: Value) -> ResolvedControl {
    let mut base = json!({
        "size": [100, 20], "text_box_name": "name", "max_length": 5,
        "text_control": "display", "place_holder_control": "hint",
        "button_mappings": [
            { "from_button_id": "button.menu_select", "to_button_id": "button.text_edit_box_selected", "handle_select": true, "handle_deselect": false, "mapping_type": "pressed" },
            { "from_button_id": "button.menu_select", "to_button_id": "button.text_edit_box_selected", "handle_select": false, "handle_deselect": true, "mapping_type": "global", "consume_event": false }
        ]
    });
    if let (Value::Object(base), Value::Object(extra)) = (&mut base, props) {
        base.extend(extra);
    }
    ctrl(
        "edit",
        "edit_box",
        base,
        vec![
            ctrl(
                "display",
                "label",
                json!({ "text": "", "size": [100, 20] }),
                vec![],
            ),
            ctrl("hint", "label", json!({ "text": "Name" }), vec![]),
        ],
    )
}

pub(crate) fn text_of(screen: &Screen, name: &str) -> String {
    let bound = screen.bound();
    let (laid, _) = layout_with(&bound, [200.0, 200.0], &env(), &screen.view);
    find(&laid, name).control.properties["text"]
        .as_str()
        .unwrap_or("")
        .to_owned()
}
