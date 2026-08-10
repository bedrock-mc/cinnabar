use std::sync::Arc;

use protocol::{
    ActorEvent, ActorKind, ActorLinkEvent, ActorLinkType, ActorRemoveEvent, ActorSpawnEvent,
};

use super::{ActorApplyResult, ActorStore};

fn spawn(runtime_id: u64, unique_id: i64) -> ActorEvent {
    ActorEvent::Spawn(ActorSpawnEvent {
        dimension: 0,
        unique_id,
        runtime_id,
        kind: ActorKind::Entity {
            identifier: "minecraft:bee".into(),
        },
        position: [1.0, 2.0, 3.0],
        velocity: [0.0; 3],
        pitch: 0.0,
        yaw: 0.0,
        head_yaw: 0.0,
        body_yaw: 0.0,
        held_item: Default::default(),
        metadata: Arc::from([]),
        attributes: Arc::from([]),
        properties: Arc::from([]),
        links: Arc::from([]),
    })
}

const fn link(
    rider_unique_id: i64,
    ridden_unique_id: i64,
    link_type: ActorLinkType,
) -> ActorLinkEvent {
    ActorLinkEvent {
        dimension: 0,
        ridden_unique_id,
        rider_unique_id,
        link_type,
        immediate: false,
        rider_initiated: false,
    }
}

#[test]
fn accepts_link_before_spawn_and_preserves_unknown_and_pair_removal() {
    let mut store = ActorStore::new(1, 0);
    assert_eq!(
        store.apply_link(1, 1, link(10, 20, ActorLinkType::Rider)),
        ActorApplyResult::Updated
    );
    assert_eq!(store.ridden_unique_id(10), Some(20));
    assert_eq!(store.apply(1, 2, spawn(10, 10)), ActorApplyResult::Inserted);
    assert_eq!(store.ridden_unique_id(10), Some(20));
    assert_eq!(
        store.apply_link(1, 3, link(10, 99, ActorLinkType::Unknown(9))),
        ActorApplyResult::Updated
    );
    assert_eq!(
        store.apply_link(1, 4, link(10, 99, ActorLinkType::Remove)),
        ActorApplyResult::Updated
    );
    assert_eq!(store.ridden_unique_id(10), Some(20));
    assert_eq!(
        store.apply_link(1, 5, link(10, 30, ActorLinkType::Passenger)),
        ActorApplyResult::Updated
    );
    assert_eq!(store.ridden_unique_id(10), Some(30));
    assert_eq!(
        store.apply_link(1, 6, link(10, 30, ActorLinkType::Remove)),
        ActorApplyResult::Updated
    );
    assert_eq!(store.ridden_unique_id(10), None);
}

#[test]
fn enforces_session_sequence_dimension_and_capacity() {
    let mut store = ActorStore::with_capacity(1, 0, 2, 1);
    assert_eq!(
        store.apply_link(2, 1, link(1, 10, ActorLinkType::Rider)),
        ActorApplyResult::StaleSession
    );
    let mut wrong_dimension = link(1, 10, ActorLinkType::Rider);
    wrong_dimension.dimension = 1;
    assert_eq!(
        store.apply_link(1, 1, wrong_dimension),
        ActorApplyResult::StaleDimension
    );
    assert_eq!(
        store.apply_link(1, 1, link(1, 10, ActorLinkType::Rider)),
        ActorApplyResult::StaleSequence
    );
    assert_eq!(
        store.apply_link(1, 2, link(1, 10, ActorLinkType::Rider)),
        ActorApplyResult::Updated
    );
    assert_eq!(
        store.apply_link(1, 3, link(2, 20, ActorLinkType::Rider)),
        ActorApplyResult::Updated
    );
    assert_eq!(
        store.apply_link(1, 4, link(3, 30, ActorLinkType::Rider)),
        ActorApplyResult::CapacityRejected
    );
    assert_eq!(
        store.apply_link(1, 5, link(1, 11, ActorLinkType::Passenger)),
        ActorApplyResult::Updated
    );
    assert_eq!(store.ridden_unique_id(1), Some(11));
}

