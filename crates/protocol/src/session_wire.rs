//! Session endpoint messages. The core makes the only Minecraft login; the client sends one
//! [`ConnectRequest`], receives a [`SessionHandoff`] and its pack archives, then exchanges raw
//! packet batches until a terminal [`CoreMessage::Transfer`] or [`CoreMessage::Disconnect`].
//! Each connection carries one upstream session. The Go core's `proxy` package owns the same contract.

use std::fmt;

use bedrock_protocol_1_26_51::packets::PacketId;
use bytes::{BufMut, Bytes, BytesMut};
use serde::{Deserialize, Serialize};
use zeroize::Zeroize;

use bridge::{BridgeError, ConnectTarget, MAX_FRAME_LEN};

mod pack_cache;
mod pack_receiver;
pub use pack_receiver::HandoffPackReceiver;
mod server;
pub use pack_cache::CachedArchive;
#[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
pub use server::decode_native_connect;
pub use server::{
    CapturedPacketKind, captured_packet_kind, decode_connect, encode_core_message,
    packet_from_body, validate_replay_cache_mode,
};

use bridge::contract::{
    KIND_BATCH, KIND_CONNECT, KIND_DISCONNECT, KIND_HANDOFF, KIND_PACK_DATA, KIND_TRANSFER,
};
/// The StartGame packet ID, which ends a handoff's startup packets.
const START_GAME_PACKET_ID: u32 = PacketId::StartGame as u32;

/// The client's only setup message.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectRequest<D = serde_json::Value> {
    pub protocol: i32,
    /// Optional local diagnostic correlation; omitted in shipping builds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub join_profile_id: Option<u64>,
    /// `None` joins the core's current selection, which follows a pending server transfer.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<ConnectTarget>,
    /// Whether the client resolves blob-cache chunks.
    #[serde(default)]
    pub client_cache: bool,
    /// Bedrock login client-data claims, keyed as the login JWT names them. Their `ThirdPartyName`
    /// names an offline login; a signed-in core joins as its account.
    pub client_data: D,
}

/// Returns the Connect frame for `request`.
pub fn encode_connect(request: &ConnectRequest) -> Result<Bytes, BridgeError> {
    let mut frame = BytesMut::new().writer();
    frame.get_mut().put_u8(KIND_CONNECT);
    serde_json::to_writer(&mut frame, request).map_err(BridgeError::SessionJson)?;
    Ok(frame.into_inner().freeze())
}

/// Returns a Batch frame holding `packets`, each an encoded packet with its header, as one network batch.
pub fn encode_batch<P: AsRef<[u8]>>(
    packets: impl IntoIterator<Item = P>,
) -> Result<Bytes, BridgeError> {
    let mut frame = BytesMut::new();
    frame.put_u8(KIND_BATCH);
    for packet in packets {
        let packet = packet.as_ref();
        let length = u32::try_from(packet.len())
            .ok()
            .filter(|length| *length != 0)
            .ok_or(invalid("batch packet length"))?;
        put_varuint32(&mut frame, length);
        frame.extend_from_slice(packet);
    }
    if frame.len() == 1 {
        return Err(invalid("empty batch"));
    }
    Ok(frame.freeze())
}

/// The first byte of a Bedrock game batch as RakNet frames it; Batch frames replace it with their kind.
const BEDROCK_BATCH_PREFIX: u8 = 0xfe;

/// Returns the Batch frame for an uncompressed Bedrock batch framed as `0xfe` then length-prefixed packets.
pub fn batch_frame_from_bedrock(batch: &[u8]) -> Result<Bytes, BridgeError> {
    match batch.split_first() {
        Some((&BEDROCK_BATCH_PREFIX, body)) if !body.is_empty() => {
            let mut frame = BytesMut::with_capacity(batch.len());
            frame.put_u8(KIND_BATCH);
            frame.extend_from_slice(body);
            Ok(frame.freeze())
        }
        _ => Err(invalid("not an uncompressed Bedrock batch")),
    }
}

/// Returns a Batch frame's body, its length-prefixed packets, without splitting it; `None` for another kind.
#[must_use]
pub fn batch_frame_body(frame: &Bytes) -> Option<Bytes> {
    (frame.first() == Some(&KIND_BATCH)).then(|| frame.slice(1..))
}

/// A message from the core.
#[derive(Debug)]
pub enum CoreMessage {
    /// Starts the session; the archives of its packs follow before any batch.
    Handoff(SessionHandoff),
    /// The next bytes of the archive at `index` in [`SessionHandoff::packs`].
    PackData { index: u32, data: Bytes },
    /// One network batch of encoded packets, in order.
    Batch(Vec<Bytes>),
    /// The server transferred the player; reconnect with a targetless Connect to follow it.
    Transfer(SessionTransfer),
    /// The session ended with the server's reason or a join-failure lang key.
    Disconnect(SessionDisconnect),
}

/// What the client needs before play. Builders set what they need and default the rest, so new
/// optional fields never break them.
#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SessionHandoff {
    /// Web join identity whose archives arrive on a separate reliable stream.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pack_stream: Option<u64>,
    /// The upstream login's canonical identity.
    pub identity: SessionIdentity,
    /// Whether the upstream login advertised blob-cache support.
    pub client_cache: bool,
    /// Whether the offer or the stack required the packs; a client that cannot apply one must leave.
    pub packs_required: bool,
    /// Archives to apply, in application order.
    pub packs: Vec<HandoffPack>,
    /// The core's latency-trace ID for this connection, stamped on both processes' records; 0
    /// when the core is not tracing.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub trace_conn: u64,
    /// Every packet the core received through StartGame, in order; StartGame is last.
    #[serde(skip)]
    pub startup: Vec<Bytes>,
}

