use bytes::{BufMut, Bytes};

pub use valentine_bedrock_core::bedrock::borrowed::{
    BedrockBorrowDecode, BorrowedStr, GAME_PACKET_ID, RawMcpeFrame, RawMcpeHeader,
    take_u32le_prefixed_string, take_varint_prefixed_bytes, take_varint_prefixed_string,
};
use valentine_bedrock_core::bedrock::codec::{BedrockCodec, BedrockSized, U32LE, VarInt};

#[cfg(feature = "bedrock_1_26_30")]
use crate::bedrock::version::v1_26_30::{
    DisconnectFailReason, DisconnectPacket, DisconnectPacketContent, LoginPacket, LoginTokens,
    TextPacket, TextPacketCategory, TextPacketContent, TextPacketContentAnnouncement,
    TextPacketContentJson, TextPacketContentJukeboxPopup, TextPacketType,
};

#[cfg(feature = "bedrock_1_26_30")]
fn varint_prefixed_len(bytes: &[u8]) -> usize {
    VarInt(bytes.len() as i32).encoded_size() + bytes.len()
}

#[cfg(feature = "bedrock_1_26_30")]
fn u32le_prefixed_len(bytes: &[u8]) -> usize {
    U32LE(bytes.len() as u32).encoded_size() + bytes.len()
}

#[cfg(feature = "bedrock_1_26_30")]
fn encode_varint_prefixed_bytes<B: BufMut>(
    bytes: &[u8],
    buf: &mut B,
) -> Result<(), std::io::Error> {
    VarInt(bytes.len() as i32).encode(buf)?;
    buf.put_slice(bytes);
    Ok(())
}

#[cfg(feature = "bedrock_1_26_30")]
fn encode_u32le_prefixed_bytes<B: BufMut>(bytes: &[u8], buf: &mut B) -> Result<(), std::io::Error> {
    U32LE(bytes.len() as u32).encode(buf)?;
    buf.put_slice(bytes);
    Ok(())
}

#[cfg(feature = "bedrock_1_26_30")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginTokensView {
    pub identity: BorrowedStr,
    pub client: BorrowedStr,
}

#[cfg(feature = "bedrock_1_26_30")]
impl BedrockSized for LoginTokensView {
    fn encoded_size(&self) -> usize {
        u32le_prefixed_len(self.identity.as_bytes()) + u32le_prefixed_len(self.client.as_bytes())
    }
}

#[cfg(feature = "bedrock_1_26_30")]
impl BedrockBorrowDecode for LoginTokensView {
    type Args = ();

    fn borrow_decode(
        buf: &mut Bytes,
        _args: Self::Args,
    ) -> Result<Self, crate::bedrock::error::DecodeError> {
        Ok(Self {
            identity: take_u32le_prefixed_string(buf)?,
            client: take_u32le_prefixed_string(buf)?,
        })
    }
}

#[cfg(feature = "bedrock_1_26_30")]
impl LoginTokensView {
    pub fn encode<B: BufMut>(&self, buf: &mut B) -> Result<(), std::io::Error> {
        encode_u32le_prefixed_bytes(self.identity.as_bytes(), buf)?;
        encode_u32le_prefixed_bytes(self.client.as_bytes(), buf)?;
        Ok(())
    }
}

#[cfg(feature = "bedrock_1_26_30")]
impl From<LoginTokensView> for LoginTokens {
    fn from(value: LoginTokensView) -> Self {
        Self {
            identity: value.identity.to_string_lossy().into_owned(),
            client: value.client.to_string_lossy().into_owned(),
        }
    }
}

#[cfg(feature = "bedrock_1_26_30")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginPacketView {
    pub protocol_version: i32,
    pub tokens: LoginTokensView,
}

#[cfg(feature = "bedrock_1_26_30")]
impl BedrockSized for LoginPacketView {
    fn encoded_size(&self) -> usize {
        4 + varint_prefixed_len_for_size(self.tokens.encoded_size())
    }
}

