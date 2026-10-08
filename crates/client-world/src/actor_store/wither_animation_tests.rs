use std::sync::Arc;

use protocol::{ActorEvent, ActorMetadataUpdateEvent, ActorStatusEvent, ActorStatusKind};

use super::*;
use crate::actor_store::ActorStore;

fn wither_store(assets: Option<Arc<assets::RuntimeEntityAssets>>) -> ActorStore {
    let mut store = assets.map_or_else(
        || ActorStore::new(1, 0),
        |assets| ActorStore::new_with_entity_assets(1, 0, assets),
    );
    let ActorEvent::Spawn(mut spawn) = crate::actor_store::tests::spawn(7, 7) else {
        unreachable!();
    };
    spawn.kind = ActorKind::Entity {
        identifier: "minecraft:wither".into(),
    };
    spawn.metadata = Arc::from([
        ActorMetadata {
            key: INVULNERABLE_TICKS_KEY,
            value: ActorMetadataValue::Int(i32::from(DEATH_TICKS)),
        },
        ActorMetadata {
            key: SHIELD_DISABLED_KEY,
            value: ActorMetadataValue::Short(1),
        },
    ]);
    spawn.attributes = Arc::from([protocol::ActorAttribute {
        name: "minecraft:health".into(),
        min: 0.0,
        max: 450.0,
        current: 4.0,
        default: None,
        modifiers: Arc::from([]),
    }]);
    store.apply(1, 1, ActorEvent::Spawn(spawn));
    store
}

