use super::*;

#[test]
fn server_animation_preserves_transition_expression_version_and_blend() {
    let packet = AnimateEntityPacket {
        m_animation: "animation.fixture.kill".into(),
        m_controller: "fixture.server".into(),
        m_next_state: "default".into(),
        m_stop_expression: "query.all_animations_finished".into(),
        m_stop_expression_version: 13,
        m_blend_out_time: 0.25,
        m_runtime_ids: vec![ActorRuntimeId {
            actor_runtime_id: 42,
        }],
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
