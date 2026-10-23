//! `Invisible` hides the body unless `RenderWhenInvisible` keeps it in the draw list.

use protocol_ids::{ActorDataKey, ActorFlag};
use std::sync::Arc;

use protocol::{ActorEvent, ActorKind, ActorMetadata, ActorMetadataValue};

use super::{ActorStore, tests::spawn};

/// Spawns an entity with both protocol flag words supplied by the caller.
fn entity_with_flags(store: &mut ActorStore, flags: u64, overflow: u64) {
    let ActorEvent::Spawn(mut event) = spawn(8, 80) else {
        unreachable!();
    };
    event.kind = ActorKind::Entity {
        identifier: "test:furniture".into(),
    };
    event.metadata = Arc::from([
        ActorMetadata {
            key: ActorDataKey::Flags.wire_value(),
            value: ActorMetadataValue::Flags(flags),
        },
        ActorMetadata {
            key: ActorDataKey::FlagsTwo.wire_value(),
            value: ActorMetadataValue::FlagsExtended(overflow),
        },
    ]);
    store.apply(1, 1, ActorEvent::Spawn(event));
}

#[test]
fn an_invisible_actor_is_hidden() {
    let mut store = ActorStore::new(1, 0);
    entity_with_flags(&mut store, 1 << ActorFlag::Invisible.wire_value(), 0);

    assert!(
        store.get(8).unwrap().is_invisible(),
        "invisibility hides the actor's body"
    );
}

#[test]
fn render_when_invisible_keeps_the_body() {
    let mut store = ActorStore::new(1, 0);
    entity_with_flags(
        &mut store,
        1 << ActorFlag::Invisible.wire_value(),
        1 << (ActorFlag::RenderWhenInvisible.wire_value() - 64),
    );

    let actor = store.get(8).unwrap();
    assert!(
        !actor.is_invisible(),
        "furniture entities pair Invisible with RenderWhenInvisible and still draw"
    );
}

#[test]
fn render_when_invisible_alone_leaves_a_visible_actor_visible() {
    let mut store = ActorStore::new(1, 0);
    entity_with_flags(
        &mut store,
        0,
        1 << (ActorFlag::RenderWhenInvisible.wire_value() - 64),
    );

    assert!(!store.get(8).unwrap().is_invisible());
}