fn status(store: &mut ActorStore, kind: ActorStatusKind) {
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
fn wither_spawn_fades_and_shield_uses_the_synced_short_instead_of_health() {
    let mut store = wither_store(None);
    let actor = store.get(7).unwrap();
    assert_eq!(actor.wither_query("is_shield_powered", 0.0), Some(0.0));
    assert_eq!(actor.wither_query("overlay_alpha", 0.0), Some(1.0));
    store.advance_interpolation_ticks(100);
    let actor = store.get(7).unwrap();
    assert_eq!(actor.wither_query("invulnerable_ticks", 0.0), Some(100.0));
    assert!((actor.wither_query("overlay_alpha", 0.0).unwrap() - 0.25).abs() < 1e-5);
    store.apply(
        1,
        3,
        ActorEvent::Metadata(ActorMetadataUpdateEvent {
            dimension: 0,
            runtime_id: 7,
            metadata: Arc::from([ActorMetadata {
                key: SHIELD_DISABLED_KEY,
                value: ActorMetadataValue::Short(0),
            }]),
            properties: Arc::from([]),
            tick: 100,
        }),
    );
    assert_eq!(
        store.get(7).unwrap().wither_query("is_shield_powered", 0.0),
        Some(1.0)
    );
    store.advance_interpolation_ticks(110);
    let actor = store.get(7).unwrap();
    assert_eq!(actor.wither_query("invulnerable_ticks", 0.0), Some(0.0));
    assert_eq!(actor.wither_query("overlay_alpha", 0.0), Some(0.0));
    assert_eq!(actor.wither_query("swell_amount", 1.0), Some(0.0));
}

#[test]
fn wither_death_keeps_upright_interpolates_swell_and_accelerates_armor_flicker() {
    let mut store = wither_store(None);
    store.advance_interpolation_ticks(u32::from(DEATH_TICKS));
    status(&mut store, ActorStatusKind::Death);
    store.advance_interpolation_ticks(1);
    let actor = store.get(7).unwrap();
    assert_eq!(actor.death_rotation_progress(0.5), None);
    assert_eq!(actor.status.death_ticks(), 1);
    for (alpha, expected) in [
        (0.0, 0.0),
        (0.5, 0.5 / SWELL_DIVISOR),
        (1.0, 1.0 / SWELL_DIVISOR),
    ] {
        assert_eq!(actor.wither_query("swell_amount", alpha), Some(expected));
    }
    assert_eq!(
        actor.wither_query("overlay_alpha", 0.0),
        Some(DEATH_OVERLAY_STEP)
    );
    for (ticks, powered) in [(14, 1.0), (13, 0.0), (11, 1.0), (9, 0.0)] {
        store.advance_interpolation_ticks(ticks);
        assert_eq!(
            store.get(7).unwrap().wither_query("is_shield_powered", 0.0),
            Some(powered)
        );
    }
    store.advance_interpolation_ticks(u32::from(DEATH_TICKS));
    let actor = store.get(7).unwrap();
    assert_eq!(actor.status.death_ticks(), DEATH_TICKS);
    assert_eq!(
        actor.wither_query("swell_amount", 1.0),
        Some(f32::from(DEATH_TICKS) / SWELL_DIVISOR)
    );
    assert_eq!(actor.wither_query("invulnerable_ticks", 0.0), Some(0.0));
    status(&mut store, ActorStatusKind::SpawnAlive);
    assert_eq!(store.get(7).unwrap().status.death_ticks(), 0);
    assert_eq!(
        store.get(7).unwrap().wither_query("swell_amount", 1.0),
        Some(0.0)
    );
}

#[test]
fn wither_ignores_wrong_metadata_types_and_frame_reads_do_not_tick_components() {
    let mut store = wither_store(None);
    let actor = store.actors.get_mut(&7).unwrap();
    actor.apply_metadata(&[
        ActorMetadata {
            key: INVULNERABLE_TICKS_KEY,
            value: ActorMetadataValue::Float(f32::NAN),
        },
        ActorMetadata {
            key: SHIELD_DISABLED_KEY,
            value: ActorMetadataValue::Int(0),
        },
    ]);
    let before = actor.clone();
    for alpha in [0.0, 0.3, 1.0] {
        assert_eq!(
            actor.wither_query("invulnerable_ticks", alpha),
            Some(f32::from(DEATH_TICKS))
        );
        assert_eq!(actor.wither_query("is_shield_powered", alpha), Some(0.0));
    }
    store.advance_interpolation_ticks(0);
    assert_eq!(store.get(7).unwrap().status, before.status);
}

#[test]
fn installed_wither_spawn_and_death_drive_the_authored_model_and_render_layers() {
    let root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.local/assets/compiled");
    let entries = match std::fs::read_dir(&root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            eprintln!(
                "skipping wither animation fixture: {} is absent",
                root.display()
            );
            return;
        }
        Err(error) => panic!("read {}: {error}", root.display()),
    };
    let paths: Vec<_> = entries
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "mcbeent")
        })
        .collect();
    if paths.is_empty() {
        eprintln!(
            "skipping wither animation fixture: no entity carrier in {}",
            root.display()
        );
        return;
    }
    assert_eq!(paths.len(), 1, "ambiguous wither fixture: {paths:?}");
    let assets =
        Arc::new(assets::RuntimeEntityAssets::decode(&std::fs::read(&paths[0]).unwrap()).unwrap());
    let mut store = wither_store(Some(assets));
    store.advance_interpolation_ticks(1);
    let rig = store.actor_rig(7).expect("authored wither rig");
    assert_eq!(rig.current.len(), 6);
    assert_eq!(rig.render.len(), 3);
    assert_eq!(rig.render[0].overlay[..3], [1.0; 3]);
    assert!((rig.render[0].overlay[3] - 0.9925).abs() < 1e-5);
    for armor in &rig.render[1..] {
        assert_eq!(armor.hidden_bones.len(), 6, "armor is absent during spawn");
    }
    let tail = rig
        .bone_names
        .iter()
        .position(|name| name.as_ref() == "upperbodypart3")
        .unwrap();
    assert_ne!(
        rig.current[tail], rig.rest[tail],
        "the move controller relocates the tail"
    );
    assert_eq!(rig.axis_scale.map(|axis| axis * rig.scale), [2.0; 3]);
    store.advance_interpolation_ticks(u32::from(DEATH_TICKS));
    status(&mut store, ActorStatusKind::Death);
    store.advance_interpolation_ticks(15);
    let rig = store.actor_rig(7).unwrap();
    assert!(
        rig.axis_scale[0] * rig.scale > 2.0 && rig.axis_scale[1] * rig.scale > 2.0,
        "death swell reaches authored scale"
    );
    assert!(
        rig.render[1..]
            .iter()
            .all(|layer| layer.hidden_bones.is_empty())
    );
    assert!(rig.render[0].overlay[3] > 0.0);
    let before = store.get(7).unwrap().clone();
    let completed_tick = rig.completed_tick;
    let completed_scale = rig.axis_scale;
    let sampled = store.render_frame(0.75).layers(7).unwrap();
    assert!(
        sampled
            .iter()
            .all(|layer| layer.model_scale_ratio == sampled[0].model_scale_ratio)
    );
    assert_ne!(
        sampled[0].model_scale_ratio, [1.0; 3],
        "swell scale samples frame alpha"
    );
    assert_eq!(store.get(7).unwrap(), &before);
    assert_eq!(store.actor_rig(7).unwrap().completed_tick, completed_tick);
    assert_eq!(store.actor_rig(7).unwrap().axis_scale, completed_scale);
    assert_eq!(store.animation_stats().actor_budget_exhaustions, 0);
    assert_eq!(store.animation_stats().frozen_actors, 0);
}
