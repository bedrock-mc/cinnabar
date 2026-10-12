use bedrock_protocol_1_26_51::packets::ShowCredits;
use bedrock_protocol_1_26_51::types::ActorRuntimeID;
use bedrock_protocol_1_26_51::wire;

/// The server asks this runtime actor to watch the End poem and credits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShowCreditsEvent {
    pub runtime_id: u64,
}

pub(crate) fn normalize(packet: &ShowCredits) -> Option<ShowCreditsEvent> {
    (packet.credits_state.0 == 0).then_some(ShowCreditsEvent {
        runtime_id: packet.player_runtime_id.0,
    })
}

/// Finishes the credits for the addressed player; it does not acknowledge a dimension switch.
#[must_use]
pub fn credits_finished_packet(runtime_id: u64) -> crate::Packet {
    crate::Packet::new(ShowCredits {
        player_runtime_id: ActorRuntimeID(runtime_id),
        credits_state: wire::ZigZag32(1),
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn completion_round_trips_the_actor_and_finished_state() {
        use bedrock_protocol_1_26_51::packets;
        use bedrock_protocol_1_26_51::wire::{Decode, Encode};
        let packet = super::credits_finished_packet(u64::MAX);
        let packets::Packet::ShowCredits(packet) = *packet.data else {
            panic!("wrong response packet")
        };
        let bytes = packet.encode_to_vec().unwrap();
        let packet = super::ShowCredits::decode_exact(&bytes).unwrap();
        assert_eq!(packet.player_runtime_id.0, u64::MAX);
        assert_eq!(packet.credits_state.0, 1);
    }
}
