//! Server forms: bounded retention metadata plus the modal response lifecycle.
//!
//! A server form arrives as one `ModalFormRequest` packet carrying a JSON
//! document whose top-level `"type"` member selects the vanilla family
//! (`"form"` button menu, `"modal"` two-button dialog, `"custom_form"` input
//! form). Each family has a bounded model; anything outside it stays explicitly
//! unsupported, and malformed JSON is a semantic skip.

mod custom;
mod npc;

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
pub const MAX_FORM_BUTTONS: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextMenuForm {
    pub title: Arc<str>,
    pub content: Arc<str>,
    /// Array order is the zero-based wire selection index. Never truncate it.
    pub buttons: Arc<[Arc<str>]>,
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
    pub title: Arc<str>,
    pub content: Arc<str>,
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
        text: Arc<str>,
        image: Option<FormButtonImage>,
    },
    Label(Arc<str>),
    Header(Arc<str>),
    Divider,
}

/// The `"modal"` family: `button1` answers `true`, `button2` answers `false`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModalDialogForm {
    pub title: Arc<str>,
    pub content: Arc<str>,
    pub button1: Arc<str>,
    pub button2: Arc<str>,
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
    /// Only the `"form"` spelling is pinned by a gophertunnel fixture; the
    /// `"modal"` and `"custom_form"` classifications are provisional pending
    /// a version-matched wire reference, and anything else stays
    /// [`FormKind::Unknown`].
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

/// A member that must be a bounded string when present; absent reads as empty.
/// `Err` carries the unsupported reason for a wrong type or an oversized value.
fn optional_text<'a>(
    object: &'a serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Result<&'a str, UnsupportedForm> {
    match object.get(key) {
        None => Ok(""),
        Some(value) => required_text_value(value),
    }
}

fn required_text_value(value: &serde_json::Value) -> Result<&str, UnsupportedForm> {
    let text = value.as_str().ok_or(UnsupportedForm::Controls)?;
    if text.len() > MAX_UI_TEXT_BYTES {
        return Err(UnsupportedForm::Limit);
    }
    Ok(text)
}

fn modal_model(object: &serde_json::Map<String, serde_json::Value>) -> ServerFormModel {
    let field = |key: &str| match object.get(key) {
        Some(value) => required_text_value(value),
        None => Err(UnsupportedForm::Controls),
    };
    let parsed = (|| {
        Ok::<_, UnsupportedForm>(ModalDialogForm {
            title: Arc::from(optional_text(object, "title")?),
            content: Arc::from(optional_text(object, "content")?),
            button1: Arc::from(field("button1")?),
            button2: Arc::from(field("button2")?),
        })
    })();
    match parsed {
        Ok(form) => ServerFormModel::Modal(form),
        Err(reason) => ServerFormModel::Unsupported(reason),
    }
}

/// A non-button `elements` entry: label/header need `text`, a divider may omit
/// it, and `image` may only be null.
fn menu_decoration(
    element: &serde_json::Map<String, serde_json::Value>,
    kind: &str,
) -> Result<MenuElement, UnsupportedForm> {
    if element
        .keys()
        .any(|key| key != "type" && key != "text" && key != "image")
        || element.get("image").is_some_and(|image| !image.is_null())
    {
        return Err(UnsupportedForm::Controls);
    }
    let text = match element.get("text") {
        Some(value) => required_text_value(value)?,
        None if kind == "divider" => "",
        None => return Err(UnsupportedForm::Controls),
    };
    Ok(match kind {
        "label" => MenuElement::Label(Arc::from(text)),
        "header" => MenuElement::Header(Arc::from(text)),
        _ => MenuElement::Divider,
    })
}

