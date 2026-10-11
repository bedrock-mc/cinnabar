use super::*;
use protocol::{ActorMetadata, ActorMetadataUpdateEvent, ActorSpawnEvent};

use crate::actor_store::{
    ACTOR_FLAG_TAMED,
    color::{COLOR_METADATA_KEY as COLOR_KEY, WOLF_IDENTIFIER},
};
const FLAGS_KEY: u32 = 0;
const TAMED: u64 = 1 << ACTOR_FLAG_TAMED;

/// Spawns a wolf with a controller that inherits the actor's color.
fn wolf(flags: u64, color: ActorMetadataValue) -> crate::actor_store::ActorStore {
    let mut compiled = counting_random_compiled(WOLF_IDENTIFIER);
    compiled.render.layers[0].color = None;
    let assets = Arc::new(RuntimeEntityAssets::from_compiled(compiled).unwrap());
    let mut store = crate::actor_store::ActorStore::new_with_entity_assets(1, 0, assets);
    store.apply(
        1,
        1,
        protocol::ActorEvent::Spawn(ActorSpawnEvent {
            dimension: 0,
            unique_id: 1,
            runtime_id: 1,
            kind: ActorKind::Entity {
                identifier: WOLF_IDENTIFIER.into(),
            },
            position: [0.0; 3],
            velocity: [0.0; 3],
            pitch: 0.0,
            yaw: 0.0,
            head_yaw: 0.0,
            body_yaw: 0.0,
            held_item: Default::default(),
            metadata: vec![
                ActorMetadata {
                    key: FLAGS_KEY,
                    value: ActorMetadataValue::Flags(flags),
                },
                ActorMetadata {
                    key: COLOR_KEY,
                    value: color,
                },
            ]
            .into(),
            attributes: Arc::from([]),
            properties: Arc::from([]),
            links: Arc::from([]),
        }),
    );
    store.advance_interpolation_ticks(1);
    store
}

/// Applies a metadata transition and completes the next actor tick.
fn update(store: &mut crate::actor_store::ActorStore, flags: u64, color: ActorMetadataValue) {
    let tick = store.actor_rig(1).unwrap().completed_tick + 1;
    store.apply(
        1,
        tick,
        protocol::ActorEvent::Metadata(ActorMetadataUpdateEvent {
            dimension: 0,
            runtime_id: 1,
            tick,
            properties: Arc::from([]),
            metadata: vec![
                ActorMetadata {
                    key: FLAGS_KEY,
                    value: ActorMetadataValue::Flags(flags),
                },
                ActorMetadata {
                    key: COLOR_KEY,
                    value: color,
                },
            ]
            .into(),
        }),
    );
    store.advance_interpolation_ticks(1);
}

#[test]
fn wolf_collar_color_follows_byte_dye_updates_and_taming() {
    let mut store = wolf(TAMED, ActorMetadataValue::Byte(14));
    let red = [176.0 / 255.0, 46.0 / 255.0, 38.0 / 255.0, 0.0];
    assert_eq!(store.actor_rig(1).unwrap().render[0].color, red);
    assert_eq!(store.render_frame(0.5).layers(1).unwrap()[0].color, red);
    update(
        &mut store,
        TAMED | (1 << 11) | (1 << 24),
        ActorMetadataValue::Byte(11),
    );
    let blue = [60.0 / 255.0, 68.0 / 255.0, 170.0 / 255.0, 0.0];
    assert_eq!(store.actor_rig(1).unwrap().render[0].color, blue);
    assert_eq!(store.render_frame(0.75).layers(1).unwrap()[0].color, blue);
    update(&mut store, 0, ActorMetadataValue::Byte(14));
    assert_eq!(store.actor_rig(1).unwrap().render[0].color, [1.0; 4]);
    update(&mut store, TAMED | (1 << 25), ActorMetadataValue::Byte(14));
    assert_eq!(store.render_frame(0.25).layers(1).unwrap()[0].color, red);
}

#[test]
fn wolf_collar_masks_byte_index_and_rejects_other_metadata_types() {
    let mut store = wolf(TAMED, ActorMetadataValue::Byte(-2));
    assert_eq!(
        store.actor_rig(1).unwrap().render[0].color,
        [176.0 / 255.0, 46.0 / 255.0, 38.0 / 255.0, 0.0]
    );
    for invalid in [ActorMetadataValue::Int(14), ActorMetadataValue::Float(14.0)] {
        update(&mut store, TAMED, invalid);
        assert_eq!(
            store.render_frame(0.5).layers(1).unwrap()[0].color,
            [1.0; 4]
        );
    }
}

#[test]
fn wolf_collar_dye_does_not_color_its_equipment() {
    use crate::actor_animation::attachable::{AttachableAnimationInput, AttachablesRuntime};

    let store = wolf(TAMED, ActorMetadataValue::Byte(14));
    let compiled = super::super::super::attachable::tests::compiled_fixture();
    let assets = Arc::new(RuntimeEntityAssets::from_compiled(compiled).unwrap());
    let mut runtime = AttachablesRuntime::new(assets);
    let snapshot = runtime
        .evaluate(
            "minecraft:test_item",
            store.get(1).unwrap(),
            &store.actor_rig(1).unwrap(),
            AttachableAnimationInput::default(),
        )
        .unwrap();
    assert_eq!(snapshot.render[0].color, [1.0; 4]);
}
