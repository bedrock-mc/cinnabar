use super::{ActorStore, LocalItemUse, local_tests::local_feed};
use protocol::{ActorEvent, ActorMetadata, ActorMetadataUpdateEvent, ActorMetadataValue};
use protocol_ids::{ActorDataKey, ActorFlag};

/// Admits a primary flag word through the same path as a server metadata update.
fn receive_flags(store: &mut ActorStore, sequence: u64, flags: u64) {
    store.apply(
        1,
        sequence,
        ActorEvent::Metadata(ActorMetadataUpdateEvent {
            dimension: 0,
            runtime_id: 1,
            metadata: [ActorMetadata {
                key: ActorDataKey::Flags.wire_value(),
                value: ActorMetadataValue::Flags(flags),
            }]
            .into(),
            properties: [].into(),
            tick: sequence,
        }),
    );
}

#[test]
fn presentation_feed_preserves_gameplay_metadata() {
    let mut store = ActorStore::new(1, 0);
    let mut feed = local_feed(0.0, 0.0);
    store.sync_local_player(1, -100, &feed);
    receive_flags(&mut store, 1, 1 << ActorFlag::Eating.wire_value());
    let canonical = store.get(1).unwrap().metadata.clone();
    store.actors.get_mut(&1).unwrap().status.fluid = Some((true, false));
    feed.sneaking = true;
    feed.sprinting = true;
    feed.gliding = true;
    feed.spinning = true;
    feed.item_use = LocalItemUse::Using;
    store.sync_local_player(1, -100, &feed);
    let actor = store.get(1).unwrap();
    assert!(actor.is_sneaking() && actor.is_gliding() && actor.is_spinning());
    assert!(actor.is_using_item());
    assert!(actor.flag(ActorFlag::Swimming.wire_value()));
    assert!(actor.flag(ActorFlag::Eating.wire_value()));
    assert_eq!(
        actor.metadata, canonical,
        "render flags must stay out of gameplay metadata"
    );
}

#[test]
fn server_flags_replace_presentation_until_the_next_feed() {
    let mut store = ActorStore::new(1, 0);
    let mut feed = local_feed(0.0, 0.0);
    feed.item_use = LocalItemUse::Using;
    feed.gliding = true;
    store.sync_local_player(1, -100, &feed);
    receive_flags(&mut store, 1, 1 << ActorFlag::Eating.wire_value());
    let actor = store.get(1).unwrap();
    assert!(!actor.is_using_item() && !actor.is_gliding());
    let canonical = actor.metadata.clone();
    store.sync_local_player(1, -100, &feed);
    let actor = store.get(1).unwrap();
    assert!(actor.is_using_item() && actor.is_gliding());
    assert!(actor.flag(ActorFlag::Eating.wire_value()));
    assert_eq!(actor.metadata, canonical);
}

#[test]
fn idle_presentation_preserves_received_use_and_pose() {
    let mut store = ActorStore::new(1, 0);
    let mut feed = local_feed(0.0, 0.0);
    store.sync_local_player(1, -100, &feed);
    receive_flags(
        &mut store,
        1,
        (1 << ActorFlag::UsingItem.wire_value())
            | (1 << ActorFlag::Gliding.wire_value())
            | (1 << ActorFlag::Swimming.wire_value()),
    );
    let canonical = store.get(1).unwrap().metadata.clone();
    feed.item_use = LocalItemUse::Idle;
    store.sync_local_player(1, -100, &feed);
    let actor = store.get(1).unwrap();
    assert!(!actor.is_using_item() && !actor.is_gliding());
    assert!(!actor.flag(ActorFlag::Swimming.wire_value()));
    assert_eq!(actor.metadata, canonical);
}

#[test]
fn presentation_flags_preserve_unusable_server_words() {
    let mut store = ActorStore::new(1, 0);
    let mut feed = local_feed(0.0, 0.0);
    feed.item_use = LocalItemUse::Using;
    store.sync_local_player(1, -100, &feed);
    store.apply(
        1,
        1,
        ActorEvent::Metadata(ActorMetadataUpdateEvent {
            dimension: 0,
            runtime_id: 1,
            metadata: [ActorMetadata {
                key: ActorDataKey::Flags.wire_value(),
                value: ActorMetadataValue::Int(5),
            }]
            .into(),
            properties: [].into(),
            tick: 1,
        }),
    );
    store.sync_local_player(1, -100, &feed);
    let actor = store.get(1).unwrap();
    assert!(!actor.is_using_item());
    assert_eq!(
        actor.metadata[&ActorDataKey::Flags.wire_value()],
        ActorMetadataValue::Int(5)
    );
}

#[test]
fn dimension_reset_retires_presentation_flags() {
    let mut store = ActorStore::new(1, 0);
    let mut feed = local_feed(0.0, 0.0);
    feed.item_use = LocalItemUse::Using;
    feed.gliding = true;
    store.sync_local_player(1, -100, &feed);
    assert!(store.get(1).unwrap().is_using_item());
    store.reset_dimension(1, 1, 2);
    store.sync_local_player(1, -100, &local_feed(0.0, 0.0));
    let actor = store.get(1).unwrap();
    assert!(!actor.is_using_item() && !actor.is_gliding());
}
