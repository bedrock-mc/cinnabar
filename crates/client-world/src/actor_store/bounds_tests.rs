use std::sync::Arc;

use protocol::{ActorEvent, ActorKind, ActorMetadata, ActorMetadataValue};

use super::{
    ActorStore, BOUNDING_BOX_HEIGHT_METADATA_KEY, BOUNDING_BOX_WIDTH_METADATA_KEY,
    SCALE_METADATA_KEY, tests::spawn,
};

fn custom_actor(metadata: Vec<ActorMetadata>) -> ActorStore {
    let ActorEvent::Spawn(mut event) = spawn(8, 80) else {
        unreachable!();
    };
    event.kind = ActorKind::Entity {
        identifier: "example:game_selector".into(),
    };
    event.position = [0.0; 3];
    event.metadata = Arc::from(metadata);
    let mut store = ActorStore::new(1, 0);
    store.apply(1, 1, ActorEvent::Spawn(event));
    store
}

fn dimension(key: u32, value: f32) -> ActorMetadata {
    ActorMetadata {
        key,
        value: ActorMetadataValue::Float(value),
    }
}

#[test]
fn custom_actor_keeps_default_bounds_when_size_metadata_is_omitted() {
    let store = custom_actor(vec![]);
    let (min, max) = store.get(8).unwrap().bounding_box().expect("default box");
    assert_eq!((min, max), ([-0.3, 0.0, -0.3], [0.3, 1.8, 0.3]));
}

#[test]
fn custom_actor_defaults_each_omitted_dimension_independently() {
    let store = custom_actor(vec![dimension(BOUNDING_BOX_HEIGHT_METADATA_KEY, 2.0)]);
    let (min, max) = store.get(8).unwrap().bounding_box().expect("default width");
    assert_eq!((min, max), ([-0.3, 0.0, -0.3], [0.3, 2.0, 0.3]));
}

#[test]
fn server_scale_changes_effective_bounds_without_changing_raw_dimensions() {
    let store = custom_actor(vec![
        dimension(BOUNDING_BOX_WIDTH_METADATA_KEY, 0.8),
        dimension(BOUNDING_BOX_HEIGHT_METADATA_KEY, 2.0),
        dimension(SCALE_METADATA_KEY, 2.0),
    ]);
    let actor = store.get(8).unwrap();
    assert_eq!(
        actor.bounding_box(),
        Some(([-0.8, 0.0, -0.8], [0.8, 4.0, 0.8]))
    );
    assert_eq!(
        actor.metadata.get(&BOUNDING_BOX_WIDTH_METADATA_KEY),
        Some(&ActorMetadataValue::Float(0.8))
    );
}

#[test]
fn zero_scale_text_host_does_not_get_an_unscaled_humanoid_hitbox() {
    let store = custom_actor(vec![dimension(SCALE_METADATA_KEY, 0.0)]);
    if let Some((min, max)) = store.get(8).unwrap().bounding_box() {
        assert!((0..3).all(|axis| max[axis] - min[axis] < 0.1));
    }
}
