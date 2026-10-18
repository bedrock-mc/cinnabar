use super::*;

#[test]
fn item_use_progress_preserves_the_negotiated_item_payload_without_a_position() {
    for (item_id, auxiliary) in [(300, 5), (-32768, 0), (12, -1)] {
        let packet = item_use_progress_packet(42, item_id, auxiliary);
        let bedrock_protocol_1_26_51::packets::Packet::ActorEvent(event) = &*packet.data else {
            panic!("expected actor event");
        };
        assert_eq!(event.target_runtime_id.0, 42);
        assert_eq!(
            event.event_id,
            bedrock_protocol_1_26_51::enums::ActorEventType::Feed
        );
        let expected = match (item_id, auxiliary) {
            (300, 5) => 19_660_805,
            (-32768, 0) => i32::MIN,
            _ => -1,
        };
        assert_eq!(event.data.0, expected);
        assert!(event.fire_at_position.is_none());
    }
}

#[test]
fn server_animation_preserves_transition_expression_version_and_blend() {
    let packet = AnimateEntityPacket {
        m_animation: "animation.fixture.kill".into(),
        m_controller: "fixture.server".into(),
        m_next_state: "default".into(),
        m_stop_expression: "query.all_animations_finished".into(),
        m_stop_expression_version: wire::I32LE(13),
        m_blend_out_time: wire::F32LE(0.25),
        m_runtime_ids: vec![ActorRuntimeID(42)],
    };
    let ItemActorEvent::Action(event) = normalize_animate_entity(packet).unwrap() else {
        panic!("expected animation action");
    };
    let ActorActionKind::Custom {
        animation,
        controller,
        next_state,
        stop_expression,
        stop_expression_version,
    } = event.kind
    else {
        panic!("expected custom animation");
    };
    assert_eq!(animation.as_ref(), "animation.fixture.kill");
    assert_eq!(controller.as_ref(), "fixture.server");
    assert_eq!(next_state.as_ref(), "default");
    assert_eq!(stop_expression.as_ref(), "query.all_animations_finished");
    assert_eq!(stop_expression_version, 13);
    assert_eq!(event.data, 0.25);
    assert_eq!(event.actor_runtime_ids.as_ref(), &[42]);
}
