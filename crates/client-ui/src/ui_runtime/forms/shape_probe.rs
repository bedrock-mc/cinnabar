//! Opt-in, process-once rejected-form observation; never retains server text.
use std::{
    ffi::OsStr,
    sync::{
        OnceLock,
        atomic::{AtomicBool, Ordering},
    },
};

use protocol::{FormRequestEvent, ServerFormModel, UnsupportedForm};
use serde::{
    Deserializer,
    de::{DeserializeSeed, IgnoredAny, MapAccess, SeqAccess, Visitor},
};

pub(super) fn observe(event: &FormRequestEvent) {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    static CLAIMED: AtomicBool = AtomicBool::new(false);
    let enabled = *ENABLED.get_or_init(|| {
        opted_in(std::env::var_os(crate::diagnostic_markers::FORM_SHAPE_PROBE).as_deref())
    });
    if let Some(summary) = inspect(enabled, &CLAIMED, event) {
        bevy::log::warn!(target: "bedrock_client::form_shape_probe", "form shape probe: {summary:?}");
    }
}

fn opted_in(value: Option<&OsStr>) -> bool {
    value == Some(OsStr::new("1"))
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum Kind {
    #[default]
    Missing,
    Null,
    Boolean,
    Number,
    String,
    Array,
    Object,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum TypeClass {
    #[default]
    Missing,
    NonString,
    Form,
    Modal,
    Custom,
    Button,
    Header,
    Label,
    Divider,
    Url,
    Path,
    OtherString,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Shape {
    kind: Kind,
    string_bytes: usize,
    entries: usize,
    rawtext_kind: Kind,
    rawtext_entries: usize,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Image {
    shape: Shape,
    type_class: TypeClass,
    data: Shape,
    other_keys: usize,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Button {
    shape: Shape,
    text: Shape,
    type_class: TypeClass,
    image: Image,
    other_keys: usize,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Node {
    shape: Shape,
    type_class: TypeClass,
    type_shape: Shape,
    title: Shape,
    content: Shape,
    text: Shape,
    data: Shape,
    image: Image,
    buttons_shape: Shape,
    buttons: [Button; 2],
    elements_shape: Shape,
    elements: [Button; 2],
    other_keys: usize,
}
#[derive(Debug, PartialEq, Eq)]
enum Model {
    TextMenu,
    ElementMenu,
    Modal,
    Custom,
    NpcDialogue,
    Family,
    Controls,
    Limit,
}
#[derive(Debug, PartialEq, Eq)]
enum Summary {
    BoundsRejected,
    ParseRejected,
    Parsed {
        model: Model,
        kind: protocol::FormKind,
        node: Box<Node>,
    },
}

fn inspect(enabled: bool, claimed: &AtomicBool, event: &FormRequestEvent) -> Option<Summary> {
    // Supported forms preserve the one-shot budget without inspecting the document.
    if !enabled
        || !matches!(event.model, ServerFormModel::Unsupported(_))
        || claimed.swap(true, Ordering::AcqRel)
    {
        return None;
    }
    if !bounded(&event.json) {
        return Some(Summary::BoundsRejected);
    }
    let mut parser = serde_json::Deserializer::from_str(&event.json);
    let Ok(node) = Seed(Mode::Form).deserialize(&mut parser) else {
        return Some(Summary::ParseRejected);
    };
    if parser.end().is_err() || node.shape.kind != Kind::Object {
        return Some(Summary::ParseRejected);
    }
    let model = match event.model {
        ServerFormModel::TextMenu(_) => Model::TextMenu,
        ServerFormModel::ElementMenu(_) => Model::ElementMenu,
        ServerFormModel::Modal(_) => Model::Modal,
        ServerFormModel::Custom(_) => Model::Custom,
        ServerFormModel::NpcDialogue(_) => Model::NpcDialogue,
        ServerFormModel::Unsupported(UnsupportedForm::Family) => Model::Family,
        ServerFormModel::Unsupported(UnsupportedForm::Controls) => Model::Controls,
        ServerFormModel::Unsupported(UnsupportedForm::Limit) => Model::Limit,
    };
    Some(Summary::Parsed {
        model,
        kind: event.kind,
        node: Box::new(node),
    })
}

fn bounded(json: &str) -> bool {
    if json.len() > protocol::MAX_FORM_JSON_BYTES {
        return false;
    }
    let mut depth = 0usize;
    let mut string = false;
    let mut escaped = false;
    for byte in json.bytes() {
        if string {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                string = false;
            }
        } else {
            match byte {
                b'"' => string = true,
                b'{' | b'[' => {
                    depth += 1;
                    if depth > protocol::MAX_FORM_JSON_DEPTH {
                        return false;
                    }
                }
                b'}' | b']' => {
                    let Some(next) = depth.checked_sub(1) else {
                        return false;
                    };
                    depth = next;
                }
                _ => {}
            }
        }
    }
    depth == 0 && !string
}

#[derive(Clone, Copy)]
enum Mode {
    Form,
    Shape,
    Type,
    Buttons,
    Button,
    Image,
}
struct Seed(Mode);
impl<'de> DeserializeSeed<'de> for Seed {
    type Value = Node;
    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Node, D::Error> {
        deserializer.deserialize_any(self)
    }
}
impl Seed {
    fn node(&self, kind: Kind) -> Node {
        Node {
            shape: Shape {
                kind,
                ..Default::default()
            },
            type_class: if matches!(self.0, Mode::Type) {
                TypeClass::NonString
            } else {
                TypeClass::Missing
            },
            ..Default::default()
        }
    }
}
impl<'de> Visitor<'de> for Seed {
    type Value = Node;
    fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("bounded JSON structure")
    }
    fn visit_unit<E: serde::de::Error>(self) -> Result<Node, E> {
        Ok(self.node(Kind::Null))
    }
    fn visit_bool<E: serde::de::Error>(self, _: bool) -> Result<Node, E> {
        Ok(self.node(Kind::Boolean))
    }
    fn visit_i64<E: serde::de::Error>(self, _: i64) -> Result<Node, E> {
        Ok(self.node(Kind::Number))
    }
    fn visit_u64<E: serde::de::Error>(self, _: u64) -> Result<Node, E> {
        Ok(self.node(Kind::Number))
    }
    fn visit_f64<E: serde::de::Error>(self, _: f64) -> Result<Node, E> {
        Ok(self.node(Kind::Number))
    }
    fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<Node, E> {
        let mut node = self.node(Kind::String);
        node.shape.string_bytes = value.len();
        if matches!(self.0, Mode::Type) {
            node.type_class = match value {
                "form" => TypeClass::Form,
                "modal" => TypeClass::Modal,
                "custom_form" => TypeClass::Custom,
                "button" => TypeClass::Button,
                "header" => TypeClass::Header,
                "label" => TypeClass::Label,
                "divider" => TypeClass::Divider,
                "url" => TypeClass::Url,
                "path" => TypeClass::Path,
                _ => TypeClass::OtherString,
            };
        }
        Ok(node)
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Node, A::Error> {
        let mut node = self.node(Kind::Array);
        if matches!(self.0, Mode::Buttons) {
            while let Some(button) = seq.next_element_seed(Seed(Mode::Button))? {
                if node.shape.entries < 2 {
                    node.buttons[node.shape.entries] = Button {
                        shape: button.shape,
                        text: button.text,
                        type_class: button.type_class,
                        image: button.image,
                        other_keys: button.other_keys,
                    };
                }
                node.shape.entries = (node.shape.entries + 1).min(protocol::MAX_FORM_BUTTONS + 1);
            }
        } else {
            while seq.next_element::<IgnoredAny>()?.is_some() {
                node.shape.entries = (node.shape.entries + 1).min(protocol::MAX_FORM_BUTTONS + 1);
            }
        }
        Ok(node)
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Node, A::Error> {
        let mut node = self.node(Kind::Object);
        while let Some(key) = map.next_key_seed(KeySeed)? {
            node.shape.entries = (node.shape.entries + 1).min(protocol::MAX_FORM_BUTTONS + 1);
            match (self.0, key) {
                (Mode::Form | Mode::Button | Mode::Image, Key::Type) => {
                    let value = map.next_value_seed(Seed(Mode::Type))?;
                    node.type_class = value.type_class;
                    node.type_shape = value.shape;
                }
                (Mode::Form, Key::Title) => {
                    node.title = map.next_value_seed(Seed(Mode::Shape))?.shape
                }
                (Mode::Form, Key::Content) => {
                    node.content = map.next_value_seed(Seed(Mode::Shape))?.shape
                }
                (Mode::Form, Key::Buttons) => {
                    let value = map.next_value_seed(Seed(Mode::Buttons))?;
                    node.buttons_shape = value.shape;
                    node.buttons = value.buttons;
                }
                (Mode::Form, Key::Elements) => {
                    let value = map.next_value_seed(Seed(Mode::Buttons))?;
                    node.elements_shape = value.shape;
                    node.elements = value.buttons;
                }
                (Mode::Button, Key::Text) => {
                    node.text = map.next_value_seed(Seed(Mode::Shape))?.shape
                }
                (Mode::Button, Key::Image) => {
                    let value = map.next_value_seed(Seed(Mode::Image))?;
                    node.image = Image {
                        shape: value.shape,
                        type_class: value.type_class,
                        data: value.data,
                        other_keys: value.other_keys,
                    };
                }
                (Mode::Image, Key::Data) => {
                    node.data = map.next_value_seed(Seed(Mode::Shape))?.shape
                }
                (Mode::Shape, Key::Rawtext) => {
                    let value = map.next_value_seed(Seed(Mode::Shape))?;
                    node.shape.rawtext_kind = value.shape.kind;
                    node.shape.rawtext_entries = value.shape.entries;
                }
                _ => {
                    node.other_keys = (node.other_keys + 1).min(protocol::MAX_FORM_BUTTONS + 1);
                    map.next_value::<IgnoredAny>()?;
                }
            }
        }
        Ok(node)
    }
}

enum Key {
    Type,
    Title,
    Content,
    Buttons,
    Elements,
    Text,
    Image,
    Data,
    Rawtext,
    Other,
}
struct KeySeed;
impl<'de> DeserializeSeed<'de> for KeySeed {
    type Value = Key;
    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Key, D::Error> {
        deserializer.deserialize_identifier(self)
    }
}
impl<'de> Visitor<'de> for KeySeed {
    type Value = Key;
    fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("JSON object key")
    }
    fn visit_str<E: serde::de::Error>(self, key: &str) -> Result<Key, E> {
        Ok(match key {
            "type" => Key::Type,
            "title" => Key::Title,
            "content" => Key::Content,
            "buttons" => Key::Buttons,
            "elements" => Key::Elements,
            "text" => Key::Text,
            "image" => Key::Image,
            "data" => Key::Data,
            "rawtext" => Key::Rawtext,
            _ => Key::Other,
        })
    }
}

#[cfg(test)]
mod tests;
