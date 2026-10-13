use bedrock_protocol_1_26_51::packets::ShowCredits;
use bedrock_protocol_1_26_51::types::ActorRuntimeID;
use bedrock_protocol_1_26_51::wire;
use protocol::{WorldEvent, into_world_event};

#[test]
fn credits_start_reaches_the_ordered_ui_surface() {
    let packet = protocol::Packet::new(ShowCredits {
        player_runtime_id: ActorRuntimeID(71),
        credits_state: wire::ZigZag32(0),
    });
    assert!(matches!(
        into_world_event(packet, 2).unwrap(),
        Some(WorldEvent::Ui(_))
    ));
}

#[test]
fn unsolicited_finished_and_unknown_credits_states_are_ignored() {
    for credits_state in [1, -1, i32::MAX] {
        let packet = protocol::Packet::new(ShowCredits {
            player_runtime_id: ActorRuntimeID(71),
            credits_state: wire::ZigZag32(credits_state),
        });
        assert!(into_world_event(packet, 2).unwrap().is_none());
    }
}
