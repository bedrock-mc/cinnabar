use protocol::{ActorEvent, ActorKind, ActorMetadata, ActorMetadataUpdateEvent, ParticleEvent};
use protocol_ids::ActorDataKey;

use super::*;

fn cloud() -> ActorEvent {
    let ActorEvent::Spawn(mut actor) = crate::actor_store::tests::spawn(7, 17) else {
        unreachable!();
    };
    actor.kind = ActorKind::Entity {
        identifier: "minecraft:area_effect_cloud".into(),
    };
    actor.metadata = [
        ActorMetadata {
            key: ActorDataKey::DataRadius.wire_value(),
            value: ActorMetadataValue::Float(3.5),
        },
        ActorMetadata {
            key: ActorDataKey::DataParticle.wire_value(),
            value: ActorMetadataValue::Int(DRAGON_BREATH_PARTICLE),
        },
        ActorMetadata {
            key: ActorDataKey::DataDuration.wire_value(),
            value: ActorMetadataValue::Int(10),
        },
        ActorMetadata {
            key: ActorDataKey::DataChangeRate.wire_value(),
            value: ActorMetadataValue::Float(0.0),
        },
    ]
    .into();
    ActorEvent::Spawn(actor)
}

fn cloud_store() -> ActorStore {
    let mut store = ActorStore::new(1, 0);
    store.apply(1, 1, cloud());
    store
}

fn effects(store: &mut ActorStore) -> Vec<SpawnParticleEffectEvent> {
    store
        .take_particle_effects()
        .into_iter()
        .map(|event| {
            assert_eq!(event.dimension, 0);
            assert_eq!(event.sequence, store.latest_sequence);
            match event.event {
                ParticleEvent::Spawn(effect) => effect,
                _ => unreachable!(),
            }
        })
        .collect()
}

fn variables(effect: &SpawnParticleEffectEvent) -> std::collections::HashMap<&str, f32> {
    effect
        .molang_variables
        .as_deref()
        .unwrap()
        .strip_prefix('{')
        .unwrap()
        .strip_suffix('}')
        .unwrap()
        .split(',')
        .map(|entry| {
            let (name, value) = entry.split_once(':').unwrap();
            (
                name.strip_prefix('"').unwrap().strip_suffix('"').unwrap(),
                value.parse().unwrap(),
            )
        })
        .collect()
}

fn update(store: &mut ActorStore, key: u32, value: ActorMetadataValue) {
    store.apply(
        1,
        store.latest_sequence + 1,
        ActorEvent::Metadata(ActorMetadataUpdateEvent {
            dimension: store.dimension,
            runtime_id: 7,
            metadata: [ActorMetadata { key, value }].into(),
            properties: [].into(),
            tick: u64::from(store.actors[&7].status.age_ticks),
        }),
    );
}

#[test]
fn cloud_anchors_first_tick_then_emits_every_five_through_inclusive_expiry() {
    let mut store = cloud_store();
    store.advance_interpolation_ticks(EMISSION_TICKS);
    assert!(effects(&mut store).is_empty());
    store.advance_interpolation_ticks(1);
    let first = effects(&mut store);
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].effect.as_ref(), LINGERING_BREATH);
    assert_eq!(first[0].position, [1.0, 2.0, 3.0]);
    assert_eq!(first[0].actor_unique_id, None);
    let vars = variables(&first[0]);
    assert_eq!(
        vars["variable.cloud_lifetime"],
        EMISSION_TICKS as f32 * simulation_clock::TICK_DURATION.as_secs_f32()
    );
    assert_eq!(vars["variable.cloud_radius"], 3.5);
    assert_eq!(vars["variable.particle_multiplier"], EMISSION_TICKS as f32);
    store.advance_interpolation_ticks(EMISSION_TICKS);
    assert_eq!(effects(&mut store).len(), 1);
    store.advance_interpolation_ticks(EMISSION_TICKS);
    assert!(effects(&mut store).is_empty());
    assert!(store.actors.contains_key(&7));
}

#[test]
fn effective_radius_tracks_elapsed_ticks_and_synced_pickups() {
    let mut store = cloud_store();
    update(
        &mut store,
        ActorDataKey::DataRadius.wire_value(),
        ActorMetadataValue::Float(2.0),
    );
    update(
        &mut store,
        ActorDataKey::DataChangeRate.wire_value(),
        ActorMetadataValue::Float(0.1),
    );
    update(
        &mut store,
        ActorDataKey::DataChangeOnPickup.wire_value(),
        ActorMetadataValue::Float(-0.25),
    );
    update(
        &mut store,
        ActorDataKey::DataPickupCount.wire_value(),
        ActorMetadataValue::Int(2),
    );
    store.advance_interpolation_ticks(EMISSION_TICKS + 1);
    assert_eq!(
        variables(&effects(&mut store)[0])["variable.cloud_radius"],
        2.0
    );
    update(
        &mut store,
        ActorDataKey::DataPickupCount.wire_value(),
        ActorMetadataValue::Int(-3),
    );
    store.advance_interpolation_ticks(EMISSION_TICKS);
    assert_eq!(
        variables(&effects(&mut store)[0])["variable.cloud_radius"],
        3.0
    );
}