/// The player identity the server knows.
#[derive(Clone, Debug, Default, Deserialize, Serialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SessionIdentity {
    pub display_name: String,
    pub xuid: String,
    pub uuid: String,
}

/// One selected pack, read from the cache or received in [`CoreMessage::PackData`] frames.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HandoffPack {
    /// SHA-256 of the complete compressed archive; mandatory for web delivery.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<[u8; 32]>,
    pub uuid: String,
    pub version: String,
    pub sub_pack: String,
    pub content_key: PackContentKey,
    /// Archive bytes.
    pub size: u64,
    /// Absent when the core streams this archive instead.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache: Option<CachedArchive>,
}

/// A pack's content key, redacted from `Debug` and zeroized on drop.
#[derive(Clone, Deserialize, Serialize, Eq, PartialEq)]
#[serde(transparent)]
pub struct PackContentKey(String);

impl Drop for PackContentKey {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

impl PackContentKey {
    /// Retains an archive key, redacting it in diagnostics and erasing it on drop.
    pub fn new(key: String) -> Self {
        Self(key)
    }

    /// Borrows the key; an empty key means the archive is not encrypted.
    #[must_use]
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for PackContentKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("PackContentKey(<redacted>)")
    }
}

/// A server transfer target.
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SessionTransfer {
    pub address: String,
    pub port: u16,
    pub reload_world: bool,
}

/// Why the session ended.
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SessionDisconnect {
    pub reason: i32,
    pub message: String,
    pub filtered_message: String,
    pub hide_screen: bool,
}

/// Decodes one frame from the core, rejecting unknown kinds and malformed bodies.
pub fn decode_core_message(frame: Bytes) -> Result<CoreMessage, BridgeError> {
    let Some(&kind) = frame.first() else {
        return Err(invalid("empty frame"));
    };
    let body = frame.slice(1..);
    match kind {
        KIND_HANDOFF => decode_handoff(body).map(CoreMessage::Handoff),
        KIND_PACK_DATA => {
            if body.len() <= 4 {
                return Err(invalid("pack data without bytes"));
            }
            let index = u32::from_be_bytes(body[..4].try_into().expect("four-byte index"));
            Ok(CoreMessage::PackData {
                index,
                data: body.slice(4..),
            })
        }
        KIND_BATCH => split_batch(&body).map(CoreMessage::Batch),
        KIND_TRANSFER => json(&body).map(CoreMessage::Transfer),
        KIND_DISCONNECT => json(&body).map(CoreMessage::Disconnect),
        _ => Err(invalid("unknown message kind")),
    }
}

fn decode_handoff(body: Bytes) -> Result<SessionHandoff, BridgeError> {
    if body.len() < 4 {
        return Err(invalid("handoff without metadata length"));
    }
    let length = u32::from_be_bytes(body[..4].try_into().expect("four-byte length")) as usize;
    let Some(rest) = body.len().checked_sub(4).filter(|rest| *rest >= length) else {
        return Err(invalid("handoff metadata length"));
    };
    let mut handoff: SessionHandoff = json(&body[4..4 + length])?;
    if rest == length {
        return Err(invalid("handoff without StartGame"));
    }
    handoff.startup = split_batch(&body.slice(4 + length..))?;
    let last = handoff.startup.last().expect("a batch holds a packet");
    if read_varuint32(last).is_none_or(|(header, _)| header & 0x3ff != START_GAME_PACKET_ID) {
        return Err(invalid("handoff startup does not end with StartGame"));
    }
    Ok(handoff)
}

fn json<T: for<'de> Deserialize<'de>>(body: &[u8]) -> Result<T, BridgeError> {
    serde_json::from_slice(body).map_err(BridgeError::SessionJson)
}

/// Splits a batch body into its packets without copying.
fn split_batch(body: &Bytes) -> Result<Vec<Bytes>, BridgeError> {
    let mut packets = Vec::new();
    let mut offset = 0;
    while offset < body.len() {
        let (length, read) =
            read_varuint32(&body[offset..]).ok_or(invalid("batch packet length"))?;
        offset += read;
        let length = length as usize;
        if length == 0 || length > body.len() - offset {
            return Err(invalid("batch packet length"));
        }
        packets.push(body.slice(offset..offset + length));
        offset += length;
    }
    if packets.is_empty() {
        return Err(invalid("empty batch"));
    }
    Ok(packets)
}

fn put_varuint32(buffer: &mut BytesMut, mut value: u32) {
    while value >= 0x80 {
        buffer.put_u8((value as u8) | 0x80);
        value >>= 7;
    }
    buffer.put_u8(value as u8);
}

/// Reads a varuint32, returning it and the bytes it used.
fn read_varuint32(bytes: &[u8]) -> Option<(u32, usize)> {
    let mut value = 0u32;
    for (index, &byte) in bytes.iter().take(5).enumerate() {
        let bits = u32::from(byte & 0x7f);
        if index == 4 && bits > 0x0f {
            return None;
        }
        value |= bits << (7 * index);
        if byte & 0x80 == 0 {
            return Some((value, index + 1));
        }
    }
    None
}

fn invalid(reason: &'static str) -> BridgeError {
    BridgeError::InvalidSessionMessage { reason }
}

#[cfg(test)]
mod tests;

/// Whether an optional numeric field holds its absent value.
fn is_zero(value: &u64) -> bool {
    *value == 0
}
