//! First presentation of spawned actors before any simulation tick.
use std::sync::Arc;

use assets::RuntimeEntityAssets;
use protocol::ActorMetadataValue;

use crate::{actor_animation::test_support, actor_store::ActorStore};

/// Loads the initialized pose and visibility fixture.
fn assets() -> Arc<RuntimeEntityAssets> {
    Arc::new(RuntimeEntityAssets::from_compiled(test_support::initialized_pose_compiled()).unwrap())
}

/// Admits a fresh lifetime with its authoritative visibility flag already installed.
fn spawn(store: &mut ActorStore, sequence: u64, flags: u64) {
    store.apply(
        1,
        sequence,
        protocol::ActorEvent::Spawn(protocol::ActorSpawnEvent {
            dimension: 0,
            unique_id: 17,
            runtime_id: 1,
            kind: protocol::ActorKind::Entity {
                identifier: "minecraft:test".into(),
            },
            position: [0.0; 3],
            velocity: [0.0; 3],
            pitch: 0.0,
            yaw: 0.0,
            head_yaw: 0.0,
            body_yaw: 0.0,
            held_item: Default::default(),
            metadata: Arc::from([protocol::ActorMetadata {
                key: 0,
                value: ActorMetadataValue::Flags(flags),
            }]),
            attributes: Arc::from([]),
            properties: Arc::from([]),
            links: Arc::from([]),
        }),
    );
}

#[test]
fn first_presentation_uses_initialized_pose_and_spawn_metadata_without_a_tick() {
    for pack in [false, true] {
        for show_bottom in [false, true] {
            let assets = assets();
            let mut store = ActorStore::new_with_entity_assets(1, 0, Arc::clone(&assets));
            if pack {
                store.set_pack_entities(Some((assets, vec![0])));
            }
            spawn(&mut store, 1, if show_bottom { 1 << 38 } else { 0 });
            let actor = store.get(1).unwrap().clone();
            store.advance_interpolation_frame(0);
            let rig = store.actor_rig(1).unwrap();
            assert_eq!(
                rig.completed_tick, 0,
                "presentation must not simulate a tick"
            );
            assert_eq!(
                rig.previous, rig.current,
                "never interpolate from bind pose"
            );
            assert_ne!(rig.current, rig.rest, "draw the authored initial pose");
            assert!((rig.current[0].translation_scale[1] - 6.0).abs() < 1e-6);
            assert_eq!(rig.render.len(), 1);
            assert_eq!(rig.render[0].color[0], 6.0);
            assert_eq!(rig.render[0].hidden_bones.is_empty(), show_bottom);
            let pose = rig.current.to_vec();
            let layers = rig.render.to_vec();
            let stats = store.animation_stats();
            store.advance_interpolation_frame(0);
            assert_eq!(store.actor_rig(1).unwrap().current, pose);
            assert_eq!(store.actor_rig(1).unwrap().render, layers);
            assert_eq!(store.animation_stats(), stats, "initialize only once");
            assert_eq!(
                store.get(1).unwrap(),
                &actor,
                "leave actor simulation unchanged"
            );
            store.advance_interpolation_ticks(1);
            assert_eq!(store.actor_rig(1).unwrap().render[0].color[0], 7.0);
        }
    }
}

#[test]
fn a_replaced_lifetime_initializes_its_own_pose_once() {
    let mut store = ActorStore::new_with_entity_assets(1, 0, assets());
    spawn(&mut store, 1, 0);
    store.advance_interpolation_frame(0);
    let first_lifetime = store.actor_rig(1).unwrap().actor;
    store.advance_interpolation_ticks(1);
    spawn(&mut store, 2, 1 << 38);
    store.advance_interpolation_frame(0);
    let rig = store.actor_rig(1).unwrap();
    assert_ne!(rig.actor, first_lifetime);
    assert_eq!(rig.completed_tick, 1);
    assert_eq!(rig.previous, rig.current);
    assert_eq!(rig.render[0].color[0], 6.0);
    assert!(rig.render[0].hidden_bones.is_empty());
    store.apply(
        1,
        3,
        protocol::ActorEvent::Remove(protocol::ActorRemoveEvent {
            dimension: 0,
            unique_id: 17,
        }),
    );
    store.advance_interpolation_frame(0);
    assert!(store.actor_rig(1).is_none());
    spawn(&mut store, 4, 0);
    store.advance_interpolation_frame(0);
    assert_eq!(store.actor_rig(1).unwrap().render[0].color[0], 6.0);
}
