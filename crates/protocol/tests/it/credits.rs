use protocol::{WorldEvent, into_world_event};
use valentine::bedrock::version::v1_26_51::{ActorRuntimeId, ShowCreditsPacket};

#[test]
fn credits_start_reaches_the_ordered_ui_surface() {
    let packet = ShowCreditsPacket {
        player_runtime_id: ActorRuntimeId {
            actor_runtime_id: 71,
        },
        credits_state: 0,
    }
    .into();
    assert!(matches!(
        into_world_event(packet, 2).unwrap(),
        Some(WorldEvent::Ui(_))
    ));
}

#[test]
fn unsolicited_finished_and_unknown_credits_states_are_ignored() {
    for credits_state in [1, -1, i32::MAX] {
        let packet = ShowCreditsPacket {
            player_runtime_id: ActorRuntimeId {
                actor_runtime_id: 71,
            },
            credits_state,
        }
        .into();
        assert!(into_world_event(packet, 2).unwrap().is_none());
    }
}
