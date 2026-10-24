use super::*;
use crate::actor_store::{ActorApplyResult, ActorStore};
use protocol::{
    ActorEvent, ActorMetadata, ActorMetadataUpdateEvent, ActorRemoveEvent, ActorStatusEvent,
    ActorStatusKind,
};

/// Admits one ordinary player with a visible name and unrelated burning flag.
fn player() -> ActorStore {
    let ActorEvent::Spawn(mut spawn) = crate::actor_store::tests::spawn(7, 5) else {
        unreachable!()
    };
    spawn.kind = ActorKind::Player {
        uuid: [7; 16],
        username: "Fixture".into(),
    };
    let mut store = ActorStore::new(1, 0);
    store.apply(1, 1, ActorEvent::Spawn(spawn));
    flags(&mut store, (1 << ActorFlag::ShowName.wire_value()) | 1);
    store
}

/// Delivers a primary server flag word through metadata admission.
fn flags(store: &mut ActorStore, flags: u64) {
    store.apply(
        1,
        store.latest_sequence + 1,
        ActorEvent::Metadata(ActorMetadataUpdateEvent {
            dimension: 0,
            runtime_id: 7,
            metadata: [ActorMetadata {
                key: ActorDataKey::Flags.wire_value(),
                value: ActorMetadataValue::Flags(flags),
            }]
            .into(),
            properties: [].into(),
            tick: 0,
        }),
    );
}

/// Delivers a life-state event without changing the actor's metadata or identity.
fn event(store: &mut ActorStore, kind: ActorStatusKind) {
    store.apply(
        1,
        store.latest_sequence + 1,
        ActorEvent::Status(ActorStatusEvent {
            runtime_id: 7,
            kind,
            data: 0,
        }),
    );
}

#[test]
fn player_death_stays_visible_through_twenty_ticks_then_hides_without_removal() {
    let mut store = player();
    let revision = store.get(7).unwrap().spawn_revision;
    event(&mut store, ActorStatusKind::Death);
    for _ in 0..DEATH_DURATION_TICKS {
        store.advance_interpolation_ticks(1);
        assert!(!store.get(7).unwrap().is_invisible());
    }
    store.advance_interpolation_ticks(0);
    assert!(!store.get(7).unwrap().is_invisible());
    store.advance_interpolation_ticks(1);
    let actor = store.get(7).unwrap();
    assert!(actor.is_invisible());
    assert!(!actor.flag(ActorFlag::ShowName.wire_value()));
    assert!(actor.is_on_fire());
    assert_eq!(actor.spawn_revision, revision);
    store.advance_interpolation_ticks(200);
    assert!(store.get(7).unwrap().is_invisible());
}

#[test]
fn metadata_can_reveal_a_completed_death_without_the_timer_hiding_it_again() {
    let mut store = player();
    event(&mut store, ActorStatusKind::Death);
    store.advance_interpolation_ticks(u32::from(DEATH_DURATION_TICKS) + 1);
    assert!(store.get(7).unwrap().is_invisible());
    flags(&mut store, 1 << ActorFlag::ShowName.wire_value());
    store.advance_interpolation_ticks(50);
    let actor = store.get(7).unwrap();
    assert!(actor.status.dead);
    assert!(!actor.is_invisible());
    assert!(actor.flag(ActorFlag::ShowName.wire_value()));
}

#[test]
fn revival_preserves_visibility_metadata_and_rearms_the_next_death() {
    let mut store = player();
    event(&mut store, ActorStatusKind::Death);
    store.advance_interpolation_ticks(21);
    assert!(store.get(7).unwrap().is_invisible());
    event(&mut store, ActorStatusKind::SpawnAlive);
    assert!(!store.get(7).unwrap().status.dead);
    assert!(store.get(7).unwrap().is_invisible());
    flags(&mut store, 0);
    assert!(!store.get(7).unwrap().is_invisible());
    event(&mut store, ActorStatusKind::Death);
    store.advance_interpolation_ticks(21);
    assert!(store.get(7).unwrap().is_invisible());
    event(&mut store, ActorStatusKind::SpawnAlive);
    flags(&mut store, 1 << ActorFlag::Invisible.wire_value());
    store.advance_interpolation_ticks(30);
    assert!(store.get(7).unwrap().is_invisible());
}

#[test]
fn server_removal_and_replacement_own_the_actor_lifetime() {
    let mut store = player();
    event(&mut store, ActorStatusKind::Death);
    store.advance_interpolation_ticks(21);
    assert_eq!(
        store.apply(
            1,
            store.latest_sequence + 1,
            ActorEvent::Remove(ActorRemoveEvent {
                dimension: 0,
                unique_id: 5
            })
        ),
        ActorApplyResult::Removed
    );
    assert!(store.get(7).is_none());
    let ActorEvent::Spawn(mut spawn) = crate::actor_store::tests::spawn(7, 5) else {
        unreachable!()
    };
    spawn.kind = ActorKind::Player {
        uuid: [7; 16],
        username: "Fixture".into(),
    };
    store.apply(1, store.latest_sequence + 1, ActorEvent::Spawn(spawn));
    assert!(!store.get(7).unwrap().is_invisible());
    assert!(!store.get(7).unwrap().status.dead);
}

#[test]
fn spectator_deaths_wait_until_the_player_leaves_spectator() {
    for mode in [
        GameModeUpdate::Explicit(PlayerGameMode::Spectator),
        GameModeUpdate::WorldDefault,
    ] {
        let mut store = player();
        store.world_default_game_mode = Some(PlayerGameMode::Spectator);
        store.actors.get_mut(&7).unwrap().player_game_mode = Some(mode);
        event(&mut store, ActorStatusKind::Death);
        store.advance_interpolation_ticks(30);
        assert!(!store.get(7).unwrap().is_invisible());
        store.actors.get_mut(&7).unwrap().player_game_mode =
            Some(GameModeUpdate::Explicit(PlayerGameMode::Survival));
        store.advance_interpolation_ticks(1);
        assert!(store.get(7).unwrap().is_invisible());
    }
}

#[test]
fn player_visibility_transition_does_not_apply_to_generic_actors() {
    let mut store = ActorStore::new(1, 0);
    store.apply(1, 1, crate::actor_store::tests::spawn(7, 5));
    event(&mut store, ActorStatusKind::Death);
    store.advance_interpolation_ticks(21);
    assert!(!store.get(7).unwrap().is_invisible());
}