#[test]
fn radius_expiration_between_emissions_cannot_be_revived_by_late_metadata() {
    let mut store = cloud_store();
    update(
        &mut store,
        ActorDataKey::DataRadius.wire_value(),
        ActorMetadataValue::Float(1.0),
    );
    update(
        &mut store,
        ActorDataKey::DataChangeRate.wire_value(),
        ActorMetadataValue::Float(-0.25),
    );
    store.advance_interpolation_ticks(4);
    assert!(effects(&mut store).is_empty());
    assert!(store.actors[&7].status.cloud_particles_expired);
    update(
        &mut store,
        ActorDataKey::DataChangeRate.wire_value(),
        ActorMetadataValue::Float(1.0),
    );
    store.advance_interpolation_ticks(10);
    assert!(effects(&mut store).is_empty());
}

#[test]
fn minimum_radius_and_unlimited_duration_are_valid() {
    let mut store = cloud_store();
    update(
        &mut store,
        ActorDataKey::DataRadius.wire_value(),
        ActorMetadataValue::Float(MIN_RADIUS),
    );
    update(
        &mut store,
        ActorDataKey::DataDuration.wire_value(),
        ActorMetadataValue::Int(-1),
    );
    store.advance_interpolation_ticks(EMISSION_TICKS * 8 + 1);
    let effects = effects(&mut store);
    assert_eq!(effects.len(), 8);
    assert!(
        effects
            .iter()
            .all(|effect| variables(effect)["variable.cloud_radius"] == MIN_RADIUS)
    );
}

#[test]
fn missing_required_wrong_typed_and_nonfinite_metadata_stay_quiet() {
    for (key, value) in [
        (ActorDataKey::DataRadius.wire_value(), None),
        (ActorDataKey::DataDuration.wire_value(), None),
        (
            ActorDataKey::DataRadius.wire_value(),
            Some(ActorMetadataValue::Float(f32::NAN)),
        ),
        (
            ActorDataKey::DataChangeRate.wire_value(),
            Some(ActorMetadataValue::Float(f32::INFINITY)),
        ),
        (
            ActorDataKey::DataChangeOnPickup.wire_value(),
            Some(ActorMetadataValue::Float(f32::NEG_INFINITY)),
        ),
        (
            ActorDataKey::DataDuration.wire_value(),
            Some(ActorMetadataValue::Int(-2)),
        ),
        (
            ActorDataKey::DataDuration.wire_value(),
            Some(ActorMetadataValue::Long(10)),
        ),
    ] {
        let mut store = cloud_store();
        match value {
            Some(value) => {
                store
                    .actors
                    .get_mut(&7)
                    .unwrap()
                    .metadata
                    .insert(key, value);
            }
            None => {
                store.actors.get_mut(&7).unwrap().metadata.remove(&key);
            }
        }
        store.advance_interpolation_ticks(11);
        assert!(effects(&mut store).is_empty(), "metadata key {key}");
        assert!(store.actors.contains_key(&7));
    }
}

#[test]
fn omitted_optional_metadata_preserves_cloud_constructor_defaults() {
    let mut store = cloud_store();
    store
        .actors
        .get_mut(&7)
        .unwrap()
        .metadata
        .remove(&ActorDataKey::DataChangeRate.wire_value());
    update(
        &mut store,
        ActorDataKey::DataRadius.wire_value(),
        ActorMetadataValue::Float(10.0),
    );
    store.advance_interpolation_ticks(EMISSION_TICKS + 1);
    assert_eq!(
        variables(&effects(&mut store)[0])["variable.cloud_radius"],
        5.0
    );
    store.advance_interpolation_ticks(EMISSION_TICKS);
    assert!(effects(&mut store).is_empty());
    let mut store = cloud_store();
    update(
        &mut store,
        ActorDataKey::DataPickupCount.wire_value(),
        ActorMetadataValue::Int(2),
    );
    store.advance_interpolation_ticks(EMISSION_TICKS + 1);
    assert_eq!(
        variables(&effects(&mut store)[0])["variable.cloud_radius"],
        2.5
    );
}

