//! Emote starts are announced with their registered animation duration.
use bedrock_protocol_1_26_51::packets::Emote;
use bedrock_protocol_1_26_51::types::ActorRuntimeID;
use bedrock_protocol_1_26_51::wire;

/// Announces the selected clip without substituting another clip's identity.
#[must_use]
pub fn emote_packet(runtime_id: u64, id: &str, length_ticks: u32) -> crate::Packet {
    crate::Packet::new(Emote {
        actor_runtime_id: ActorRuntimeID(runtime_id),
        emote_id: id.into(),
        emote_length_ticks: wire::VarUInt(length_ticks),
        xuid: wire::Str::default(),
        platform_id: wire::Str::default(),
        flags: wire::U8(0),
    })
}
