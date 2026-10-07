use protocol::{ActorEvent, ActorMoveEvent, ActorPositionOrigin, ActorRemoveEvent, ParticleEvent};

use super::*;

fn fireball(identifier: &str) -> ActorEvent {
    let ActorEvent::Spawn(mut actor) = crate::actor_store::tests::spawn(7, 17) else {
        unreachable!();
    };
    actor.kind = ActorKind::Entity {
        identifier: identifier.into(),
    };
    ActorEvent::Spawn(actor)
}

fn store() -> ActorStore {
    let mut store = ActorStore::new(1, 0);
    store.apply(1, 1, fireball("minecraft:dragon_fireball"));
    store
}

fn spawn_event(event: &crate::CommittedParticleEvent) -> &SpawnParticleEffectEvent {
    let ParticleEvent::Spawn(effect) = &event.event else {
        unreachable!()
    };
    effect
}

#[test]
fn dragon_fireball_emits_one_stationary_world_space_trail_particle_each_tick() {
    let mut store = store();
    for _ in 0..8 {
        store.advance_interpolation_ticks(1);
        let effects = store.take_particle_effects();
        assert_eq!(effects.len(), 1);
        assert_eq!(effects[0].sequence, 1);
        assert_eq!(effects[0].dimension, 0);
        let effect = spawn_event(&effects[0]);
        assert_eq!(effect.effect.as_ref(), TRAIL_EFFECT);
        assert_eq!(effect.position, [1.0, 2.0 + TRAIL_HEIGHT, 3.0]);
        assert_eq!(effect.actor_unique_id, None);
        assert_eq!(effect.molang_variables, None);
    }
}

#[test]
fn trails_follow_fixed_tick_interpolation_and_frame_batching_preserves_origins() {
    let mut per_tick = store();
    let mut per_frame = store();
    let ticks = super::super::ACTOR_INTERPOLATION_TICKS;
    let motion = ActorEvent::Move(ActorMoveEvent {
        dimension: 0,
        runtime_id: 7,
        position: [Some(1.0 + ticks as f32), None, None],
        position_origin: ActorPositionOrigin::Feet,
        pitch: None,
        yaw: None,
        head_yaw: None,
        on_ground: None,
        teleported: false,
        player_mode: None,
        source_tick: None,
        interpolation: Default::default(),
    });
    per_tick.apply(1, 2, motion.clone());
    per_frame.apply(1, 2, motion);
    let mut emitted = Vec::new();
    for _ in 0..ticks {
        per_tick.advance_interpolation_ticks(1);
        emitted.extend(per_tick.take_particle_effects());
    }
    per_frame.advance_interpolation_frame(ticks);
    assert_eq!(emitted, per_frame.take_particle_effects());
    let positions: Vec<_> = emitted
        .iter()
        .map(|event| spawn_event(event).position)
        .collect();
    let expected: Vec<_> = (1..=ticks)
        .map(|step| [1.0 + step as f32, 2.0 + TRAIL_HEIGHT, 3.0])
        .collect();
    assert_eq!(positions, expected);
}

#[test]
fn removal_and_dimension_reset_stop_trails_and_other_entities_emit_none() {
    let mut store = store();
    store.advance_interpolation_ticks(1);
    assert_eq!(store.take_particle_effects().len(), 1);
    store.apply(
        1,
        2,
        ActorEvent::Remove(ActorRemoveEvent {
            dimension: 0,
            unique_id: 17,
        }),
    );
    store.advance_interpolation_ticks(1);
    assert!(store.take_particle_effects().is_empty());
    store.apply(1, 3, fireball("minecraft:fireball"));
    store.advance_interpolation_ticks(1);
    assert!(store.take_particle_effects().is_empty());
    store.apply(1, 4, fireball("minecraft:dragon_fireball"));
    store.advance_interpolation_ticks(1);
    store.reset_dimension(1, 5, 1);
    assert!(store.take_particle_effects().is_empty());
    store.advance_interpolation_ticks(1);
    assert!(store.take_particle_effects().is_empty());
}

#[test]
fn invalid_trail_origins_are_skipped_without_removing_the_actor() {
    let mut store = store();
    store.actors.get_mut(&7).unwrap().position[0] = f32::NAN;
    store.advance_dragon_fireball_particles();
    assert!(store.take_particle_effects().is_empty());
    assert!(store.actors.contains_key(&7));
    store.actors.get_mut(&7).unwrap().position[0] = 1.0;
    store.advance_dragon_fireball_particles();
    assert_eq!(store.take_particle_effects().len(), 1);
}
