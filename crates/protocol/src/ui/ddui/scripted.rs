//! The vanilla script screens (`@minecraft/server-ui` `CustomForm` and `MessageBox`) read from
//! the data store property their server publishes for each shown instance.
//!
//! A custom form property holds `title`, `closeButton` and an array-shaped `layout` map. Each
//! layout entry names its control type with a `<type>_visible` member and hides itself with
//! `visible`. Controls report input by writing their own paths: buttons count presses in
//! `onClick`, and value controls write `toggled`, `value` or `text`.

use std::borrow::Cow;

use super::DataValue;

/// The data store every script screen publishes its properties into.
pub const SCRIPT_STORE: &str = "minecraft";

/// A screen the server's script UI library shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptedScreen {
    CustomForm,
    MessageBox,
}

impl ScriptedScreen {
    /// The screen id a `ShowScreen` request names this screen with.
    #[must_use]
    pub const fn screen_id(self) -> &'static str {
        match self {
            Self::CustomForm => "minecraft:custom_form",
            Self::MessageBox => "minecraft:message_box",
        }
    }

    /// The script screen a `ShowScreen` request names, if it is one.
    #[must_use]
    pub fn from_screen_id(screen_id: &str) -> Option<Self> {
        [Self::CustomForm, Self::MessageBox]
            .into_iter()
            .find(|screen| screen.screen_id() == screen_id)
    }

    /// The store property holding one shown instance; a request without an instance id reads
    /// instance 0.
    #[must_use]
    pub fn property(self, data_instance_id: Option<u32>) -> String {
        let prefix = match self {
            Self::CustomForm => "custom_form_data_",
            Self::MessageBox => "message_box_data_",
        };
        format!("{prefix}{}", data_instance_id.unwrap_or(0))
    }
}

/// Text a control displays: a plain string or a raw message (`rawtext`, `translate`, `with`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ScreenText<'a> {
    Plain(&'a str),
    /// A raw message object, to be resolved like chat raw text.
    RawMessage(&'a DataValue),
}

impl<'a> ScreenText<'a> {
    fn of(value: Option<&'a DataValue>) -> Option<Self> {
        match value? {
            DataValue::String(text) => Some(Self::Plain(text)),
            raw @ DataValue::Map(_) => Some(Self::RawMessage(raw)),
            _ => None,
        }
    }

    /// The raw message as JSON text, or the plain string itself.
    #[must_use]
    pub fn source(self) -> Cow<'a, str> {
        match self {
            Self::Plain(text) => Cow::Borrowed(text),
            Self::RawMessage(value) => Cow::Owned(json(value).to_string()),
        }
    }
}

/// A button's label and options.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScreenButton<'a> {
    pub label: Option<ScreenText<'a>>,
    pub tooltip: Option<ScreenText<'a>>,
    pub disabled: bool,
    /// Presses the server has seen; a press writes the next count.
    pub clicks: f64,
}

impl<'a> ScreenButton<'a> {
    fn of(entry: &'a DataValue) -> Self {
        Self {
            label: ScreenText::of(entry.get("label")),
            tooltip: ScreenText::of(entry.get("tooltip")),
            disabled: flag(entry, "disabled"),
            clicks: entry
                .get("onClick")
                .and_then(DataValue::as_number)
                .unwrap_or(0.0),
        }
    }
}

/// One dropdown choice.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DropdownItem<'a> {
    pub label: Option<ScreenText<'a>>,
    pub description: Option<ScreenText<'a>>,
    /// The number the dropdown's `value` takes when this item is chosen.
    pub value: f64,
}

/// What a layout entry draws.
#[derive(Debug, Clone, PartialEq)]
pub enum ControlKind<'a> {
    Header(Option<ScreenText<'a>>),
    Label(Option<ScreenText<'a>>),
    Divider,
    Spacer,
    Image {
        src: Option<&'a str>,
        pack: Option<&'a str>,
        width: Option<f64>,
        tooltip: Option<ScreenText<'a>>,
    },
    Button(ScreenButton<'a>),
    Toggle {
        label: Option<ScreenText<'a>>,
        description: Option<ScreenText<'a>>,
        toggled: bool,
        disabled: bool,
    },
    Slider {
        label: Option<ScreenText<'a>>,
        description: Option<ScreenText<'a>>,
        value: f64,
        min: f64,
        max: f64,
        step: f64,
        disabled: bool,
    },
    Dropdown {
        label: Option<ScreenText<'a>>,
        description: Option<ScreenText<'a>>,
        value: f64,
        items: Vec<DropdownItem<'a>>,
        disabled: bool,
    },
    TextField {
        label: Option<ScreenText<'a>>,
        description: Option<ScreenText<'a>>,
        text: &'a str,
        disabled: bool,
    },
}

/// One visible layout entry and the index its paths use.
#[derive(Debug, Clone, PartialEq)]
pub struct FormControl<'a> {
    pub index: usize,
    pub kind: ControlKind<'a>,
}

impl FormControl<'_> {
    /// The path of this control's `member`, such as `layout[3].onClick`.
    #[must_use]
    pub fn path(&self, member: &str) -> String {
        format!("layout[{}].{member}", self.index)
    }
}

/// A custom form: its title, optional close button and visible controls in layout order.
#[derive(Debug, Clone, PartialEq)]
pub struct CustomFormView<'a> {
    pub title: Option<ScreenText<'a>>,
    /// The header close button, when the form shows one.
    pub close_button: Option<ScreenButton<'a>>,
    pub controls: Vec<FormControl<'a>>,
}

