use valentine::bedrock::version::v1_26_51::{
    ActorDataBoundingBoxComponent, ActorDataFlagComponent, ActorUniqueId,
    ClientMovementPredictionSyncPacket,
};

use crate::Packet;

/// Client-predicted movement facts the server reads after it corrected the client.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MovementPredictionSync {
    /// Actor data flag bitset; bit `n` is flag `n`.
    pub actor_flags: [u64; 3],
    /// Bounding box scale, width, height.
    pub bounding_box: [f32; 3],
    /// Movement speed, underwater speed, lava speed, jump strength, health, hunger,
    /// friction modifier, bounciness, air drag modifier; 0 when the attribute is unset.
    pub attributes: [f32; 9],
    pub unique_id: i64,
    pub flying: bool,
}

/// Encodes a prediction sync; non-finite floats are sent as zero rather than failing.
#[must_use]
pub fn client_movement_prediction_sync(sync: MovementPredictionSync) -> Packet {
    let finite = |value: f32| if value.is_finite() { value } else { 0.0 };
    crate::Packet::new(ClientMovementPredictionSyncPacket {
        actor_data_flag: ActorDataFlagComponent {
            actor_flag_bitset_data: sync.actor_flags,
        },
        actor_bounding_box: ActorDataBoundingBoxComponent {
            actor_data_bounding_box: sync.bounding_box.map(finite),
        },
        movement_attributes: sync.attributes.map(finite),
        actor_unique_id: ActorUniqueId {
            actor_unique_id: sync.unique_id,
        },
        actor_flying_state: sync.flying,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packet_carries_fields_and_zeroes_non_finite_floats() {
        let packet = client_movement_prediction_sync(MovementPredictionSync {
            actor_flags: [1 << 3, 0, 0],
            bounding_box: [1.0, 0.6, f32::NAN],
            attributes: [0.1, 0.0, 0.0, 0.0, 20.0, 20.0, 1.0, 0.0, 1.0],
            unique_id: 7,
            flying: true,
        });
        let valentine::bedrock::version::v1_26_51::McpePacketData::ClientMovementPredictionSyncPacket(
            body,
        ) = packet.data
        else {
            panic!("wrong packet");
        };
        assert_eq!(
            body.actor_bounding_box.actor_data_bounding_box,
            [1.0, 0.6, 0.0]
        );
        assert_eq!(body.actor_unique_id.actor_unique_id, 7);
        assert!(body.actor_flying_state);
        assert_eq!(body.actor_data_flag.actor_flag_bitset_data[0], 1 << 3);
    }
}
