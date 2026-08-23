//! Server forms: bounded retention metadata plus the modal response lifecycle.
//!
//! A server form arrives as one `ModalFormRequest` packet carrying a JSON
//! document whose top-level `"type"` member selects the vanilla family
//! (`"modal"` message dialog, `"form"` button menu, `"custom_form"` element
//! list; the pinned gophertunnel fixture uses `"form"`). Normalization
//! validates structure and captures only the family, the title, and the raw
//! text — never the element model — so oversized or malformed content is a
//! counted semantic skip instead of session state.

use std::sync::Arc;

use serde::{
    Deserialize, Deserializer,
    de::{DeserializeSeed, IgnoredAny},
};
use valentine::bedrock::version::v1_26_44::{
    EnumsModalFormCancelReason, ModalFormRequestPacket, ModalFormResponsePacket,
};

use super::{MAX_FORM_JSON_BYTES, MAX_UI_TEXT_BYTES, UiEvent, UiPacketError};

pub const MAX_FORM_JSON_DEPTH: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormKind {
    Modal,
    Menu,
    Custom,
    /// Missing, non-string, or unrecognized `"type"` member.
    Unknown,
}

impl FormKind {
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
}

pub(crate) fn normalize_form(packet: ModalFormRequestPacket) -> Result<UiEvent, UiPacketError> {
    let json = bounded_form(packet.form_uijson)?;
    let mut header = FormHeader::default();
    scan_form_header(&json, &mut header)?;
    Ok(UiEvent::Form(FormRequestEvent {
        form_id: packet.form_id,
        kind: header.kind.unwrap_or(FormKind::Unknown),
        title: header.title,
        json,
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

/// The one selection this slice wires: the zero-based button index a modal or
/// menu form answers with. Custom-form element state has no capture surface
/// yet, so no wrong-shaped custom payload can even be constructed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModalFormResponseSelection {
    ButtonIndex(u32),
}

/// Encodes the protocol-2168 submit answer: form id, response data present(1)
/// with the bare-number button selection, cancel reason absent(0)
/// (gophertunnel `minecraft/protocol/packet/modal_form_response.go`).
pub fn modal_form_submit_response(
    form_id: u32,
    selection: ModalFormResponseSelection,
) -> crate::Packet {
    let payload = match selection {
        ModalFormResponseSelection::ButtonIndex(index) => index.to_string(),
    };
    ModalFormResponsePacket {
        form_id,
        json_response: Some(payload),
        form_cancel_reason: None,
    }
    .into()
}

/// Encodes the vanilla user-closed dismissal: response data absent(0), cancel
/// reason present(1) as the `UserClosed` wire value 0.
pub fn modal_form_cancel_response(form_id: u32) -> crate::Packet {
    ModalFormResponsePacket {
        form_id,
        json_response: None,
        form_cancel_reason: Some(EnumsModalFormCancelReason::UserClosed),
    }
    .into()
}
