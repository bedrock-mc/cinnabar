//! Native dimension-transfer acknowledgements and correlated loading screens.

use bedrock_protocol_1_26_51::enums::{PlayerActionType, ServerboundLoadingScreenType};
use bedrock_protocol_1_26_51::packets::{PlayerAction, ServerboundLoadingScreen};
use bedrock_protocol_1_26_51::types::ActorRuntimeID;
use bedrock_protocol_1_26_51::wire;

use crate::{Packet, WorldEvent};

pub(crate) fn normalize_ack(
    action: &PlayerAction,
    from_subclient: u32,
    to_subclient: u32,
) -> Option<WorldEvent> {
    (action.action == PlayerActionType::Changedimensionack
        && from_subclient == 0
        && to_subclient == 0)
        .then_some(WorldEvent::DimensionChangeAck {
            runtime_id: action.player_runtime_id.0,
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
    crate::Packet::new(ServerboundLoadingScreen {
        loading_screen_packet_type: match phase {
            LoadingScreenPhase::Start => ServerboundLoadingScreenType::Startloadingscreen,
            LoadingScreenPhase::End => ServerboundLoadingScreenType::Endloadingscreen,
        },
        loading_screen_id: loading_screen_id.map(wire::U32LE),
    })
}

/// Completes a transfer with zero block positions and face; initialization belongs to login.
#[must_use]
pub fn dimension_change_done_packet(local_runtime_id: u64) -> Packet {
    crate::Packet::new(PlayerAction {
        player_runtime_id: ActorRuntimeID(local_runtime_id),
        action: PlayerActionType::Changedimensionack,
        ..Default::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use bedrock_protocol_1_26_51::packets;
    use bedrock_protocol_1_26_51::types::BlockPos;

    #[test]
    fn loading_messages_preserve_absent_zero_and_nonzero_ids() {
        for id in [None, Some(0), Some(u32::MAX)] {
            for phase in [LoadingScreenPhase::Start, LoadingScreenPhase::End] {
                let packets::Packet::ServerboundLoadingScreen(packet) =
                    *loading_screen_packet(phase, id).data
                else {
                    panic!("expected loading screen packet");
                };
                assert_eq!(packet.loading_screen_id.map(|id| id.0), id);
                assert_eq!(
                    packet.loading_screen_packet_type,
                    if phase == LoadingScreenPhase::Start {
                        ServerboundLoadingScreenType::Startloadingscreen
                    } else {
                        ServerboundLoadingScreenType::Endloadingscreen
                    }
                );
            }
        }
    }

    #[test]
    fn dimension_done_has_native_actor_and_zero_action_payload() {
        let packets::Packet::PlayerAction(packet) = *dimension_change_done_packet(77).data else {
            panic!("expected player action packet");
        };
        assert_eq!(packet.player_runtime_id.0, 77);
        assert_eq!(packet.action, PlayerActionType::Changedimensionack);
        assert_eq!(packet.block_position, BlockPos::default());
        assert_eq!(packet.result_pos, BlockPos::default());
        assert_eq!(packet.face.0, 0);
    }
}