fn text_menu_model(object: &serde_json::Map<String, serde_json::Value>) -> ServerFormModel {
    let unsupported = ServerFormModel::Unsupported;
    // A menu has one controls representation. Never combine arrays or drop
    // unsupported elements, since either would change response indexes.
    let (controls, element_controls) = match (object.get("buttons"), object.get("elements")) {
        (Some(buttons), None) => (buttons, false),
        (None, Some(elements)) => (elements, true),
        _ => return unsupported(UnsupportedForm::Controls),
    };
    let text = |key: &str| match object.get(key) {
        None => Some(""),
        Some(value) => value.as_str(),
    };
    let (Some(title), Some(content), Some(buttons)) =
        (text("title"), text("content"), controls.as_array())
    else {
        return unsupported(UnsupportedForm::Controls);
    };
    if title.len() > MAX_UI_TEXT_BYTES
        || content.len() > MAX_UI_TEXT_BYTES
        || buttons.len() > MAX_FORM_BUTTONS
    {
        return unsupported(UnsupportedForm::Limit);
    }
    let mut labels = Vec::with_capacity(buttons.len());
    let mut images = Vec::with_capacity(buttons.len());
    let mut elements = Vec::new();
    let mut decorated = false;
    let mut omitted_images = 0;
    for button in buttons {
        let Some(button) = button.as_object() else {
            return unsupported(UnsupportedForm::Controls);
        };
        if element_controls
            && let Some(kind @ ("label" | "header" | "divider")) =
                button.get("type").and_then(serde_json::Value::as_str)
        {
            match menu_decoration(button, kind) {
                Ok(element) => elements.push(element),
                Err(reason) => return unsupported(reason),
            }
            decorated = true;
            continue;
        }
        if button
            .keys()
            .any(|key| key != "text" && key != "image" && (!element_controls || key != "type"))
        {
            return unsupported(UnsupportedForm::Controls);
        }
        if element_controls
            && button.get("type").and_then(serde_json::Value::as_str) != Some("button")
        {
            return unsupported(UnsupportedForm::Controls);
        }
        let mut image = None;
        // ServerFormBindingInformation::createBindingData
        // normalizes both representations through the same image value. Absent
        // and null images both mean a text-only button.
        if let Some(value) = button.get("image").filter(|value| !value.is_null()) {
            let Some(object) = value.as_object() else {
                return unsupported(UnsupportedForm::Controls);
            };
            let kind = object.get("type").and_then(serde_json::Value::as_str);
            if object.keys().any(|key| key != "type" && key != "data")
                || !matches!(kind, Some("url" | "path"))
            {
                return unsupported(UnsupportedForm::Controls);
            }
            let Some(data) = object.get("data").and_then(serde_json::Value::as_str) else {
                return unsupported(UnsupportedForm::Controls);
            };
            if data.len() > MAX_UI_TEXT_BYTES {
                return unsupported(UnsupportedForm::Limit);
            }
            image = Some(match kind {
                Some("path") => FormButtonImage::Path(Arc::from(data)),
                _ => FormButtonImage::Url(Arc::from(data)),
            });
            omitted_images += 1;
        }
        let Some(label) = button.get("text").and_then(serde_json::Value::as_str) else {
            return unsupported(UnsupportedForm::Controls);
        };
        if label.len() > MAX_UI_TEXT_BYTES {
            return unsupported(UnsupportedForm::Limit);
        }
        let label: Arc<str> = Arc::from(label);
        elements.push(MenuElement::Button {
            text: Arc::clone(&label),
            image: image.clone(),
        });
        labels.push(label);
        images.push(image);
    }
    if decorated {
        return ServerFormModel::ElementMenu(ElementMenuForm {
            title: Arc::from(title),
            content: Arc::from(content),
            elements: elements.into(),
        });
    }
    ServerFormModel::TextMenu(TextMenuForm {
        title: Arc::from(title),
        content: Arc::from(content),
        buttons: labels.into(),
        button_images: images.into(),
        omitted_images,
    })
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
    Step(u32),
    /// Dropdown selected option index.
    Dropdown(u32),
    Input(String),
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
