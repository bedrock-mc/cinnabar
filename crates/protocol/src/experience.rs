//! The optional Cinnabar carrier uses the existing ScriptMessage packet.

use valentine::bedrock::version::v1_26_51::{McpePacketData, ScriptMessagePacket};

pub const EXPERIENCE_CHANNEL: &str = "cinnabar:extensions/v1";
pub const MAX_EXPERIENCE_ENVELOPE_BYTES: usize = 64 * 1024;

/// Inert until the application has admitted an offer and obtained consent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExperienceMessage {
    pub bytes: Vec<u8>,
}

/// Drops unrelated and oversized optional data without failing vanilla play.
pub(crate) fn normalize(packet: ScriptMessagePacket) -> Option<ExperienceMessage> {
    (packet.message_id == EXPERIENCE_CHANNEL
        && packet.message_value.len() <= MAX_EXPERIENCE_ENVELOPE_BYTES)
        .then_some(ExperienceMessage {
            bytes: packet.message_value,
        })
}

/// Encodes a bounded envelope without defining a new Bedrock packet ID.
pub fn experience_packet(bytes: Vec<u8>) -> Option<crate::Packet> {
    if bytes.len() > MAX_EXPERIENCE_ENVELOPE_BYTES {
        return None;
    }
    Some(
        ScriptMessagePacket {
            message_id: EXPERIENCE_CHANNEL.to_owned(),
            message_value: bytes,
        }
        .into(),
    )
}

/// Identifies only this optional carrier before an authorized socket write.
pub fn is_experience_packet(packet: &crate::Packet) -> bool {
    matches!(&packet.data, McpePacketData::ScriptMessagePacket(message)
        if message.message_id == EXPERIENCE_CHANNEL)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unrelated_scripts_are_inert() {
        assert!(
            normalize(ScriptMessagePacket {
                message_id: "minecraft:unrelated".into(),
                message_value: vec![1, 2, 3],
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
            Some(crate::WorldEvent::Experience(ExperienceMessage { bytes }))
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