/// The path a custom form's header close button counts presses in.
pub const CLOSE_BUTTON_CLICKS: &str = "closeButton.onClick";

impl<'a> CustomFormView<'a> {
    /// Reads a custom form property; `None` when it is not an object (a closed form is null).
    #[must_use]
    pub fn read(property: &'a DataValue) -> Option<Self> {
        let DataValue::Map(_) = property else {
            return None;
        };
        let close_button = property
            .get("closeButton")
            .filter(|entry| flag(entry, "visible"))
            .map(ScreenButton::of);
        let layout = property.get("layout");
        let controls = layout
            .map(|layout| {
                (0..layout.array_len())
                    .filter_map(|index| {
                        let entry = layout.element(index)?;
                        if !flag(entry, "visible") {
                            return None;
                        }
                        Some(FormControl {
                            index,
                            kind: control(entry)?,
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        Some(Self {
            title: ScreenText::of(property.get("title")),
            close_button,
            controls,
        })
    }
}

/// A message box: title, body and up to two buttons.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MessageBoxView<'a> {
    pub title: Option<ScreenText<'a>>,
    pub body: Option<ScreenText<'a>>,
    pub button1: Option<ScreenButton<'a>>,
    pub button2: Option<ScreenButton<'a>>,
}

impl<'a> MessageBoxView<'a> {
    /// Reads a message box property; `None` when it is not an object.
    #[must_use]
    pub fn read(property: &'a DataValue) -> Option<Self> {
        let DataValue::Map(_) = property else {
            return None;
        };
        Some(Self {
            title: ScreenText::of(property.get("title")),
            body: ScreenText::of(property.get("body")),
            button1: property.get("button1").map(ScreenButton::of),
            button2: property.get("button2").map(ScreenButton::of),
        })
    }

    /// The path counting presses of button 1 or 2.
    #[must_use]
    pub fn button_path(second: bool) -> &'static str {
        if second {
            "button2.onClick"
        } else {
            "button1.onClick"
        }
    }
}

fn flag(entry: &DataValue, member: &str) -> bool {
    entry
        .get(member)
        .and_then(DataValue::as_bool)
        .unwrap_or(false)
}

fn number(entry: &DataValue, member: &str) -> f64 {
    entry
        .get(member)
        .and_then(DataValue::as_number)
        .unwrap_or(0.0)
}

fn control(entry: &DataValue) -> Option<ControlKind<'_>> {
    let text = |member| ScreenText::of(entry.get(member));
    Some(if flag(entry, "header_visible") {
        ControlKind::Header(text("text"))
    } else if flag(entry, "label_visible") {
        ControlKind::Label(text("text"))
    } else if flag(entry, "divider_visible") {
        ControlKind::Divider
    } else if flag(entry, "spacer_visible") {
        ControlKind::Spacer
    } else if flag(entry, "image_visible") {
        ControlKind::Image {
            src: entry.get("image_src").and_then(DataValue::as_str),
            pack: entry.get("image_pack").and_then(DataValue::as_str),
            width: entry.get("image_width").and_then(DataValue::as_number),
            tooltip: text("image_tooltip"),
        }
    } else if flag(entry, "button_visible") {
        ControlKind::Button(ScreenButton::of(entry))
    } else if flag(entry, "toggle_visible") {
        ControlKind::Toggle {
            label: text("label"),
            description: text("description"),
            toggled: flag(entry, "toggled"),
            disabled: flag(entry, "disabled"),
        }
    } else if flag(entry, "slider_visible") {
        ControlKind::Slider {
            label: text("label"),
            description: text("description"),
            value: number(entry, "value"),
            min: number(entry, "minValue"),
            max: number(entry, "maxValue"),
            step: number(entry, "step"),
            disabled: flag(entry, "disabled"),
        }
    } else if flag(entry, "dropdown_visible") {
        let items = entry.get("items");
        ControlKind::Dropdown {
            label: text("label"),
            description: text("description"),
            value: number(entry, "value"),
            items: items
                .map(|items| {
                    (0..items.array_len())
                        .filter_map(|index| items.element(index))
                        .map(|item| DropdownItem {
                            label: ScreenText::of(item.get("label")),
                            description: ScreenText::of(item.get("description")),
                            value: number(item, "value"),
                        })
                        .collect()
                })
                .unwrap_or_default(),
            disabled: flag(entry, "disabled"),
        }
    } else if flag(entry, "textfield_visible") {
        ControlKind::TextField {
            label: text("label"),
            description: text("description"),
            text: entry.get("text").and_then(DataValue::as_str).unwrap_or(""),
            disabled: flag(entry, "disabled"),
        }
    } else {
        return None;
    })
}

/// The JSON form of a value, for raw message parsing.
fn json(value: &DataValue) -> serde_json::Value {
    match value {
        DataValue::None => serde_json::Value::Null,
        DataValue::Bool(value) => (*value).into(),
        DataValue::Int(value) => (*value).into(),
        DataValue::Number(value) => serde_json::Number::from_f64(*value)
            .map_or(serde_json::Value::Null, serde_json::Value::Number),
        DataValue::String(value) => value.as_ref().into(),
        DataValue::List(items) => items.iter().map(json).collect(),
        DataValue::Map(entries) => entries
            .iter()
            .map(|(key, value)| (key.to_string(), json(value)))
            .collect::<serde_json::Map<_, _>>()
            .into(),
    }
}
