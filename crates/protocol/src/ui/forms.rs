//! Server forms: bounded retention metadata plus the modal response lifecycle.
//!
//! A server form arrives as one `ModalFormRequest` packet carrying a JSON
//! document whose top-level `"type"` member selects the vanilla family
//! (`"form"` button menu, `"modal"` two-button dialog, `"custom_form"` input
//! form). Each family has a bounded model; anything outside it stays explicitly
//! unsupported, and malformed JSON is a semantic skip.

mod custom;
mod npc;
mod text;
pub use text::FormText;
use text::{literal_value, optional_text, text_value};

use std::sync::Arc;

pub use custom::{CustomForm, CustomFormElement, FormNumber};
pub use npc::{
    NPC_DIALOGUE_FORM_ID, NpcButton, NpcDialogueForm, NpcRequestKind, npc_request_packet,
    server_settings_request_packet,
};
pub(crate) use npc::{normalize_npc_dialogue, normalize_server_settings};

use serde::{
    Deserialize, Deserializer,
    de::{DeserializeSeed, IgnoredAny},
};
use valentine::bedrock::version::v1_26_51::{
    EnumsModalFormCancelReason, ModalFormRequestPacket, ModalFormResponsePacket,
};

use super::{MAX_FORM_JSON_BYTES, MAX_UI_TEXT_BYTES, UiEvent, UiPacketError};

pub const MAX_FORM_JSON_DEPTH: usize = 16;
/// Array entries remain bounded by the document size, including their separators.
pub const MAX_CUSTOM_FORM_ITEMS: usize = MAX_FORM_JSON_BYTES / 2;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextMenuForm {
    pub title: FormText,
    pub content: FormText,
    /// Array order is the zero-based wire selection index. Never truncate it.
    pub buttons: Arc<[FormText]>,
    /// Per-button image, aligned by index with `buttons` (`None` when the button has
    /// none). Retained for the renderer; the atlas step still decides what loads.
    pub button_images: Arc<[Option<FormButtonImage>]>,
    /// Count of buttons carrying an image; kept for the interim presentation notice.
    pub omitted_images: u16,
}

/// A retained button image reference. The URL/path string is decoded but not
/// fetched here; nothing is loaded until the renderer's atlas step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormButtonImage {
    /// Wire `{ "type": "path", "data": <resource-pack texture path> }`.
    Path(Arc<str>),
    /// Wire `{ "type": "url", "data": <url> }`.
    Url(Arc<str>),
}

/// A menu whose `elements` mix buttons with labels, headers, and dividers. Only
/// buttons answer; a button's response index counts buttons alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ElementMenuForm {
    pub title: FormText,
    pub content: FormText,
    pub elements: Arc<[MenuElement]>,
}

