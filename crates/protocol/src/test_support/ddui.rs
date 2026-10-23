//! Script screen stores shaped like the vanilla server's `CustomForm` and `MessageBox`.

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::UiEvent;
use crate::ui::ddui::{
    DataDrivenUiEvent, DataStoreChange, DataValue, SCRIPT_STORE, ScriptedScreen,
    ShowDataDrivenScreen,
};

fn map(entries: Vec<(&str, DataValue)>) -> DataValue {
    DataValue::Map(
        entries
            .into_iter()
            .map(|(key, value)| (Arc::<str>::from(key), value))
            .collect::<BTreeMap<_, _>>(),
    )
}

fn text(value: &str) -> DataValue {
    DataValue::String(value.into())
}

fn number(value: f64) -> DataValue {
    DataValue::Number(value)
}

/// An array-shaped map: decimal keys and a `length` member.
fn array(items: Vec<DataValue>) -> DataValue {
    let length = items.len() as i64;
    let mut entries: Vec<(String, DataValue)> = items
        .into_iter()
        .enumerate()
        .map(|(index, item)| (index.to_string(), item))
        .collect();
    entries.push(("length".into(), DataValue::Int(length)));
    DataValue::Map(
        entries
            .into_iter()
            .map(|(key, value)| (Arc::<str>::from(key), value))
            .collect(),
    )
}

/// One layout entry of control `kind` with its members; it is visible.
fn control(kind: &str, members: Vec<(&str, DataValue)>) -> DataValue {
    let marker = format!("{kind}_visible");
    let mut entries = vec![
        (marker.as_str(), DataValue::Bool(true)),
        ("visible", DataValue::Bool(true)),
    ];
    entries.extend(members);
    map(entries)
}

/// A custom form with every control the script library offers, as a server publishes it.
#[must_use]
pub fn custom_form_property() -> DataValue {
    let button = |label: &str, disabled: bool| {
        control(
            "button",
            vec![
                ("label", text(label)),
                ("onClick", number(0.0)),
                ("disabled", DataValue::Bool(disabled)),
            ],
        )
    };
    map(vec![
        ("title", text("Data-Driven Test")),
        (
            "closeButton",
            map(vec![
                ("button_visible", DataValue::Bool(true)),
                ("visible", DataValue::Bool(true)),
                ("label", text("Close")),
                ("onClick", number(0.0)),
            ]),
        ),
        (
            "layout",
            array(vec![
                control("header", vec![("text", text("Header text"))]),
                control("label", vec![("text", text("Clicks: 0"))]),
                control("divider", vec![]),
                control(
                    "toggle",
                    vec![
                        ("label", text("A toggle")),
                        ("toggled", DataValue::Bool(true)),
                        ("description", text("Toggle description")),
                    ],
                ),
                control(
                    "slider",
                    vec![
                        ("label", text("A slider")),
                        ("value", number(4.0)),
                        ("minValue", number(0.0)),
                        ("maxValue", number(10.0)),
                        ("step", number(2.0)),
                    ],
                ),
                control(
                    "dropdown",
                    vec![
                        ("label", text("A dropdown")),
                        ("value", number(1.0)),
                        (
                            "items",
                            array(vec![
                                map(vec![("label", text("First")), ("value", number(0.0))]),
                                map(vec![("label", text("Second")), ("value", number(1.0))]),
                                map(vec![("label", text("Third")), ("value", number(2.0))]),
                            ]),
                        ),
                    ],
                ),
                control(
                    "textfield",
                    vec![("label", text("A text field")), ("text", text("hello"))],
                ),
                control("spacer", vec![]),
                control(
                    "label",
                    vec![(
                        "text",
                        map(vec![(
                            "rawtext",
                            DataValue::List(vec![map(vec![("text", text("Raw message text"))])]),
                        )]),
                    )],
                ),
                button("Click me", false),
                button("Disabled", true),
            ]),
        ),
    ])
}

/// A message box with a body and two buttons.
#[must_use]
pub fn message_box_property() -> DataValue {
    let button = |label: &str| map(vec![("label", text(label)), ("onClick", number(0.0))]);
    map(vec![
        ("title", text("Message title")),
        ("body", text("Message body text")),
        ("button1", button("Yes")),
        ("button2", button("No")),
    ])
}

/// The store change and show request a server sends for `screen` with `property`.
#[must_use]
pub fn show_events(
    screen: ScriptedScreen,
    form_id: u32,
    data_instance_id: u32,
    property: DataValue,
) -> [UiEvent; 2] {
    [
        UiEvent::DataDrivenUi(DataDrivenUiEvent::Store(vec![DataStoreChange::Replace {
            store: SCRIPT_STORE.into(),
            property: screen.property(Some(data_instance_id)).into(),
            update_count: 1,
            value: property,
        }])),
        UiEvent::DataDrivenUi(DataDrivenUiEvent::Show(ShowDataDrivenScreen {
            screen_id: screen.screen_id().into(),
            form_id,
            data_instance_id: Some(data_instance_id),
        })),
    ]
}