#[cfg(feature = "bedrock_1_26_30")]
impl BedrockBorrowDecode for LoginPacketView {
    type Args = ();

    fn borrow_decode(
        buf: &mut Bytes,
        _args: Self::Args,
    ) -> Result<Self, crate::bedrock::error::DecodeError> {
        let protocol_version = i32::decode(buf, ())?;
        let mut tokens = take_varint_prefixed_bytes(buf)?;
        let tokens = LoginTokensView::borrow_decode(&mut tokens, ())?;
        Ok(Self {
            protocol_version,
            tokens,
        })
    }
}

#[cfg(feature = "bedrock_1_26_30")]
impl LoginPacketView {
    pub fn decode(buf: &mut Bytes) -> Result<Self, crate::bedrock::error::DecodeError> {
        Self::borrow_decode(buf, ())
    }

    pub fn encode<B: BufMut>(&self, buf: &mut B) -> Result<(), std::io::Error> {
        self.protocol_version.encode(buf)?;
        VarInt(self.tokens.encoded_size() as i32).encode(buf)?;
        self.tokens.encode(buf)?;
        Ok(())
    }
}

#[cfg(feature = "bedrock_1_26_30")]
impl From<LoginPacketView> for LoginPacket {
    fn from(value: LoginPacketView) -> Self {
        Self {
            protocol_version: value.protocol_version,
            tokens: value.tokens.into(),
        }
    }
}

#[cfg(feature = "bedrock_1_26_30")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisconnectPacketContentView {
    pub message: BorrowedStr,
    pub filtered_message: BorrowedStr,
}

#[cfg(feature = "bedrock_1_26_30")]
impl BedrockSized for DisconnectPacketContentView {
    fn encoded_size(&self) -> usize {
        varint_prefixed_len(self.message.as_bytes())
            + varint_prefixed_len(self.filtered_message.as_bytes())
    }
}

#[cfg(feature = "bedrock_1_26_30")]
impl BedrockBorrowDecode for DisconnectPacketContentView {
    type Args = ();

    fn borrow_decode(
        buf: &mut Bytes,
        _args: Self::Args,
    ) -> Result<Self, crate::bedrock::error::DecodeError> {
        Ok(Self {
            message: take_varint_prefixed_string(buf)?,
            filtered_message: take_varint_prefixed_string(buf)?,
        })
    }
}

#[cfg(feature = "bedrock_1_26_30")]
impl DisconnectPacketContentView {
    pub fn encode<B: BufMut>(&self, buf: &mut B) -> Result<(), std::io::Error> {
        encode_varint_prefixed_bytes(self.message.as_bytes(), buf)?;
        encode_varint_prefixed_bytes(self.filtered_message.as_bytes(), buf)?;
        Ok(())
    }
}

#[cfg(feature = "bedrock_1_26_30")]
impl From<DisconnectPacketContentView> for DisconnectPacketContent {
    fn from(value: DisconnectPacketContentView) -> Self {
        Self {
            message: value.message.to_string_lossy().into_owned(),
            filtered_message: value.filtered_message.to_string_lossy().into_owned(),
        }
    }
}

#[cfg(feature = "bedrock_1_26_30")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisconnectPacketView {
    pub reason: DisconnectFailReason,
    pub hide_disconnect_reason: bool,
    pub content: Option<DisconnectPacketContentView>,
}

#[cfg(feature = "bedrock_1_26_30")]
impl BedrockSized for DisconnectPacketView {
    fn encoded_size(&self) -> usize {
        self.reason.encoded_size() + 1 + self.content.as_ref().map_or(0, BedrockSized::encoded_size)
    }
}

#[cfg(feature = "bedrock_1_26_30")]
impl BedrockBorrowDecode for DisconnectPacketView {
    type Args = ();

    fn borrow_decode(
        buf: &mut Bytes,
        _args: Self::Args,
    ) -> Result<Self, crate::bedrock::error::DecodeError> {
        let reason = DisconnectFailReason::decode(buf, ())?;
        let hide_disconnect_reason = bool::decode(buf, ())?;
        let content = if hide_disconnect_reason {
            None
        } else {
            Some(DisconnectPacketContentView::borrow_decode(buf, ())?)
        };

        Ok(Self {
            reason,
            hide_disconnect_reason,
            content,
        })
    }
}

