use valentine::bedrock::version::v1_26_51::{ActorRuntimeId, ShowCreditsPacket};

/// The server asks this runtime actor to watch the End poem and credits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShowCreditsEvent {
    pub runtime_id: u64,
}

pub(crate) fn normalize(packet: &ShowCreditsPacket) -> Option<ShowCreditsEvent> {
    (packet.credits_state == 0).then_some(ShowCreditsEvent {
        runtime_id: packet.player_runtime_id.actor_runtime_id,
    })
}

/// Finishes the credits for the addressed player; it does not acknowledge a dimension switch.
#[must_use]
pub fn credits_finished_packet(runtime_id: u64) -> crate::Packet {
    ShowCreditsPacket {
        player_runtime_id: ActorRuntimeId {
            actor_runtime_id: runtime_id,
        },
        credits_state: 1,
    }
    .into()
}

#[cfg(test)]
mod tests {
    #[test]
    fn completion_round_trips_the_actor_and_finished_state() {
        use valentine::bedrock::codec::BedrockCodec;
        let packet = super::credits_finished_packet(u64::MAX);
        let valentine::bedrock::version::v1_26_51::McpePacketData::ShowCreditsPacket(packet) =
            packet.data
        else {
            panic!("wrong response packet")
        };
        let mut bytes = Vec::new();
        packet.encode(&mut bytes).unwrap();
        let packet = super::ShowCreditsPacket::decode(&mut bytes.as_slice(), ()).unwrap();
        assert_eq!(packet.player_runtime_id.actor_runtime_id, u64::MAX);
        assert_eq!(packet.credits_state, 1);
    }
}