#[test]
fn actor_lifetime_cleanup_removes_both_sides_and_reset_clears_links() {
    let mut store = ActorStore::new(1, 0);
    assert_eq!(store.apply(1, 1, spawn(20, 20)), ActorApplyResult::Inserted);
    assert_eq!(
        store.apply_link(1, 2, link(10, 20, ActorLinkType::Rider)),
        ActorApplyResult::Updated
    );
    assert_eq!(
        store.apply_link(1, 3, link(11, 20, ActorLinkType::Passenger)),
        ActorApplyResult::Updated
    );
    assert_eq!(
        store.apply(
            1,
            4,
            ActorEvent::Remove(ActorRemoveEvent {
                dimension: 0,
                unique_id: 20
            })
        ),
        ActorApplyResult::Removed
    );
    assert_eq!(store.ridden_unique_id(10), None);
    assert_eq!(store.ridden_unique_id(11), None);
    assert_eq!(
        store.apply_link(1, 5, link(10, 30, ActorLinkType::Rider)),
        ActorApplyResult::Updated
    );
    assert_eq!(store.reset_dimension(1, 6, 1), ActorApplyResult::Reset);
    assert_eq!(store.ridden_unique_id(10), None);
}

#[test]
fn removal_cleans_link_authority_even_when_the_actor_never_spawned() {
    let mut store = ActorStore::new(1, 0);
    assert_eq!(
        store.apply_link(1, 1, link(10, 20, ActorLinkType::Rider)),
        ActorApplyResult::Updated
    );
    assert_eq!(
        store.apply(
            1,
            2,
            ActorEvent::Remove(ActorRemoveEvent {
                dimension: 0,
                unique_id: 20,
            })
        ),
        ActorApplyResult::MissingActor
    );
    assert_eq!(store.ridden_unique_id(10), None);
}

#[test]
fn runtime_and_unique_id_replacements_cleanup_the_previous_lifetime_links() {
    let mut store = ActorStore::new(1, 0);
    assert_eq!(store.apply(1, 1, spawn(20, 20)), ActorApplyResult::Inserted);
    assert_eq!(
        store.apply_link(1, 2, link(10, 20, ActorLinkType::Rider)),
        ActorApplyResult::Updated
    );
    assert_eq!(store.apply(1, 3, spawn(20, 21)), ActorApplyResult::Replaced);
    assert_eq!(store.ridden_unique_id(10), None);

    assert_eq!(
        store.apply_link(1, 4, link(10, 21, ActorLinkType::Rider)),
        ActorApplyResult::Updated
    );
    assert_eq!(store.apply(1, 5, spawn(22, 21)), ActorApplyResult::Replaced);
    assert_eq!(store.ridden_unique_id(10), None);
}

#[test]
fn beginning_a_new_session_clears_all_link_authority() {
    let mut store = ActorStore::new(1, 0);
    assert_eq!(
        store.apply_link(1, 1, link(10, 20, ActorLinkType::Rider)),
        ActorApplyResult::Updated
    );
    store.begin_session(2, 0);
    assert_eq!(store.ridden_unique_id(10), None);
    assert_eq!(
        store.apply_link(1, 2, link(10, 30, ActorLinkType::Rider)),
        ActorApplyResult::StaleSession
    );
}

#[test]
fn embedded_spawn_links_apply_once_in_packet_order() {
    let mut store = ActorStore::new(1, 0);
    let ActorEvent::Spawn(mut event) = spawn(10, 10) else {
        unreachable!()
    };
    event.links = Arc::from([
        link(10, 20, ActorLinkType::Rider),
        link(10, 30, ActorLinkType::Passenger),
        link(10, 40, ActorLinkType::Unknown(7)),
    ]);
    assert_eq!(
        store.apply(1, 1, ActorEvent::Spawn(event)),
        ActorApplyResult::Inserted
    );
    assert_eq!(store.ridden_unique_id(10), Some(30));
}

#[test]
fn embedded_link_with_a_stale_dimension_rejects_the_composite_spawn() {
    let mut store = ActorStore::new(1, 0);
    let ActorEvent::Spawn(mut event) = spawn(10, 10) else {
        unreachable!()
    };
    let mut stale = link(10, 20, ActorLinkType::Rider);
    stale.dimension = 1;
    event.links = Arc::from([stale]);
    assert_eq!(
        store.apply(1, 1, ActorEvent::Spawn(event)),
        ActorApplyResult::StaleDimension
    );
    assert!(store.is_empty());
    assert_eq!(store.ridden_unique_id(10), None);
}
