//! The optional Cinnabar carrier uses the existing ScriptMessage packet.

use bedrock_protocol_1_26_51::packets::{self, ScriptMessage};

mod control;
use control::ExperienceControl;

pub const EXPERIENCE_CHANNEL: &str = "cinnabar:extensions/v1";
pub const MAX_EXPERIENCE_ENVELOPE_BYTES: usize = 64 * 1024;
/// Bounds the separate world-notice lane, including its JSON framing.
pub const MAX_EXPERIENCE_CONTROL_BYTES: usize = 256;

/// Inert until the application has admitted an offer and obtained consent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExperienceMessage {
    bytes: Vec<u8>,
    control: Option<ExperienceControl>,
}

impl ExperienceMessage {
    /// Classifies control traffic once, before either admission budget sees it.
    pub fn new(bytes: Vec<u8>) -> Self {
        let control = serde_json::from_slice(&bytes).ok();
        Self { bytes, control }
    }

    /// Returns the original carrier bytes for data and handshake validation.
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Returns a decoded world notice without parsing its carrier again.
    pub fn world_notice(&self) -> Option<(&str, u64)> {
        match &self.control {
            Some(ExperienceControl::Epoch {
                session,
                world_epoch,
            }) => Some((session, *world_epoch)),
            _ => None,
        }
    }
}

/// Drops unrelated and oversized optional data without failing vanilla play.
pub(crate) fn normalize(packet: ScriptMessage) -> Option<ExperienceMessage> {
    (packet.message_id == EXPERIENCE_CHANNEL
        && packet.message_value.len() <= MAX_EXPERIENCE_ENVELOPE_BYTES)
        .then(|| ExperienceMessage::new(packet.message_value.to_vec()))
}

/// Encodes a bounded envelope without defining a new Bedrock packet ID.
pub fn experience_packet(bytes: Vec<u8>) -> Option<crate::Packet> {
    if bytes.len() > MAX_EXPERIENCE_ENVELOPE_BYTES {
        return None;
    }
    Some(crate::Packet::new(ScriptMessage {
        message_id: EXPERIENCE_CHANNEL.into(),
        message_value: bytes.into(),
    }))
}

/// Identifies only this optional carrier before an authorized socket write.
pub fn is_experience_packet(packet: &crate::Packet) -> bool {
    matches!(&*packet.data, packets::Packet::ScriptMessage(message)
        if message.message_id == EXPERIENCE_CHANNEL)
}

/// Identifies the notice acknowledged by an outbound control packet.
pub fn experience_world_ack(packet: &crate::Packet) -> Option<u64> {
    let packets::Packet::ScriptMessage(message) = &*packet.data else {
        return None;
    };
    if message.message_id != EXPERIENCE_CHANNEL {
        return None;
    }
    match serde_json::from_slice::<ExperienceControl>(&message.message_value).ok()? {
        ExperienceControl::WorldAck {
            requested_epoch, ..
        } => Some(requested_epoch),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unrelated_scripts_are_inert() {
        assert!(
            normalize(ScriptMessage {
                message_id: "minecraft:unrelated".into(),
                message_value: vec![1, 2, 3].into(),
            })
            .is_none()
        );
    }

    #[test]
    fn carrier_round_trip_uses_generated_packet() {
        let bytes = b"fixture".to_vec();
        let event = crate::into_world_event(experience_packet(bytes.clone()).unwrap(), 0).unwrap();
        assert_eq!(
            event,
            Some(crate::WorldEvent::Experience(ExperienceMessage::new(bytes)))
        );
        assert!(experience_packet(vec![0; MAX_EXPERIENCE_ENVELOPE_BYTES + 1]).is_none());
    }

    #[test]
    fn extension_does_not_accept_another_subclient_route() {
        let mut packet = experience_packet(Vec::new()).unwrap();
        packet.header.from_subclient = 1;
        assert!(crate::into_world_event(packet, 0).unwrap().is_none());
        let mut packet = experience_packet(Vec::new()).unwrap();
        packet.header.to_subclient = 1;
        assert!(crate::into_world_event(packet, 0).unwrap().is_none());
    }
}
