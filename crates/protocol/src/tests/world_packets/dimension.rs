use super::*;

#[test]
fn dimension_ack_is_selected_by_subclient_without_requiring_the_action_actor_id() {
    use bedrock_protocol_1_26_51::enums::PlayerActionType;
    use bedrock_protocol_1_26_51::packets::PlayerAction;
    for runtime_id in [0, 42, u64::MAX] {
        let action = PlayerAction {
            player_runtime_id: ActorRuntimeID(runtime_id),
            action: PlayerActionType::Changedimensionack,
            ..Default::default()
        };
        let packet: protocol::Packet = protocol::Packet::new(action);
        assert_eq!(
            into_world_event(packet.clone(), 1).unwrap(),
            Some(WorldEvent::DimensionChangeAck { runtime_id })
        );
        let mut other_subclient = packet.clone();
        other_subclient.header.to_subclient = 1;
        assert!(into_world_event(other_subclient, 1).unwrap().is_none());
        let mut other_subclient = packet;
        other_subclient.header.from_subclient = 1;
        assert!(into_world_event(other_subclient, 1).unwrap().is_none());
    }
}