impl ElementMenuForm {
    pub fn button_count(&self) -> usize {
        self.elements
            .iter()
            .filter(|element| matches!(element, MenuElement::Button { .. }))
            .count()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuElement {
    Button {
        text: FormText,
        image: Option<FormButtonImage>,
    },
    Label(FormText),
    Header(FormText),
    Divider,
}

/// The `"modal"` family: `button1` answers `true`, `button2` answers `false`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModalDialogForm {
    pub title: FormText,
    pub content: FormText,
    pub button1: FormText,
    pub button2: FormText,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnsupportedForm {
    Family,
    Controls,
    Limit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServerFormModel {
    TextMenu(TextMenuForm),
    ElementMenu(ElementMenuForm),
    Modal(ModalDialogForm),
    Custom(CustomForm),
    NpcDialogue(NpcDialogueForm),
    Unsupported(UnsupportedForm),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormKind {
    Modal,
    Menu,
    Custom,
    /// Missing, non-string, or unrecognized `"type"` member.
    Unknown,
    /// A server settings response (a custom form shown with the settings).
    ServerSettings,
    /// An NPC dialogue opened or closed by the server.
    NpcDialogue,
}

impl FormKind {
    /// Classifies the native family spellings; unknown families remain unsupported.
    fn from_wire(type_member: &str) -> Self {
        match type_member {
            "modal" => Self::Modal,
            "form" => Self::Menu,
            "custom_form" => Self::Custom,
            _ => Self::Unknown,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormRequestEvent {
    pub form_id: u32,
    pub kind: FormKind,
    pub title: Option<Arc<str>>,
    pub json: Arc<str>,
    pub model: ServerFormModel,
}

pub(crate) fn normalize_form(packet: ModalFormRequestPacket) -> Result<UiEvent, UiPacketError> {
    let json = bounded_form(packet.form_uijson)?;
    let mut header = FormHeader::default();
    scan_form_header(&json, &mut header)?;
    let kind = header.kind.unwrap_or(FormKind::Unknown);
    let model = form_model(&json, kind);
    Ok(UiEvent::Form(FormRequestEvent {
        form_id: packet.form_id,
        kind,
        title: header.title,
        json,
        model,
    }))
}

fn form_model(json: &str, kind: FormKind) -> ServerFormModel {
    let unsupported = ServerFormModel::Unsupported;
    if kind == FormKind::Unknown {
        return unsupported(UnsupportedForm::Family);
    }
    // The raw document and nesting were bounded before this second parse;
    // allocation here is bounded by MAX_FORM_JSON_BYTES, not server counts.
    let Ok(value) = serde_json::from_str::<serde_json::Value>(json) else {
        return unsupported(UnsupportedForm::Controls);
    };
    let Some(object) = value.as_object() else {
        return unsupported(UnsupportedForm::Controls);
    };
    let wire_type = object.get("type").and_then(serde_json::Value::as_str);
    match (kind, wire_type) {
        (FormKind::Menu, Some("form")) => text_menu_model(object),
        (FormKind::Modal, Some("modal")) => modal_model(object),
        (FormKind::Custom, Some("custom_form")) => custom::custom_model(object),
        _ => unsupported(UnsupportedForm::Family),
    }
}

fn modal_model(object: &serde_json::Map<String, serde_json::Value>) -> ServerFormModel {
    let field = |key: &str| match object.get(key) {
        Some(value) => text_value(value),
        None => Err(UnsupportedForm::Controls),
    };
    let parsed = (|| {
        Ok::<_, UnsupportedForm>(ModalDialogForm {
            title: optional_text(object, "title")?,
            content: optional_text(object, "content")?,
            button1: field("button1")?,
            button2: field("button2")?,
        })
    })();
    match parsed {
        Ok(form) => ServerFormModel::Modal(form),
        Err(reason) => ServerFormModel::Unsupported(reason),
    }
}

/// Reads a decoration without letting unused metadata reject the menu.
fn menu_decoration(
    element: &serde_json::Map<String, serde_json::Value>,
    kind: &str,
) -> Result<MenuElement, UnsupportedForm> {
    Ok(match kind {
        "label" => MenuElement::Label(optional_text(element, "text")?),
        "header" => MenuElement::Header(optional_text(element, "text")?),
        _ => MenuElement::Divider,
    })
}

/// Selects buttons before elements, preserving button-only response indexes.
fn text_menu_model(object: &serde_json::Map<String, serde_json::Value>) -> ServerFormModel {
    match parse_menu(object) {
        Ok(form) => form,
        Err(reason) => ServerFormModel::Unsupported(reason),
    }
}

/// Normalizes either menu representation and ignores fields the client does not use.
fn parse_menu(
    object: &serde_json::Map<String, serde_json::Value>,
) -> Result<ServerFormModel, UnsupportedForm> {
    let (controls, typed) = match object.get("buttons").filter(|value| !value.is_null()) {
        Some(buttons) => (buttons, false),
        None => (
            object.get("elements").ok_or(UnsupportedForm::Controls)?,
            true,
        ),
    };
    let controls = controls.as_array().ok_or(UnsupportedForm::Controls)?;
    let title = optional_text(object, "title")?;
    let content = optional_text(object, "content")?;
    let mut labels = Vec::with_capacity(controls.len());
    let mut images = Vec::with_capacity(controls.len());
    let mut elements = Vec::with_capacity(controls.len());
    let mut decorated = false;
    for control in controls {
        let control = control.as_object().ok_or(UnsupportedForm::Controls)?;
        if typed {
            match control.get("type").and_then(serde_json::Value::as_str) {
                Some(kind @ ("label" | "header" | "divider")) => {
                    elements.push(menu_decoration(control, kind)?);
                    decorated = true;
                    continue;
                }
                Some("button") => (),
                _ => return Err(UnsupportedForm::Controls),
            }
        }
        let label = optional_text(control, "text")?;
        let image = button_image(control.get("image"))?;
        elements.push(MenuElement::Button {
            text: label.clone(),
            image: image.clone(),
        });
        labels.push(label);
        images.push(image);
    }
    if decorated {
        Ok(ServerFormModel::ElementMenu(ElementMenuForm {
            title,
            content,
            elements: elements.into(),
        }))
    } else {
        let omitted_images = images
            .iter()
            .filter(|image| image.is_some())
            .count()
            .min(u16::MAX as usize) as u16;
        Ok(ServerFormModel::TextMenu(TextMenuForm {
            title,
            content,
            buttons: labels.into(),
            button_images: images.into(),
            omitted_images,
        }))
    }
}

/// Retains supported image sources; unknown source kinds have no button texture.
fn button_image(
    value: Option<&serde_json::Value>,
) -> Result<Option<FormButtonImage>, UnsupportedForm> {
    let Some(value) = value.filter(|value| !value.is_null()) else {
        return Ok(None);
    };
    let object = value.as_object().ok_or(UnsupportedForm::Controls)?;
    let kind = object.get("type").and_then(serde_json::Value::as_str);
    if !matches!(kind, Some("path" | "url")) {
        return Ok(None);
    }
    let data = object
        .get("data")
        .ok_or(UnsupportedForm::Controls)
        .and_then(literal_value)?;
    Ok(Some(if kind == Some("path") {
        FormButtonImage::Path(data)
    } else {
        FormButtonImage::Url(data)
    }))
}

fn bounded_form(value: String) -> Result<Arc<str>, UiPacketError> {
    if value.len() > MAX_FORM_JSON_BYTES {
        return Err(UiPacketError::FormTooLarge {
            bytes: value.len(),
            max: MAX_FORM_JSON_BYTES,
        });
    }
    Ok(Arc::from(value))
}

#[derive(Default)]
struct FormHeader {
    kind: Option<FormKind>,
    title: Option<Arc<str>>,
    title_overflow: Option<(usize, usize)>,
}

fn scan_form_header(json: &str, header: &mut FormHeader) -> Result<(), UiPacketError> {
    ensure_bounded_depth(json.as_bytes())?;
    let mut deserializer = serde_json::Deserializer::from_str(json);
    FormHeaderProbe { header }
        .deserialize(&mut deserializer)
        .and_then(|()| deserializer.end())
        .map_err(|_| UiPacketError::InvalidFormJson)?;
    if let Some((bytes, max)) = header.title_overflow.take() {
        return Err(UiPacketError::TextTooLong { bytes, max });
    }
    Ok(())
}

/// Counts container nesting outside strings so pathological input is rejected
/// by an explicit repository bound before any serde walk.
fn ensure_bounded_depth(bytes: &[u8]) -> Result<(), UiPacketError> {
    let mut depth = 0usize;
    let mut cursor = 0;
    while cursor < bytes.len() {
        match bytes[cursor] {
            b'"' => {
                cursor =
                    scan_json_string_end(bytes, cursor).ok_or(UiPacketError::InvalidFormJson)?
            }
            b'{' | b'[' => {
                depth += 1;
                if depth > MAX_FORM_JSON_DEPTH {
                    return Err(UiPacketError::FormJsonDepthExceeded {
                        depth,
                        max: MAX_FORM_JSON_DEPTH,
                    });
                }
                cursor += 1;
            }
            b'}' | b']' => {
                depth = depth.saturating_sub(1);
                cursor += 1;
            }
            _ => cursor += 1,
        }
    }
    Ok(())
}

fn scan_json_string_end(bytes: &[u8], start: usize) -> Option<usize> {
    debug_assert_eq!(bytes.get(start), Some(&b'"'));
    let mut cursor = start + 1;
    while cursor < bytes.len() {
        match bytes[cursor] {
            b'"' => return Some(cursor + 1),
            b'\\' => cursor = cursor.checked_add(2)?,
            0x00..=0x1f => return None,
            _ => cursor += 1,
        }
    }
    None
}

struct FormHeaderProbe<'a> {
    header: &'a mut FormHeader,
}

/// Captures only the two metadata members; every other member is skipped
/// without interpretation, so duplicate or odd values cannot grow retained
/// state beyond the raw text itself.
impl<'de> DeserializeSeed<'de> for FormHeaderProbe<'_> {
    type Value = ();

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_map(self)
    }
}

impl<'de, 'a> serde::de::Visitor<'de> for FormHeaderProbe<'a> {
    type Value = ();

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("a top-level server form object")
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: serde::de::MapAccess<'de>,
    {
        while let Some(key) = map.next_key::<String>()? {
            match key.as_str() {
                "type" => {
                    if let MetadataMember::Text(value) = map.next_value()? {
                        self.header.kind = Some(FormKind::from_wire(&value));
                    }
                }
                "title" => {
                    if let MetadataMember::Text(value) = map.next_value()? {
                        if value.len() > MAX_UI_TEXT_BYTES {
                            self.header.title_overflow = Some((value.len(), MAX_UI_TEXT_BYTES));
                        } else {
                            self.header.title = Some(Arc::from(value));
                            self.header.title_overflow = None;
                        }
                    }
                }
                _ => {
                    map.next_value::<IgnoredAny>()?;
                }
            }
        }
        Ok(())
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum MetadataMember {
    Text(String),
    Other(IgnoredAny),
}

/// A submit answer for a menu or modal form. Per gophertunnel v1.57.0
/// `minecraft/protocol/packet/modal_form_response.go`, a menu answers with a bare
/// integer button index and a modal with `true`/`false`; the JSON scalar shape is
/// what distinguishes them on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModalFormResponseSelection {
    /// Menu/action form: the zero-based button index.
    ButtonIndex(u32),
    /// Modal form: `true` for button1, `false` for button2.
    ModalButton(bool),
}

/// One custom-form control's submitted value, in element order. Non-input elements
/// (label/header/divider) encode as `Null` so array indexes stay aligned.
#[derive(Debug, Clone, PartialEq)]
pub enum CustomFormValue {
    Toggle(bool),
    Slider(f64),
    /// Step-slider selected step index.
    Step(i32),
    /// Dropdown selected option index.
    Dropdown(i32),
    Input(String),
    /// Selected multiselect option indexes, in selection order.
    MultiSelect(Arc<[i32]>),
    Null,
}

/// Encodes a menu/modal submit answer: form id, response data present(1) with the
/// bare JSON scalar (integer index or `true`/`false`), cancel reason absent(0).
pub fn modal_form_submit_response(
    form_id: u32,
    selection: ModalFormResponseSelection,
) -> crate::Packet {
    let payload = match selection {
        ModalFormResponseSelection::ButtonIndex(index) => index.to_string(),
        ModalFormResponseSelection::ModalButton(flag) => flag.to_string(),
    };
    ModalFormResponsePacket {
        form_id,
        json_response: Some(payload),
        form_cancel_reason: None,
    }
    .into()
}

/// Encodes a custom-form submit answer: response data present(1) carrying the JSON
/// array of `values` in element order, cancel reason absent(0).
pub fn custom_form_submit_response(form_id: u32, values: &[CustomFormValue]) -> crate::Packet {
    let array = serde_json::Value::Array(values.iter().map(custom_value_json).collect());
    ModalFormResponsePacket {
        form_id,
        json_response: Some(array.to_string()),
        form_cancel_reason: None,
    }
    .into()
}

fn custom_value_json(value: &CustomFormValue) -> serde_json::Value {
    match value {
        CustomFormValue::Toggle(flag) => serde_json::Value::Bool(*flag),
        CustomFormValue::Slider(number) => serde_json::Number::from_f64(*number)
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null),
        CustomFormValue::Step(index) | CustomFormValue::Dropdown(index) => {
            serde_json::Value::Number((*index).into())
        }
        CustomFormValue::MultiSelect(indexes) => serde_json::json!(indexes.as_ref()),
        CustomFormValue::Input(text) => serde_json::Value::String(text.clone()),
        CustomFormValue::Null => serde_json::Value::Null,
    }
}

/// An overlapping dialog cannot take ownership of an already occupied UI.
/// UserBusy is wire value 1 in the pinned response schema.
pub fn modal_form_busy_response(form_id: u32) -> crate::Packet {
    ModalFormResponsePacket {
        form_id,
        json_response: None,
        form_cancel_reason: Some(EnumsModalFormCancelReason::Userbusy),
    }
    .into()
}

/// Encodes the vanilla user-closed dismissal: response data absent(0), cancel
/// reason present(1) as the `UserClosed` wire value 0.
pub fn modal_form_cancel_response(form_id: u32) -> crate::Packet {
    ModalFormResponsePacket {
        form_id,
        json_response: None,
        form_cancel_reason: Some(EnumsModalFormCancelReason::Userclosed),
    }
    .into()
}
