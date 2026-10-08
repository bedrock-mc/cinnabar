//! Mock screen-controller data: the JSON an author writes for `#bindings`,
//! collections, factories, and whole server forms or HUD states, turned into
//! the engine's [`DataSource`]. Forms and the HUD go through the engine's own
//! mappings, so their binding names match what the client feeds.

use std::collections::BTreeMap;

use json_ui::{
    ActionElement, ActionForm, BossBar, CollectionItem, Context, CustomElement, CustomForm,
    DataSource, FactoryItem, FormButton, FormModel, HudModel, HudTitle, ModalForm, Scalar, Sidebar,
    Timed,
};
use serde::Deserialize;
use serde_json::Value;

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct MockData {
    /// `#name` -> bool, number or text.
    pub globals: BTreeMap<String, Value>,
    /// Collection name -> items; an item's `role` picks the factory control and
    /// its `#name` keys are the values read at that index.
    pub collections: BTreeMap<String, Vec<BTreeMap<String, Value>>>,
    /// Named factory -> controls it created.
    pub factories: BTreeMap<String, Vec<MockFactoryItem>>,
    pub grid_dimensions: BTreeMap<String, [u32; 2]>,
    pub factory_id: Option<String>,
    /// Unbound globals read as false, as menu screen controllers answer them.
    pub strict: bool,
    /// Radio toggle group name -> selected index.
    pub radio: BTreeMap<String, usize>,
    pub form: Option<MockForm>,
    pub hud: Option<MockHud>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct MockFactoryItem {
    pub control_id: String,
    pub name: Option<String>,
    pub vars: BTreeMap<String, Value>,
    pub values: BTreeMap<String, Value>,
    pub born: f64,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum MockForm {
    Action {
        #[serde(default)]
        title: String,
        #[serde(default)]
        body: String,
        #[serde(default)]
        buttons: Vec<MockButton>,
    },
    Modal {
        #[serde(default)]
        title: String,
        #[serde(default)]
        body: String,
        #[serde(default)]
        button1: String,
        #[serde(default)]
        button2: String,
    },
    Custom {
        #[serde(default)]
        title: String,
        #[serde(default)]
        elements: Vec<MockElement>,
        #[serde(default)]
        submit: Option<String>,
    },
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum MockButton {
    Text(String),
    Image { text: String, image: String },
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MockElement {
    Label {
        text: String,
    },
    Header {
        text: String,
    },
    Divider,
    Toggle {
        text: String,
        #[serde(default)]
        on: bool,
    },
    Slider {
        text: String,
        #[serde(default)]
        fraction: f64,
    },
    StepSlider {
        text: String,
        steps: usize,
        #[serde(default)]
        index: usize,
    },
    Dropdown {
        text: String,
        options: Vec<String>,
        #[serde(default)]
        index: usize,
    },
    Input {
        text: String,
        #[serde(default)]
        value: String,
        #[serde(default)]
        placeholder: String,
    },
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct MockHud {
    pub title: Option<String>,
    pub subtitle: Option<String>,
    pub actionbar: Option<String>,
    /// Sidebar title plus `[name, score]` rows, already sorted.
    pub sidebar: Option<MockSidebar>,
    pub boss_bars: Vec<MockBossBar>,
    pub chat: Vec<String>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct MockSidebar {
    pub title: String,
    pub rows: Vec<(String, String)>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct MockBossBar {
    pub name: String,
    pub progress: f64,
    pub color: String,
}

/// Everything a bind needs from mock data: the source, context additions and
/// the fade clocks a HUD title reads.
pub struct Prepared {
    pub data: DataSource,
    pub context: Context,
    pub clocks: BTreeMap<String, f64>,
}

impl MockData {
    pub fn prepare(&self, base: &Context) -> Prepared {
        let mut context = base.clone();
        let mut clocks = BTreeMap::new();
        let mut data = match (&self.form, &self.hud) {
            (Some(form), _) => {
                let model = form.model();
                context = json_ui::form_context(&model, &context);
                let mut data = json_ui::form_data_source(&model);
                if let Some(id) = json_ui::form_factory_id(&model) {
                    data.set_factory_id(id);
                }
                data
            }
            (None, Some(hud)) => {
                let model = hud.model();
                context = json_ui::hud_context(&context);
                clocks = json_ui::hud_clocks(&model);
                json_ui::hud_data_source(&model)
            }
            (None, None) => DataSource::new(),
        };
        for (name, value) in &self.globals {
            if let Some(scalar) = scalar(value) {
                data.set_global(hash_name(name), scalar);
            }
        }
        for (name, items) in &self.collections {
            data.set_collection(name.clone(), items.iter().map(collection_item).collect());
        }
        for (name, items) in &self.factories {
            data.set_factory(name.clone(), items.iter().map(factory_item).collect());
        }
        for (name, dimensions) in &self.grid_dimensions {
            data.set_grid_dimensions(hash_name(name), *dimensions);
        }
        for (name, index) in &self.radio {
            data.select_radio(name, *index);
        }
        if let Some(id) = &self.factory_id {
            data.set_factory_id(id.clone());
        }
        if self.strict {
            data.set_strict(true);
        }
        Prepared {
            data,
            context,
            clocks,
        }
    }
}

fn hash_name(name: &str) -> String {
    if name.starts_with('#') {
        name.to_owned()
    } else {
        format!("#{name}")
    }
}

fn scalar(value: &Value) -> Option<Scalar> {
    match value {
        Value::Bool(flag) => Some(Scalar::Bool(*flag)),
        Value::Number(number) => number.as_f64().map(Scalar::Num),
        Value::String(text) => Some(Scalar::Text(text.clone())),
        _ => None,
    }
}

fn collection_item(item: &BTreeMap<String, Value>) -> CollectionItem {
    let mut out = CollectionItem {
        role: item.get("role").and_then(Value::as_str).map(str::to_owned),
        values: BTreeMap::new(),
    };
    for (key, value) in item.iter().filter(|(key, _)| key.as_str() != "role") {
        if let Some(scalar) = scalar(value) {
            out.values.insert(hash_name(key), scalar);
        }
    }
    out
}

fn factory_item(item: &MockFactoryItem) -> FactoryItem {
    let mut out = FactoryItem::new(item.control_id.clone(), item.born);
    out.name = item.name.clone();
    out.vars = item
        .vars
        .iter()
        .map(|(key, value)| (key.trim_start_matches('$').to_owned(), value.clone()))
        .collect();
    out.values = item
        .values
        .iter()
        .filter_map(|(key, value)| Some((hash_name(key), scalar(value)?)))
        .collect();
    out
}

impl MockForm {
    fn model(&self) -> FormModel {
        match self {
            MockForm::Action {
                title,
                body,
                buttons,
            } => FormModel::Action(ActionForm {
                title: title.clone(),
                body: body.clone(),
                elements: buttons
                    .iter()
                    .map(|button| {
                        ActionElement::Button(match button {
                            MockButton::Text(text) => FormButton {
                                text: text.clone(),
                                image: None,
                            },
                            MockButton::Image { text, image } => FormButton {
                                text: text.clone(),
                                image: Some(json_ui::ButtonImage::Path(image.clone())),
                            },
                        })
                    })
                    .collect(),
            }),
            MockForm::Modal {
                title,
                body,
                button1,
                button2,
            } => FormModel::Modal(ModalForm {
                title: title.clone(),
                body: body.clone(),
                button1: button1.clone(),
                button2: button2.clone(),
            }),
            MockForm::Custom {
                title,
                elements,
                submit,
            } => FormModel::Custom(CustomForm {
                title: title.clone(),
                elements: elements.iter().map(MockElement::model).collect(),
                submit_text: submit.clone().unwrap_or_default(),
                submit_visible: true,
            }),
        }
    }
}

impl MockElement {
    fn model(&self) -> CustomElement {
        let tooltip = String::new();
        match self.clone() {
            MockElement::Label { text } => CustomElement::Label { text },
            MockElement::Header { text } => CustomElement::Header { text },
            MockElement::Divider => CustomElement::Divider,
            MockElement::Toggle { text, on } => CustomElement::Toggle { text, on, tooltip },
            MockElement::Slider { text, fraction } => CustomElement::Slider {
                text,
                fraction,
                tooltip,
            },
            MockElement::StepSlider { text, steps, index } => CustomElement::StepSlider {
                text,
                steps,
                index,
                tooltip,
            },
            MockElement::Dropdown {
                text,
                options,
                index,
            } => CustomElement::Dropdown {
                text,
                options,
                index,
                open: false,
                tooltip,
            },
            MockElement::Input {
                text,
                value,
                placeholder,
            } => CustomElement::Input {
                text,
                value,
                placeholder,
                tooltip,
            },
        }
    }
}

impl MockHud {
    fn model(&self) -> HudModel {
        let timed = |text: &Option<String>| {
            text.as_ref().map(|text| Timed {
                text: text.clone(),
                born: 0.0,
            })
        };
        HudModel {
            survival_ui: true,
            hotbar_visible: true,
            chat_visible: !self.chat.is_empty(),
            chat_lifetime: 10.0,
            chat: self
                .chat
                .iter()
                .map(|text| Timed {
                    text: text.clone(),
                    born: 0.0,
                })
                .collect(),
            title: self.title.as_ref().map(|title| HudTitle {
                creation_id: 0,
                title: title.clone(),
                subtitle: self.subtitle.clone().unwrap_or_default(),
                fade_in: 0.5,
                stay: 3.5,
                fade_out: 1.0,
                background_alpha: 0.0,
                born: 0.0,
            }),
            actionbar: timed(&self.actionbar),
            sidebar: self.sidebar.as_ref().map(|sidebar| Sidebar {
                title: sidebar.title.clone(),
                rows: sidebar.rows.clone(),
                background_opacity: 0.3,
                title_background_opacity: 0.4,
            }),
            boss_bars: self
                .boss_bars
                .iter()
                .map(|bar| BossBar {
                    name: bar.name.clone(),
                    progress: bar.progress,
                    color: bar.color.clone(),
                    notches: 0,
                })
                .collect(),
            ..HudModel::default()
        }
    }
}
