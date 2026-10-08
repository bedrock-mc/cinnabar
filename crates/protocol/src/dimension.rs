//! Native dimension-transfer acknowledgements and correlated loading screens.

use valentine::bedrock::version::v1_26_51::{
    ActorRuntimeId, EnumsPlayerActionType, EnumsServerboundLoadingScreenPacketType,
    PlayerActionPacket, ServerboundLoadingScreenPacket,
};

use crate::{Packet, WorldEvent};

pub(crate) fn normalize_ack(
    action: &PlayerActionPacket,
    from_subclient: u32,
    to_subclient: u32,
) -> Option<WorldEvent> {
    (action.action == EnumsPlayerActionType::Changedimensionack
        && from_subclient == 0
        && to_subclient == 0)
        .then_some(WorldEvent::DimensionChangeAck {
            runtime_id: action.player_runtime_id.actor_runtime_id,
        })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadingScreenPhase {
    Start,
    End,
}

/// Echoes the optional ID carried by ChangeDimension, including an explicit zero.
#[must_use]
pub fn loading_screen_packet(phase: LoadingScreenPhase, loading_screen_id: Option<u32>) -> Packet {
    ServerboundLoadingScreenPacket {
        loading_screen_packet_type: match phase {
            LoadingScreenPhase::Start => {
                EnumsServerboundLoadingScreenPacketType::Startloadingscreen
            }
            LoadingScreenPhase::End => EnumsServerboundLoadingScreenPacketType::Endloadingscreen,
        },
        loading_screen_id,
    }
    .into()
}

/// Completes a transfer with zero block positions and face; initialization belongs to login.
#[must_use]
pub fn dimension_change_done_packet(local_runtime_id: u64) -> Packet {
    PlayerActionPacket {
        player_runtime_id: ActorRuntimeId {
            actor_runtime_id: local_runtime_id,
        },
        action: EnumsPlayerActionType::Changedimensionack,
        ..Default::default()
    }
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use valentine::bedrock::version::v1_26_51::{BlockPos, McpePacketData};

    #[test]
    fn loading_messages_preserve_absent_zero_and_nonzero_ids() {
        for id in [None, Some(0), Some(u32::MAX)] {
            for phase in [LoadingScreenPhase::Start, LoadingScreenPhase::End] {
                let McpePacketData::ServerboundLoadingScreenPacket(packet) =
                    loading_screen_packet(phase, id).data
                else {
                    panic!("expected loading screen packet");
                };
                assert_eq!(packet.loading_screen_id, id);
                assert_eq!(
                    packet.loading_screen_packet_type,
                    if phase == LoadingScreenPhase::Start {
                        EnumsServerboundLoadingScreenPacketType::Startloadingscreen
                    } else {
                        EnumsServerboundLoadingScreenPacketType::Endloadingscreen
                    }
                );
            }
        }
    }

    #[test]
    fn dimension_done_has_native_actor_and_zero_action_payload() {
        let McpePacketData::PlayerActionPacket(packet) = dimension_change_done_packet(77).data
        else {
            panic!("expected player action packet");
        };
        assert_eq!(packet.player_runtime_id.actor_runtime_id, 77);
        assert_eq!(packet.action, EnumsPlayerActionType::Changedimensionack);
        assert_eq!(packet.block_position, BlockPos::default());
        assert_eq!(packet.result_pos, BlockPos::default());
        assert_eq!(packet.face, 0);
    }
}
