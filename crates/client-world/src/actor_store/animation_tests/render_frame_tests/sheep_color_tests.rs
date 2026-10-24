//! Sheep wool dye reaches the rig through colour and name metadata.
use std::sync::Arc;

use assets::{RuntimeEntityAssets, facts::entity::NativeEntityClass};
use protocol::{
    ActorKind, ActorMetadata, ActorMetadataUpdateEvent, ActorMetadataValue, ActorSpawnEvent,
};

use crate::actor_store::ActorStore;
use actor_animation::test_support;

const COLOR_KEY: u32 = protocol_ids::ActorDataKey::ColorIndex.wire_value();
const NAME_KEY: u32 = protocol_ids::ActorDataKey::Name.wire_value();
const RED: [f32; 4] = [176.0 / 255.0, 46.0 / 255.0, 38.0 / 255.0, 0.0];

/// Spawns a sheep with a controller that inherits the actor's color, as vanilla's does.
fn sheep(metadata: Vec<ActorMetadata>) -> ActorStore {
    let identifier = NativeEntityClass::Sheep.identifier().unwrap();
    let mut compiled = test_support::counting_random_compiled(identifier);
    compiled.render.layers[0].color = None;
    let assets = Arc::new(
        RuntimeEntityAssets::from_compiled(compiled.with_unresolved_admissions()).unwrap(),
    );
    let mut store = ActorStore::new_with_entity_assets(1, 0, assets);
    store.apply(
        1,
        1,
        protocol::ActorEvent::Spawn(ActorSpawnEvent {
            dimension: 0,
            unique_id: 0,
            runtime_id: 1,
            kind: ActorKind::Entity {
                identifier: identifier.into(),
            },
            position: [0.0; 3],
            velocity: [0.0; 3],
            pitch: 0.0,
            yaw: 0.0,
            head_yaw: 0.0,
            body_yaw: 0.0,
            held_item: Default::default(),
            metadata: metadata.into(),
            attributes: Arc::from([]),
            properties: Arc::from([]),
            links: Arc::from([]),
        }),
    );
    store.advance_interpolation_ticks(1);
    store
}

fn color(value: i8) -> ActorMetadata {
    ActorMetadata {
        key: COLOR_KEY,
        value: ActorMetadataValue::Byte(value),
    }
}

/// Applies a metadata transition and completes the next actor tick.
fn update(store: &mut ActorStore, metadata: Vec<ActorMetadata>) {
    let tick = store.actor_rig(1).unwrap().completed_tick + 1;
    store.apply(
        1,
        tick,
        protocol::ActorEvent::Metadata(ActorMetadataUpdateEvent {
            dimension: 0,
            runtime_id: 1,
            tick,
            properties: Arc::from([]),
            metadata: metadata.into(),
        }),
    );
    store.advance_interpolation_ticks(1);
}

#[test]
fn sheep_wool_follows_its_byte_dye_from_spawn_and_updates() {
    let mut store = sheep(vec![color(0)]);
    assert_eq!(store.actor_rig(1).unwrap().render[0].color, [1.0; 4]);
    assert_eq!(
        store.render_frame(0.5).layers(1).unwrap()[0].color,
        [1.0; 4]
    );
    // `wololo` turns a sheep red by updating its colour index.
    update(&mut store, vec![color(14)]);
    assert_eq!(store.actor_rig(1).unwrap().render[0].color, RED);
    assert_eq!(store.render_frame(0.5).layers(1).unwrap()[0].color, RED);
    let red = sheep(vec![color(14)]);
    assert_eq!(red.actor_rig(1).unwrap().render[0].color, RED);
}

#[test]
fn jeb_sheep_wool_changes_colour_every_tick_without_metadata_updates() {
    let mut store = sheep(vec![
        color(14),
        ActorMetadata {
            key: NAME_KEY,
            value: ActorMetadataValue::String("jeb_".into()),
        },
    ]);
    let first = store.actor_rig(1).unwrap().render[0].color;
    assert_ne!(first, RED);
    store.advance_interpolation_ticks(1);
    let second = store.actor_rig(1).unwrap().render[0].color;
    assert_ne!(second, first);
    assert_eq!(store.render_frame(0.5).layers(1).unwrap()[0].color, second);
}