#[cfg(feature = "bedrock_1_26_30")]
impl DisconnectPacketView {
    pub fn decode(buf: &mut Bytes) -> Result<Self, crate::bedrock::error::DecodeError> {
        Self::borrow_decode(buf, ())
    }

    pub fn encode<B: BufMut>(&self, buf: &mut B) -> Result<(), std::io::Error> {
        self.reason.encode(buf)?;
        self.hide_disconnect_reason.encode(buf)?;
        if let Some(content) = &self.content {
            content.encode(buf)?;
        }
        Ok(())
    }
}

#[cfg(feature = "bedrock_1_26_30")]
impl From<DisconnectPacketView> for DisconnectPacket {
    fn from(value: DisconnectPacketView) -> Self {
        Self {
            reason: value.reason,
            hide_disconnect_reason: value.hide_disconnect_reason,
            content: value.content.map(Into::into),
        }
    }
}

#[cfg(feature = "bedrock_1_26_30")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextPacketContentAnnouncementView {
    pub source_name: BorrowedStr,
    pub message: BorrowedStr,
}

#[cfg(feature = "bedrock_1_26_30")]
impl BedrockSized for TextPacketContentAnnouncementView {
    fn encoded_size(&self) -> usize {
        varint_prefixed_len(self.source_name.as_bytes())
            + varint_prefixed_len(self.message.as_bytes())
    }
}

#[cfg(feature = "bedrock_1_26_30")]
impl BedrockBorrowDecode for TextPacketContentAnnouncementView {
    type Args = ();

    fn borrow_decode(
        buf: &mut Bytes,
        _args: Self::Args,
    ) -> Result<Self, crate::bedrock::error::DecodeError> {
        Ok(Self {
            source_name: take_varint_prefixed_string(buf)?,
            message: take_varint_prefixed_string(buf)?,
        })
    }
}

#[cfg(feature = "bedrock_1_26_30")]
impl TextPacketContentAnnouncementView {
    pub fn encode<B: BufMut>(&self, buf: &mut B) -> Result<(), std::io::Error> {
        encode_varint_prefixed_bytes(self.source_name.as_bytes(), buf)?;
        encode_varint_prefixed_bytes(self.message.as_bytes(), buf)?;
        Ok(())
    }
}

#[cfg(feature = "bedrock_1_26_30")]
impl From<TextPacketContentAnnouncementView> for TextPacketContentAnnouncement {
    fn from(value: TextPacketContentAnnouncementView) -> Self {
        Self {
            source_name: value.source_name.to_string_lossy().into_owned(),
            message: value.message.to_string_lossy().into_owned(),
        }
    }
}

#[cfg(feature = "bedrock_1_26_30")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextPacketContentJsonView {
    pub message: BorrowedStr,
}

#[cfg(feature = "bedrock_1_26_30")]
impl BedrockSized for TextPacketContentJsonView {
    fn encoded_size(&self) -> usize {
        varint_prefixed_len(self.message.as_bytes())
    }
}

#[cfg(feature = "bedrock_1_26_30")]
impl BedrockBorrowDecode for TextPacketContentJsonView {
    type Args = ();

    fn borrow_decode(
        buf: &mut Bytes,
        _args: Self::Args,
    ) -> Result<Self, crate::bedrock::error::DecodeError> {
        Ok(Self {
            message: take_varint_prefixed_string(buf)?,
        })
    }
}

#[cfg(feature = "bedrock_1_26_30")]
impl TextPacketContentJsonView {
    pub fn encode<B: BufMut>(&self, buf: &mut B) -> Result<(), std::io::Error> {
        encode_varint_prefixed_bytes(self.message.as_bytes(), buf)
    }
}

