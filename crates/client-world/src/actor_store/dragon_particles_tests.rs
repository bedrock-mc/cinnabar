use protocol::{ActorEvent, ActorStatusEvent, ActorStatusKind};

use super::*;
use crate::actor_store::{ActorApplyResult, DEATH_DURATION_TICKS};

fn dying(identifier: &str) -> ActorStore {
    let mut store = ActorStore::new(1, 0);
    store.apply(1, 1, spawn_kind(identifier));
    store.apply(
        1,
        2,
        ActorEvent::Status(ActorStatusEvent {
            runtime_id: 7,
            kind: ActorStatusKind::Death,
            data: 0,
        }),
    );
    store
}

fn spawn_kind(identifier: &str) -> ActorEvent {
    let ActorEvent::Spawn(mut actor) = crate::actor_store::tests::spawn(7, 17) else {
        unreachable!();
    };
    actor.kind = ActorKind::Entity {
        identifier: identifier.into(),
    };
    ActorEvent::Spawn(actor)
}

#[test]
fn dragon_death_does_not_start_an_ordinary_red_overlay() {
    let mut store = dying("minecraft:ender_dragon");
    assert!(!store.actors[&7].hurt_overlay_active());
    store.apply(
        1,
        3,
        ActorEvent::Status(ActorStatusEvent {
            runtime_id: 7,
            kind: ActorStatusKind::Hurt,
            data: 0,
        }),
    );
    assert!(store.actors[&7].hurt_overlay_active());
    store.advance_interpolation_ticks(u32::from(super::super::HURT_DURATION_TICKS));
    assert!(!store.actors[&7].hurt_overlay_active());
    assert!(dying("minecraft:bee").actors[&7].hurt_overlay_active());
}

#[test]
fn dragon_death_status_resets_its_native_counter_without_inheriting_mob_ticks() {
    let mut store = dying("minecraft:ender_dragon");
    store.advance_interpolation_ticks(40);
    let previous_position = store.actors[&7].position;
    assert_eq!(store.actors[&7].status.death_ticks(), 41);
    store.apply(
        1,
        3,
        ActorEvent::Status(ActorStatusEvent {
            runtime_id: 7,
            kind: ActorStatusKind::Death,
            data: 0,
        }),
    );
    assert_eq!(store.actors[&7].status.death_ticks(), 1);
    assert_eq!(store.actors[&7].position, previous_position);
    store.advance_interpolation_ticks(1);
    assert_eq!(store.actors[&7].status.death_ticks(), 2);
    assert!((store.actors[&7].position[1] - previous_position[1] - 0.1).abs() < 0.0001);
}

#[test]
fn dragon_death_emits_once_per_tick_past_the_ordinary_death_duration() {
    let mut store = dying("minecraft:ender_dragon");
    store.advance_interpolation_ticks(u32::from(DRAGON_DEATH_TICKS));
    assert_eq!(store.actors[&7].status.death_ticks(), DRAGON_DEATH_TICKS);
    let effects = store.take_particle_effects();
    assert_eq!(
        effects.len(),
        usize::from(DRAGON_DEATH_TICKS + DRAGON_DEATH_TICKS - FINAL_EXPLOSION_START)
    );
    let effects: Vec<_> = effects
        .into_iter()
        .map(|event| match event.event {
            ParticleEvent::Spawn(effect) => effect,
            _ => unreachable!(),
        })
        .collect();
    assert_eq!(
        effects
            .iter()
            .filter(|effect| effect.effect.as_ref() == DYING_EXPLOSION)
            .count(),
        usize::from(DRAGON_DEATH_TICKS - 1)
    );
    assert!(effects.iter().enumerate().all(|(index, effect)| {
        let [x, y, z] = effect.position;
        let tick = if index < usize::from(FINAL_EXPLOSION_START - 2) {
            index + 1
        } else {
            usize::from(FINAL_EXPLOSION_START - 1)
                + (index - usize::from(FINAL_EXPLOSION_START - 2)) / 2
        };
        let lift = (tick - usize::from(effect.effect.as_ref() == DEATH_EXPLOSION)) as f32
            * DEATH_LIFT_PER_TICK;
        (-3.0..=5.0).contains(&x)
            && (2.0 + lift - 0.0001..=6.0 + lift + 0.0001).contains(&y)
            && (-1.0..=7.0).contains(&z)
    }));
    store.advance_interpolation_ticks(1);
    assert!(store.take_particle_effects().is_empty());
}

#[test]
fn dragon_death_lifts_the_body_each_tick_and_keeps_the_previous_pose() {
    let mut store = dying("minecraft:ender_dragon");
    let initial = store.actors[&7].position;
    assert_eq!(store.actors[&7].status.death_ticks(), 1);
    for tick in 1..DRAGON_DEATH_TICKS {
        store.advance_interpolation_ticks(1);
        let actor = &store.actors[&7];
        assert!(
            (actor.position[1] - (initial[1] + tick as f32 * 0.1)).abs() < 0.0001,
            "death tick {tick}: body must rise rather than snap back to its network target"
        );
        assert!(
            (actor.previous_pose.position[1] - (initial[1] + (tick - 1) as f32 * 0.1)).abs()
                < 0.0001
        );
        assert_eq!(actor.native_velocity(), [0.0; 3]);
        assert_eq!(actor.death_rotation_progress(0.0), None);
        assert_eq!(actor.status.death_ticks(), tick + 1);
    }
}

