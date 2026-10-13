use super::*;
use protocol::{ActorEvent, ActorRemoveEvent, WorldEvent};

/// Admits a real dedicated-packet event, keeping wire construction in protocol.
fn painting(title: &str, direction: i32) -> ActorEvent {
    let WorldEvent::Actor(event) = protocol::test_support::packets::add_painting_event(
        -73,
        91,
        title,
        direction,
        [10.5, 70.5, 20.5],
    )
    .unwrap()
    .unwrap() else {
        panic!("painting actor event");
    };
    event
}

#[test]
fn dedicated_painting_ingress_retains_centred_wall_boxes_and_removal() {
    for (direction, expected) in [
        (0, [1.46875, 1.96875, 0.03125]),
        (1, [0.03125, 1.96875, 1.46875]),
        (2, [1.46875, 1.96875, 0.03125]),
        (3, [0.03125, 1.96875, 1.46875]),
    ] {
        let mut store = ActorStore::new(1, 0);
        assert_eq!(
            store.apply(1, 1, painting("backyard", direction)),
            ActorApplyResult::Inserted
        );
        let actor = store.actors.get(&91).unwrap();
        assert_eq!(actor.painting.as_ref().unwrap().motive.as_ref(), "backyard");
        assert_eq!(actor.native_position(), [10.5, 70.5, 20.5]);
        let (low, high) = actor.bounding_box().unwrap();
        for (axis, extent) in expected.into_iter().enumerate() {
            assert_eq!(low[axis], [10.5, 70.5, 20.5][axis] - extent);
            assert_eq!(high[axis], [10.5, 70.5, 20.5][axis] + extent);
        }
        assert_eq!(actor.hit_boxes().next(), Some((low, high)));
        assert_eq!(actor.shadow_radius(false), 0.0);
        assert_eq!(
            store.apply(
                1,
                2,
                ActorEvent::Remove(ActorRemoveEvent {
                    dimension: 0,
                    unique_id: -73
                })
            ),
            ActorApplyResult::Removed
        );
        assert!(!store.actors.contains_key(&91));
    }
}

#[test]
fn unknown_painting_motives_fall_back_without_losing_actor_identity() {
    let mut store = ActorStore::new(1, 0);
    store.apply(1, 1, painting("future_motive", 0));
    let actor = store.actors.get(&91).unwrap();
    assert_eq!(actor.unique_id, -73);
    assert_eq!(
        actor.painting.as_ref().unwrap().motive.as_ref(),
        "future_motive"
    );
    assert_eq!(store.painting_motive_fallbacks, 1);
    let (low, high) = actor.bounding_box().unwrap();
    assert_eq!(high[0] - low[0], 0.9375);
    assert_eq!(high[1] - low[1], 0.9375);
    let replacement = super::tests::spawn(91, -74);
    store.apply(1, 2, replacement);
    assert!(store.actors.get(&91).unwrap().painting.is_none());
}