#[cfg(feature = "bedrock_1_26_30")]
impl From<TextPacketContentJsonView> for TextPacketContentJson {
    fn from(value: TextPacketContentJsonView) -> Self {
        Self {
            message: value.message.to_string_lossy().into_owned(),
        }
    }
}

#[cfg(feature = "bedrock_1_26_30")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextPacketContentJukeboxPopupView {
    pub message: BorrowedStr,
    pub parameters: Vec<BorrowedStr>,
}

#[cfg(feature = "bedrock_1_26_30")]
impl BedrockSized for TextPacketContentJukeboxPopupView {
    fn encoded_size(&self) -> usize {
        varint_prefixed_len(self.message.as_bytes())
            + VarInt(self.parameters.len() as i32).encoded_size()
            + self
                .parameters
                .iter()
                .map(|item| varint_prefixed_len(item.as_bytes()))
                .sum::<usize>()
    }
}

#[cfg(feature = "bedrock_1_26_30")]
impl BedrockBorrowDecode for TextPacketContentJukeboxPopupView {
    type Args = ();

    fn borrow_decode(
        buf: &mut Bytes,
        _args: Self::Args,
    ) -> Result<Self, crate::bedrock::error::DecodeError> {
        let message = take_varint_prefixed_string(buf)?;
        let len_raw = VarInt::decode(buf, ())?.0 as i64;
        if len_raw < 0 {
            return Err(crate::bedrock::error::DecodeError::NegativeLength { value: len_raw });
        }
        let len = len_raw as usize;
        let mut parameters = Vec::with_capacity(len);
        for _ in 0..len {
            parameters.push(take_varint_prefixed_string(buf)?);
        }
        Ok(Self {
            message,
            parameters,
        })
    }
}

#[cfg(feature = "bedrock_1_26_30")]
impl TextPacketContentJukeboxPopupView {
    pub fn encode<B: BufMut>(&self, buf: &mut B) -> Result<(), std::io::Error> {
        encode_varint_prefixed_bytes(self.message.as_bytes(), buf)?;
        VarInt(self.parameters.len() as i32).encode(buf)?;
        for item in &self.parameters {
            encode_varint_prefixed_bytes(item.as_bytes(), buf)?;
        }
        Ok(())
    }
}

#[cfg(feature = "bedrock_1_26_30")]
impl From<TextPacketContentJukeboxPopupView> for TextPacketContentJukeboxPopup {
    fn from(value: TextPacketContentJukeboxPopupView) -> Self {
        Self {
            message: value.message.to_string_lossy().into_owned(),
            parameters: value
                .parameters
                .into_iter()
                .map(|item| item.to_string_lossy().into_owned())
                .collect(),
        }
    }
}

#[cfg(feature = "bedrock_1_26_30")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextPacketContentView {
    Announcement(TextPacketContentAnnouncementView),
    Chat(TextPacketContentAnnouncementView),
    Json(TextPacketContentJsonView),
    JsonAnnouncement(TextPacketContentJsonView),
    JsonWhisper(TextPacketContentJsonView),
    JukeboxPopup(TextPacketContentJukeboxPopupView),
    Popup(TextPacketContentJukeboxPopupView),
    Raw(TextPacketContentJsonView),
    System(TextPacketContentJsonView),
    Tip(TextPacketContentJsonView),
    Translation(TextPacketContentJukeboxPopupView),
    Whisper(TextPacketContentAnnouncementView),
}

#[cfg(feature = "bedrock_1_26_30")]
impl BedrockSized for TextPacketContentView {
    fn encoded_size(&self) -> usize {
        match self {
            Self::Announcement(v) | Self::Chat(v) | Self::Whisper(v) => v.encoded_size(),
            Self::Json(v)
            | Self::JsonAnnouncement(v)
            | Self::JsonWhisper(v)
            | Self::Raw(v)
            | Self::System(v)
            | Self::Tip(v) => v.encoded_size(),
            Self::JukeboxPopup(v) | Self::Popup(v) | Self::Translation(v) => v.encoded_size(),
        }
    }
}