#[test]
fn dragon_death_lift_resumes_from_later_authoritative_movement_targets() {
    let mut store = dying("minecraft:ender_dragon");
    store.advance_interpolation_ticks(40);
    store.apply(
        1,
        3,
        protocol::ActorEvent::Move(protocol::ActorMoveEvent {
            dimension: 0,
            runtime_id: 7,
            position: [None, Some(20.0), None],
            position_origin: protocol::ActorPositionOrigin::Feet,
            pitch: None,
            yaw: None,
            head_yaw: None,
            on_ground: None,
            teleported: false,
            player_mode: None,
            source_tick: None,
            interpolation: Default::default(),
        }),
    );
    assert_eq!(store.actors[&7].interpolation_ticks_remaining, 3);
    store.advance_interpolation_ticks(3);
    let actor = &store.actors[&7];
    assert!((actor.position[1] - 20.1).abs() < 0.0001);
    assert_eq!(actor.received_pose.position[1], 20.0);
    assert_eq!(actor.interpolation_ticks_remaining, 0);
    store.advance_interpolation_ticks(1);
    let actor = &store.actors[&7];
    assert!((actor.previous_pose.position[1] - 20.1).abs() < 0.0001);
    assert!((actor.position[1] - 20.2).abs() < 0.0001);
    assert_eq!(actor.received_pose.position[1], 20.0);
}

#[test]
fn frame_batching_preserves_every_fixed_tick_effect_and_position() {
    let mut per_tick = dying("minecraft:ender_dragon");
    let mut per_frame = dying("minecraft:ender_dragon");
    let mut effects = Vec::new();
    let ticks = u32::from(FINAL_EXPLOSION_START + 10);
    for _ in 0..ticks {
        per_tick.advance_interpolation_ticks(1);
        effects.extend(per_tick.take_particle_effects());
    }
    per_frame.advance_interpolation_frame(ticks);
    assert_eq!(effects, per_frame.take_particle_effects());
    assert_eq!(
        u32::from(per_frame.actors[&7].status.death_ticks()),
        ticks + 1
    );
}

#[test]
fn final_explosion_bursts_use_the_once_only_dragon_effect_at_both_boundaries() {
    let mut store = dying("minecraft:ender_dragon");
    store.advance_interpolation_ticks(u32::from(FINAL_EXPLOSION_START - 2));
    let initial = store.take_particle_effects();
    assert!(initial.into_iter().all(|event| matches!(event.event,
        ParticleEvent::Spawn(effect) if effect.effect.as_ref() == DYING_EXPLOSION)));
    for _ in FINAL_EXPLOSION_START..=DRAGON_DEATH_TICKS {
        store.advance_interpolation_ticks(1);
        let effects = store.take_particle_effects();
        let names: Vec<_> = effects
            .iter()
            .map(|event| match &event.event {
                ParticleEvent::Spawn(effect) => effect.effect.as_ref(),
                _ => unreachable!(),
            })
            .collect();
        assert_eq!(names, [DEATH_EXPLOSION, DYING_EXPLOSION]);
    }
    store.advance_interpolation_ticks(1);
    assert!(store.take_particle_effects().is_empty());
}

#[test]
fn ordinary_mobs_keep_their_death_timing_and_have_no_dragon_particles() {
    let mut store = dying("minecraft:bee");
    store.advance_interpolation_ticks(25);
    assert!(store.take_particle_effects().is_empty());
    assert_eq!(
        store.actors[&7].status.death_ticks(),
        u16::from(DEATH_DURATION_TICKS)
    );
    assert_eq!(store.actors[&7].status.death_progress(0.0), Some(1.0));
    assert_eq!(store.actors[&7].death_rotation_progress(0.0), Some(1.0));
}

#[test]
fn revival_and_dimension_reset_clear_dragon_death_effect_state() {
    let mut store = dying("minecraft:ender_dragon");
    store.advance_interpolation_ticks(21);
    store.take_particle_effects();
    store.apply(
        1,
        3,
        ActorEvent::Status(ActorStatusEvent {
            runtime_id: 7,
            kind: ActorStatusKind::SpawnAlive,
            data: 0,
        }),
    );
    store.advance_interpolation_ticks(1);
    assert_eq!(store.actors[&7].status.death_ticks(), 0);
    assert!(store.take_particle_effects().is_empty());
    let mut store = dying("minecraft:ender_dragon");
    store.advance_interpolation_ticks(1);
    assert_eq!(store.reset_dimension(1, 3, 1), ActorApplyResult::Reset);
    assert!(store.take_particle_effects().is_empty());
}

#[test]
fn authority_publishes_tick_effects_through_the_existing_particle_drain() {
    let mut authority = crate::WorldAuthority::new(
        protocol::WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: 1,
            local_player_unique_id: 1,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: 0,
            block_network_ids_are_hashes: true,
        },
        std::sync::Arc::new(assets::RuntimeAssets::diagnostic()),
        None,
        [0.0; 3],
        None,
    );
    authority
        .apply_ordered_event(
            protocol::WorldEvent::Actor(spawn_kind("minecraft:ender_dragon")),
            Some(1),
        )
        .unwrap();
    authority
        .apply_ordered_event(
            protocol::WorldEvent::Actor(ActorEvent::Status(ActorStatusEvent {
                runtime_id: 7,
                kind: ActorStatusKind::Death,
                data: 0,
            })),
            Some(2),
        )
        .unwrap();
    authority.advance_actor_interpolation_frame(21);
    let effects = authority.take_committed_particles();
    assert_eq!(effects.len(), 21);
    assert!(
        effects
            .iter()
            .all(|effect| effect.sequence == 2 && effect.dimension == 0)
    );
    assert!(authority.take_committed_particles().is_empty());
    authority
        .apply_ordered_event(
            protocol::WorldEvent::Actor(ActorEvent::Remove(protocol::ActorRemoveEvent {
                dimension: 0,
                unique_id: 17,
            })),
            Some(3),
        )
        .unwrap();
    authority.advance_actor_interpolation_frame(1);
    assert!(authority.take_committed_particles().is_empty());
}
