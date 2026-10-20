use super::*;
use json_ui::{
    ButtonInput, Catalog, Context, Dispatcher, EmptyLibrary, InputMode, LayoutEnv, ResolvedControl,
    TextMeasure, TextureMeta, TextureSource, ViewState, bind, form_data_source, hit_regions,
    layout, resolve,
    vanilla::{Binding, Button},
};
use protocol::CustomFormValue;
use serde_json::{Value, json};

#[test]
fn control_locked_disables_every_supported_control_in_game() {
    for (mut element, binding, expected) in controls() {
        element["control_locked"] = json!("ingame");
        let (form, state) = mapped_model(element.clone(), true);
        assert_eq!(
            bound_control(&form, binding, "button")
                .properties
                .get("enabled"),
            Some(&json!(false)),
            "{element}"
        );
        assert_eq!(state.submission().as_ref(), &[expected]);
    }
}

#[test]
fn control_locked_tracks_both_phases_and_keeps_missing_or_unknown_locks_enabled() {
    for (element, binding, _) in controls() {
        for lock in [
            None,
            Some(json!("none")),
            Some(json!("pregame")),
            Some(json!("ingame")),
            Some(json!("")),
            Some(json!("unknown")),
            Some(Value::Null),
            Some(json!(true)),
            Some(json!(3)),
        ] {
            for in_game in [false, true] {
                let mut element = element.clone();
                if let Some(lock) = &lock {
                    element["control_locked"] = lock.clone();
                }
                let expected = match lock.as_ref().and_then(Value::as_str) {
                    Some("pregame") => in_game,
                    Some("ingame") => !in_game,
                    _ => true,
                };
                let (form, _) = mapped_model(element, in_game);
                assert_eq!(
                    bound_control(&form, binding, "button")
                        .properties
                        .get("enabled"),
                    Some(&json!(expected)),
                    "lock={lock:?}, in_game={in_game}"
                );
            }
        }
    }
}

#[test]
fn control_locked_bindings_block_pointer_keyboard_gamepad_and_text_edits() {
    for (element, binding, _) in controls() {
        let kind = match element["type"].as_str().unwrap() {
            "step_slider" => "slider",
            "input" => "edit_box",
            kind => kind,
        };
        for locked in [false, true] {
            let mut element = element.clone();
            element["control_locked"] = json!(if locked { "ingame" } else { "none" });
            let (form, _) = mapped_model(element, true);
            let bound = bound_control(&form, binding, kind);
            let laid = layout(
                &bound,
                [100.0, 20.0],
                &LayoutEnv {
                    text: &Metrics,
                    textures: &Metrics,
                },
            );
            let hits = hit_regions(&laid);
            let hit = hits.first().expect("interactive control");
            assert_eq!(hit.enabled, !locked);
            for (mode, id, point) in [
                (InputMode::Mouse, Button::MenuSelect, Some([10.0, 10.0])),
                (InputMode::Mouse, Button::MenuOk, None),
                (InputMode::Gamepad, Button::MenuOk, None),
                (InputMode::Touch, Button::MenuSelect, Some([10.0, 10.0])),
            ] {
                let mut dispatcher = Dispatcher::default();
                let mut view = ViewState {
                    focused: Some(hit.key.clone()),
                    ..Default::default()
                };
                let mut events = Vec::new();
                for down in [true, false] {
                    events.extend(
                        dispatcher
                            .button(
                                &hits,
                                &mut view,
                                ButtonInput {
                                    id: id.as_str(),
                                    down,
                                    point,
                                    mode,
                                    now: 0.0,
                                },
                            )
                            .events,
                    );
                }
                assert_eq!(
                    events.is_empty(),
                    locked,
                    "{kind}, {mode:?}, locked={locked}"
                );
            }
            if kind == "edit_box" {
                let edits =
                    Dispatcher::default().text(&hits, &mut ViewState::default(), "Changed", None);
                assert_eq!(edits.events.is_empty(), locked);
            }
        }
    }
}

/// Supplies one initialized value for each control that consumes a lock binding.
fn controls() -> [(Value, Binding, CustomFormValue); 4] {
    [
        (
            json!({"type":"toggle","default":true}),
            Binding::CustomToggleEnabled,
            CustomFormValue::Toggle(true),
        ),
        (
            json!({"type":"slider","min":0,"max":10,"default":4}),
            Binding::CustomSliderEnabled,
            CustomFormValue::Slider(4.0),
        ),
        (
            json!({"type":"step_slider","steps":["Slow","Fast"],"default":1}),
            Binding::CustomSliderEnabled,
            CustomFormValue::Step(1),
        ),
        (
            json!({"type":"input","default":"Draft"}),
            Binding::CustomInputEnabled,
            CustomFormValue::Input("Draft".into()),
        ),
    ]
}

/// Converts a real form packet and retains its independent response values.
fn mapped_model(element: Value, in_game: bool) -> (FormModel, FormEngineState) {
    let document = json!({"type":"custom_form","content":[element]});
    let Some(protocol::WorldEvent::Ui(protocol::UiEvent::Form(event))) =
        protocol::into_world_event(
            protocol::test_support::packets::modal_form_request_packet(7, &document.to_string()),
            0,
        )
        .unwrap()
    else {
        panic!("custom form packet");
    };
    let state = FormEngineState::for_model(&event.model);
    let form = engine_model(
        &event.model,
        &state,
        in_game,
        &|_| None,
        &|_| RemoteImage::Failed,
        &|text| text.to_string(),
    )
    .unwrap();
    (form, state)
}

/// Binds an authored widget to one collection answer, without local game fixtures.
fn bound_control(model: &FormModel, binding: Binding, kind: &str) -> ResolvedControl {
    let mut catalog = Catalog::default();
    catalog.overlay_text("ui/probe.json", &json!({
        "namespace":"probe",
        "control": {
            "type":kind, "size":[100,20], "max_length":64, "always_listening":true,
            "collection_name":"custom_form", "collection_index":0,
            "button_mappings":[
                {"from_button_id":Button::MenuSelect.as_str(),"to_button_id":"button.probe","mapping_type":"pressed"},
                {"from_button_id":Button::MenuOk.as_str(),"to_button_id":"button.probe","mapping_type":"focused"}
            ],
            "bindings":[{
                "binding_type":"collection", "binding_collection_name":"custom_form",
                "binding_name":binding.as_str(), "binding_name_override":"#enabled"
            }]
        }
    }).to_string());
    let root = resolve(&catalog, "probe.control", &Context::desktop())
        .control
        .unwrap();
    bind(&root, &form_data_source(model), &EmptyLibrary)
}

struct Metrics;
impl TextMeasure for Metrics {
    /// Fixed dimensions keep this input test independent of installed fonts.
    fn extent(&self, _: &str) -> [f64; 2] {
        [10.0, 10.0]
    }
}
impl TextureSource for Metrics {
    /// The isolated controls do not require textures.
    fn texture(&self, _: &str) -> Option<TextureMeta> {
        None
    }
}