#[cfg(feature = "bedrock_1_26_30")]
impl TextPacketContentView {
    pub fn decode_for_type(
        buf: &mut Bytes,
        type_: TextPacketType,
    ) -> Result<Option<Self>, crate::bedrock::error::DecodeError> {
        let content = match type_ {
            TextPacketType::Announcement => Some(Self::Announcement(
                TextPacketContentAnnouncementView::borrow_decode(buf, ())?,
            )),
            TextPacketType::Chat => Some(Self::Chat(
                TextPacketContentAnnouncementView::borrow_decode(buf, ())?,
            )),
            TextPacketType::Json => Some(Self::Json(TextPacketContentJsonView::borrow_decode(
                buf,
                (),
            )?)),
            TextPacketType::JsonAnnouncement => Some(Self::JsonAnnouncement(
                TextPacketContentJsonView::borrow_decode(buf, ())?,
            )),
            TextPacketType::JsonWhisper => Some(Self::JsonWhisper(
                TextPacketContentJsonView::borrow_decode(buf, ())?,
            )),
            TextPacketType::JukeboxPopup => Some(Self::JukeboxPopup(
                TextPacketContentJukeboxPopupView::borrow_decode(buf, ())?,
            )),
            TextPacketType::Popup => Some(Self::Popup(
                TextPacketContentJukeboxPopupView::borrow_decode(buf, ())?,
            )),
            TextPacketType::Raw => Some(Self::Raw(TextPacketContentJsonView::borrow_decode(
                buf,
                (),
            )?)),
            TextPacketType::System => Some(Self::System(TextPacketContentJsonView::borrow_decode(
                buf,
                (),
            )?)),
            TextPacketType::Tip => Some(Self::Tip(TextPacketContentJsonView::borrow_decode(
                buf,
                (),
            )?)),
            TextPacketType::Translation => Some(Self::Translation(
                TextPacketContentJukeboxPopupView::borrow_decode(buf, ())?,
            )),
            TextPacketType::Whisper => Some(Self::Whisper(
                TextPacketContentAnnouncementView::borrow_decode(buf, ())?,
            )),
            _ => None,
        };
        Ok(content)
    }

    pub fn encode<B: BufMut>(&self, buf: &mut B) -> Result<(), std::io::Error> {
        match self {
            Self::Announcement(v) | Self::Chat(v) | Self::Whisper(v) => v.encode(buf),
            Self::Json(v)
            | Self::JsonAnnouncement(v)
            | Self::JsonWhisper(v)
            | Self::Raw(v)
            | Self::System(v)
            | Self::Tip(v) => v.encode(buf),
            Self::JukeboxPopup(v) | Self::Popup(v) | Self::Translation(v) => v.encode(buf),
        }
    }
}

#[cfg(feature = "bedrock_1_26_30")]
impl From<TextPacketContentView> for TextPacketContent {
    fn from(value: TextPacketContentView) -> Self {
        match value {
            TextPacketContentView::Announcement(v) => Self::Announcement(v.into()),
            TextPacketContentView::Chat(v) => Self::Chat(v.into()),
            TextPacketContentView::Json(v) => Self::Json(v.into()),
            TextPacketContentView::JsonAnnouncement(v) => Self::JsonAnnouncement(v.into()),
            TextPacketContentView::JsonWhisper(v) => Self::JsonWhisper(v.into()),
            TextPacketContentView::JukeboxPopup(v) => Self::JukeboxPopup(v.into()),
            TextPacketContentView::Popup(v) => Self::Popup(v.into()),
            TextPacketContentView::Raw(v) => Self::Raw(v.into()),
            TextPacketContentView::System(v) => Self::System(v.into()),
            TextPacketContentView::Tip(v) => Self::Tip(v.into()),
            TextPacketContentView::Translation(v) => Self::Translation(v.into()),
            TextPacketContentView::Whisper(v) => Self::Whisper(v.into()),
        }
    }
}

