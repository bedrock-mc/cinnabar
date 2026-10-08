//! Emote starts are announced with their registered animation duration.
use valentine::bedrock::version::v1_26_51::{ActorRuntimeId, EmotePacket};

/// Announces the selected clip without substituting another clip's identity.
#[must_use]
pub fn emote_packet(runtime_id: u64, id: &str, length_ticks: u32) -> crate::Packet {
    EmotePacket {
        actor_runtime_id: ActorRuntimeId {
            actor_runtime_id: runtime_id,
        },
        emote_id: id.to_owned(),
        emote_length_ticks: length_ticks,
        xuid: String::new(),
        platform_id: String::new(),
        flags: 0,
    }
    .into()
}
