use super::*;

#[test]
fn server_swings_for_the_local_player_reach_its_swing_tracker() {
    let mut store = ActorStore::new(1, 0);
    store.exclude_remote_state_for(42);
    store.apply(1, 1, spawn(42, -7));
    store.apply_item_actor(
        1,
        2,
        protocol::ItemActorEvent::Action(protocol::ActorActionEvent {
            actor_runtime_ids: Arc::from([42_u64]),
            kind: protocol::ActorActionKind::SwingArm,
            data: 0.0,
            swing_source: None,
        }),
    );
    assert_eq!(
        store.take_local_server_swings(),
        vec![protocol::SwingSource::None]
    );
    assert_eq!(
        store.take_local_server_swings(),
        Vec::<protocol::SwingSource>::new(),
        "each swing is taken once"
    );
}

#[test]
fn dimension_reset_discards_only_the_previous_dimensions_server_swings() {
    let mut store = ActorStore::new(1, 0);
    store.exclude_remote_state_for(42);
    store.apply(1, 1, spawn(42, -7));
    let action = protocol::ItemActorEvent::Action(protocol::ActorActionEvent {
        actor_runtime_ids: Arc::from([42_u64]),
        kind: protocol::ActorActionKind::SwingArm,
        data: 0.0,
        swing_source: None,
    });
    store.apply_item_actor(1, 2, action.clone());
    assert_eq!(store.reset_dimension(1, 3, 1), ActorApplyResult::Reset);
    assert_eq!(
        store.take_local_server_swings(),
        Vec::<protocol::SwingSource>::new()
    );
    store.apply_item_actor(1, 4, action);
    assert_eq!(
        store.take_local_server_swings(),
        vec![protocol::SwingSource::None]
    );
}

#[test]
fn local_server_callbacks_preserve_sources_and_do_not_saturate_at_a_byte() {
    let mut store = ActorStore::new(1, 0);
    store.exclude_remote_state_for(42);
    for tick in 1..=300 {
        store.apply_item_actor(
            1,
            tick,
            protocol::ItemActorEvent::Action(protocol::ActorActionEvent {
                actor_runtime_ids: Arc::from([42_u64]),
                kind: protocol::ActorActionKind::SwingArm,
                data: 0.0,
                swing_source: Some(Arc::from(if tick % 2 == 0 { "attack" } else { "event" })),
            }),
        );
    }
    let sources = store.take_local_server_swings();
    assert_eq!(sources.len(), 300);
    for (index, source) in sources.into_iter().enumerate() {
        assert_eq!(
            source,
            if index % 2 == 0 {
                protocol::SwingSource::Event
            } else {
                protocol::SwingSource::Attack
            }
        );
    }
}