#[cfg(feature = "bedrock_1_26_30")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextPacketView {
    pub needs_translation: bool,
    pub category: TextPacketCategory,
    pub type_: TextPacketType,
    pub content: Option<TextPacketContentView>,
    pub xuid: BorrowedStr,
    pub platform_chat_id: BorrowedStr,
    pub filtered_message: Option<BorrowedStr>,
}

#[cfg(feature = "bedrock_1_26_30")]
impl BedrockSized for TextPacketView {
    fn encoded_size(&self) -> usize {
        1 + self.category.encoded_size()
            + self.type_.encoded_size()
            + self.content.as_ref().map_or(0, BedrockSized::encoded_size)
            + varint_prefixed_len(self.xuid.as_bytes())
            + varint_prefixed_len(self.platform_chat_id.as_bytes())
            + 1
            + self
                .filtered_message
                .as_ref()
                .map_or(0, |value| varint_prefixed_len(value.as_bytes()))
    }
}

#[cfg(feature = "bedrock_1_26_30")]
impl BedrockBorrowDecode for TextPacketView {
    type Args = ();

    fn borrow_decode(
        buf: &mut Bytes,
        _args: Self::Args,
    ) -> Result<Self, crate::bedrock::error::DecodeError> {
        let needs_translation = bool::decode(buf, ())?;
        let category = TextPacketCategory::decode(buf, ())?;
        let type_ = TextPacketType::decode(buf, ())?;
        let content = TextPacketContentView::decode_for_type(buf, type_)?;
        let xuid = take_varint_prefixed_string(buf)?;
        let platform_chat_id = take_varint_prefixed_string(buf)?;
        let has_filtered_message = bool::decode(buf, ())?;
        let filtered_message = if has_filtered_message {
            Some(take_varint_prefixed_string(buf)?)
        } else {
            None
        };

        Ok(Self {
            needs_translation,
            category,
            type_,
            content,
            xuid,
            platform_chat_id,
            filtered_message,
        })
    }
}

#[cfg(feature = "bedrock_1_26_30")]
impl TextPacketView {
    pub fn decode(buf: &mut Bytes) -> Result<Self, crate::bedrock::error::DecodeError> {
        Self::borrow_decode(buf, ())
    }

    pub fn encode<B: BufMut>(&self, buf: &mut B) -> Result<(), std::io::Error> {
        self.needs_translation.encode(buf)?;
        self.category.encode(buf)?;
        self.type_.encode(buf)?;
        if let Some(content) = &self.content {
            content.encode(buf)?;
        }
        encode_varint_prefixed_bytes(self.xuid.as_bytes(), buf)?;
        encode_varint_prefixed_bytes(self.platform_chat_id.as_bytes(), buf)?;
        self.filtered_message.is_some().encode(buf)?;
        if let Some(filtered_message) = &self.filtered_message {
            encode_varint_prefixed_bytes(filtered_message.as_bytes(), buf)?;
        }
        Ok(())
    }
}

#[cfg(feature = "bedrock_1_26_30")]
impl From<TextPacketView> for TextPacket {
    fn from(value: TextPacketView) -> Self {
        Self {
            needs_translation: value.needs_translation,
            category: value.category,
            type_: value.type_,
            content: value.content.map(Into::into),
            xuid: value.xuid.to_string_lossy().into_owned(),
            platform_chat_id: value.platform_chat_id.to_string_lossy().into_owned(),
            filtered_message: value
                .filtered_message
                .map(|value| value.to_string_lossy().into_owned()),
        }
    }
}

#[cfg(feature = "bedrock_1_26_30")]
fn varint_prefixed_len_for_size(inner: usize) -> usize {
    VarInt(inner as i32).encoded_size() + inner
}

#[cfg(feature = "bedrock_1_26_30")]
pub type BorrowedLoginPacket = LoginPacketView;
#[cfg(feature = "bedrock_1_26_30")]
pub type BorrowedDisconnectPacket = DisconnectPacketView;
#[cfg(feature = "bedrock_1_26_30")]
pub type BorrowedTextPacket = TextPacketView;