#[test]
fn fixed_tick_batching_preserves_cloud_requests_and_shared_queue_stays_bounded() {
    let mut per_tick = cloud_store();
    let mut per_frame = cloud_store();
    let mut emitted = Vec::new();
    for _ in 0..11 {
        per_tick.advance_interpolation_ticks(1);
        emitted.extend(per_tick.take_particle_effects());
    }
    per_frame.advance_interpolation_frame(11);
    assert_eq!(emitted, per_frame.take_particle_effects());
    for runtime_id in 8..8 + crate::COMMITTED_PARTICLE_CAPACITY as u64 + 5 {
        let ActorEvent::Spawn(mut actor) = cloud() else {
            unreachable!()
        };
        actor.runtime_id = runtime_id;
        actor.unique_id = runtime_id as i64;
        per_frame.apply(1, runtime_id, ActorEvent::Spawn(actor));
    }
    per_frame.advance_interpolation_ticks(EMISSION_TICKS + 1);
    assert_eq!(
        per_frame.take_particle_effects().len(),
        crate::COMMITTED_PARTICLE_CAPACITY
    );
    assert!(per_frame.take_particle_effects().is_empty());
    per_frame.reset_dimension(1, u64::MAX, 1);
    assert!(per_frame.take_particle_effects().is_empty());
}

#[test]
fn facts_migration_custom_clouds_inherit_the_compiled_particle_route() {
    let mut expected = cloud_store();
    expected.advance_interpolation_ticks(EMISSION_TICKS + 1);
    let expected = effects(&mut expected);
    assert_eq!(expected.len(), 1);
    for late in [false, true] {
        let mut store = ActorStore::new(1, 0);
        let registry = || protocol::ActorIdentifierRegistry {
            entries: [protocol::ActorIdentifier {
                identifier: "fixture:breath".into(),
                base_identifier: "minecraft:area_effect_cloud".into(),
            }]
            .into(),
            skipped: 0,
        };
        if !late {
            store.apply_actor_interactions(registry());
        }
        let ActorEvent::Spawn(mut spawn) = cloud() else {
            unreachable!()
        };
        spawn.kind = ActorKind::Entity {
            identifier: "fixture:breath".into(),
        };
        store.apply(1, 1, ActorEvent::Spawn(spawn));
        if late {
            store.apply_actor_interactions(registry());
        }
        store.advance_interpolation_ticks(EMISSION_TICKS + 1);
        assert_eq!(effects(&mut store), expected);
    }
}

#[test]
fn potion_clouds_emit_every_two_ticks_with_argb_color_and_shared_radius() {
    let mut store = cloud_store();
    update(
        &mut store,
        ActorDataKey::DataParticle.wire_value(),
        ActorMetadataValue::Int(32),
    );
    update(
        &mut store,
        ActorDataKey::EffectColor.wire_value(),
        ActorMetadataValue::Int(0x80402010_u32 as i32),
    );
    store.advance_interpolation_ticks(2);
    assert!(effects(&mut store).is_empty());
    store.advance_interpolation_ticks(1);
    let emitted = effects(&mut store);
    assert_eq!(emitted.len(), 1);
    assert_eq!(
        emitted[0].effect.as_ref(),
        assets::facts::ParticleEffect::MobspellLingering.identifier()
    );
    let values = variables(&emitted[0]);
    for (component, byte) in [("r", 64_u8), ("g", 32), ("b", 16), ("a", 128)] {
        assert!(
            (values[format!("variable.color.{component}").as_str()] - f32::from(byte) / 255.0)
                .abs()
                < 0.00001
        );
    }
    assert_eq!(values["variable.cloud_lifetime"], 0.1);
    assert_eq!(values["variable.cloud_radius"], 3.5);
    assert_eq!(values["variable.particle_multiplier"], 2.0);
    store.advance_interpolation_ticks(1);
    assert!(effects(&mut store).is_empty());
    store.advance_interpolation_ticks(1);
    assert_eq!(effects(&mut store).len(), 1);
}

#[test]
fn non_dragon_profiles_and_missing_profile_use_the_potion_emitter() {
    for profile in [
        None,
        Some(ActorMetadataValue::Int(0)),
        Some(ActorMetadataValue::Int(i32::MAX)),
        Some(ActorMetadataValue::Float(49.0)),
    ] {
        let mut store = cloud_store();
        let actor = store.actors.get_mut(&7).unwrap();
        actor
            .metadata
            .remove(&ActorDataKey::DataParticle.wire_value());
        if let Some(value) = profile {
            actor
                .metadata
                .insert(ActorDataKey::DataParticle.wire_value(), value);
        }
        store.advance_interpolation_ticks(3);
        let emitted = effects(&mut store);
        assert_eq!(emitted.len(), 1);
        assert_eq!(
            emitted[0].effect.as_ref(),
            assets::facts::ParticleEffect::MobspellLingering.identifier()
        );
        assert_eq!(variables(&emitted[0])["variable.color.a"], 0.0);
    }
}
