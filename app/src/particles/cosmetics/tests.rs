use super::*;
use particles::EmptyWorld;
use render_model::ActorEmitterTransform;

const EFFECT: &str = r##"{"particle_effect":{"description":{"identifier":"cinnabar_cosmetic:0123456789abcdef_0",
 "basic_render_parameters":{"material":"particles_alpha","texture":"textures/particle/particles"}},
 "components":{"minecraft:emitter_rate_steady":{"spawn_rate":10,"max_particles":30},
 "minecraft:emitter_lifetime_looping":{"active_time":1},
 "minecraft:particle_lifetime_expression":{"max_lifetime":2},
 "minecraft:particle_appearance_billboard":{"size":[0.1,0.1],"facing_camera_mode":"lookat_xyz"}}}}"##;
const IDENTIFIER: &str = "cinnabar_cosmetic:0123456789abcdef_0";

/// Uses a fully admitted original fixture so every catalog consumer can resolve its rig.
fn item() -> cosmetics::Item {
    render_model::hats::fixture::crown()
}

/// A placement of the fixture effect on one actor at `x`.
fn placement(runtime_id: u64, x: f32) -> CosmeticEmitterPlacement {
    CosmeticEmitterPlacement {
        key: CosmeticEmitterKey {
            runtime_id,
            spawn_revision: 1,
            item: item(),
            emitter: 0,
        },
        identifier: IDENTIFIER.into(),
        effect: EFFECT.as_bytes().into(),
        sprite: None,
        transform: ActorEmitterTransform {
            position: [x, 1.0, 0.0],
            basis: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        },
    }
}

/// Draws every live particle's atlas rectangle from a camera facing the origin.
fn drawn_uvs(system: &mut ParticleSystem) -> Vec<[f32; 4]> {
    let view = particles::ParticleView {
        position: [0.0, 1.0, 4.0],
        right: [1.0, 0.0, 0.0],
        up: [0.0, 1.0, 0.0],
        forward: [0.0, 0.0, -1.0],
        half_diagonal: 1.0,
    };
    let draws = system.build_draw(&view, &EmptyWorld);
    draws
        .opaque
        .iter()
        .chain(&draws.blend)
        .chain(&draws.add)
        .map(|instance| instance.uv)
        .collect()
}

#[test]
fn sprite_emitters_draw_their_own_cosmetic_tile() {
    let mut system = ParticleSystem::default();
    let mut tracker = CosmeticEmitters::default();
    let mut sprite = placement(7, 0.0);
    sprite.effect = EFFECT
        .replace(
            "textures/particle/particles",
            render_model::hats::COSMETIC_PARTICLE_TEXTURE,
        )
        .into_bytes()
        .into();
    let side = render_model::hats::COSMETIC_SPRITE_SIDE as usize;
    sprite.sprite = Some((0x5EED, vec![180; side * side * 4].into()));
    tracker.sync(&mut system, &[sprite]);
    system.tick(0.3, &EmptyWorld);
    let uvs = drawn_uvs(&mut system);
    assert!(!uvs.is_empty());
    let fallback = system.atlas().fallback().normalized();
    assert!(
        uvs.iter().all(|uv| *uv != fallback),
        "sprite particles sample their tile, not the white texel"
    );
}

/// Ids of the emitters still emitting in `system`.
fn emitting(tracker: &CosmeticEmitters, system: &ParticleSystem) -> Vec<u64> {
    tracker
        .live
        .values()
        .map(|live| live.id)
        .filter(|id| system.is_emitting(*id))
        .collect()
}

#[test]
fn cosmetic_emitters_register_lazily_follow_and_stop_with_their_placement() {
    let mut system = ParticleSystem::default();
    let mut tracker = CosmeticEmitters::default();
    assert!(!system.has_effect(IDENTIFIER));
    tracker.sync(&mut system, &[placement(7, 0.0)]);
    assert!(system.has_effect(IDENTIFIER));
    let [id] = emitting(&tracker, &system)[..] else {
        panic!("one emitter starts");
    };
    system.tick(0.2, &EmptyWorld);
    assert!(system.live_particles() > 0);

    tracker.sync(&mut system, &[placement(7, 3.0)]);
    assert_eq!(
        emitting(&tracker, &system),
        [id],
        "a held placement moves its emitter"
    );
    assert_eq!(system.emitter_count(), 1);

    tracker.sync(&mut system, &[]);
    assert!(
        !system.is_emitting(id),
        "unequipped or hidden items stop emitting"
    );
    assert!(tracker.live.is_empty());
}

#[test]
fn cosmetic_emitters_restart_after_the_system_is_cleared_or_rebuilt() {
    let mut system = ParticleSystem::default();
    let mut tracker = CosmeticEmitters::default();
    tracker.sync(&mut system, &[placement(7, 0.0)]);
    let first = emitting(&tracker, &system);
    system.clear();
    tracker.sync(&mut system, &[placement(7, 0.0)]);
    let second = emitting(&tracker, &system);
    assert_eq!(second.len(), 1);
    assert_ne!(first, second, "a cleared emitter restarts");

    let mut rebuilt = ParticleSystem::default();
    tracker.sync(&mut rebuilt, &[placement(7, 0.0)]);
    assert!(
        rebuilt.has_effect(IDENTIFIER),
        "rebuilt systems register again"
    );
    assert_eq!(emitting(&tracker, &rebuilt).len(), 1);
    assert_eq!(tracker.live.len(), 1);
}

#[test]
fn cosmetic_emitters_are_bounded_and_skip_rejected_effects() {
    let mut system = ParticleSystem::default();
    let mut tracker = CosmeticEmitters::default();
    let crowd: Vec<_> = (1..=MAX_COSMETIC_EMITTERS as u64 + 8)
        .map(|runtime_id| placement(runtime_id, 0.0))
        .collect();
    tracker.sync(&mut system, &crowd);
    assert_eq!(emitting(&tracker, &system).len(), MAX_COSMETIC_EMITTERS);
    tracker.sync(&mut system, &crowd[8..]);
    assert_eq!(
        emitting(&tracker, &system).len(),
        MAX_COSMETIC_EMITTERS - 8,
        "stopped emitters hold their slots until they leave the table"
    );
    system.tick(0.01, &EmptyWorld);
    tracker.sync(&mut system, &crowd[8..]);
    assert_eq!(
        emitting(&tracker, &system).len(),
        MAX_COSMETIC_EMITTERS,
        "freed slots admit waiting placements"
    );

    let mut system = ParticleSystem::default();
    let mut tracker = CosmeticEmitters::default();
    let mut broken = placement(9, 0.0);
    broken.effect = b"{\"particle_effect\":7}".as_slice().into();
    tracker.sync(&mut system, &[broken.clone()]);
    tracker.sync(&mut system, &[broken]);
    assert!(tracker.live.is_empty());
    assert_eq!(system.emitter_count(), 0);
    assert!(tracker.rejected.contains(IDENTIFIER));
}

/// Spawns a long-lived non-cosmetic emitter first, so table eviction would remove it first.
/// It is actor-bound because unbound looping emitters finish after one cycle.
fn bystander(system: &mut ParticleSystem) -> u64 {
    let effect = EFFECT.replace(IDENTIFIER, "test:bystander");
    assert!(system.register_effect(effect.as_bytes()));
    system
        .spawn(&SpawnRequest {
            effect: "test:bystander".into(),
            bound: Some((99, [0.0; 3])),
            ..SpawnRequest::default()
        })
        .unwrap()
}

#[test]
fn cosmetics_wait_for_shared_capacity_without_evicting_gameplay_emitters() {
    let mut system = ParticleSystem::default();
    let gameplay: Vec<_> = (0..particles::MAX_EMITTERS)
        .map(|_| bystander(&mut system))
        .collect();
    let mut tracker = CosmeticEmitters::default();
    let placements = [placement(7, 0.0), placement(8, 0.0)];

    tracker.sync(&mut system, &placements);
    assert!(
        tracker.live.is_empty(),
        "a full shared table defers cosmetics"
    );
    assert!(gameplay.iter().all(|id| system.is_emitting(*id)));

    system.stop(gameplay[0]);
    system.tick(0.01, &EmptyWorld);
    assert_eq!(system.emitter_count(), particles::MAX_EMITTERS - 1);
    tracker.sync(&mut system, &placements);
    assert_eq!(emitting(&tracker, &system).len(), 1);
    assert_eq!(system.emitter_count(), particles::MAX_EMITTERS);
    assert!(gameplay[1..].iter().all(|id| system.is_emitting(*id)));
}

/// A crowd filling the cosmetic budget with `effect`.
fn crowd(effect: &str) -> Vec<CosmeticEmitterPlacement> {
    (1..=MAX_COSMETIC_EMITTERS as u64)
        .map(|runtime_id| CosmeticEmitterPlacement {
            effect: effect.as_bytes().into(),
            ..placement(runtime_id, 0.0)
        })
        .collect()
}

#[test]
fn hiding_and_showing_cosmetics_never_exceeds_the_budget_while_particles_drain() {
    let mut system = ParticleSystem::default();
    let other = bystander(&mut system);
    let mut tracker = CosmeticEmitters::default();
    let crowd = crowd(EFFECT);
    for _ in 0..8 {
        tracker.sync(&mut system, &crowd);
        system.tick(0.2, &EmptyWorld);
        assert!(system.emitter_count() <= MAX_COSMETIC_EMITTERS + 1);
        tracker.sync(&mut system, &[]);
        system.tick(0.01, &EmptyWorld);
        assert!(tracker.occupied() <= MAX_COSMETIC_EMITTERS);
        assert!(
            system.is_emitting(other),
            "cosmetics never evict other effects"
        );
    }
}

#[test]
fn finished_emitters_restart_without_leaking_slots() {
    let mut system = ParticleSystem::default();
    let other = bystander(&mut system);
    let mut tracker = CosmeticEmitters::default();
    // Unbound looping emitters finish after one cycle while their particles drain.
    let brief = EFFECT.replace(r#"{"active_time":1}"#, r#"{"active_time":0.3}"#);
    assert_ne!(brief, EFFECT);
    let crowd = crowd(&brief);
    tracker.sync(&mut system, &crowd);
    let first: HashSet<u64> = tracker.live.values().map(|live| live.id).collect();
    assert_eq!(first.len(), MAX_COSMETIC_EMITTERS);
    let mut restarted = false;
    for _ in 0..24 {
        system.tick(0.25, &EmptyWorld);
        tracker.sync(&mut system, &crowd);
        restarted |= tracker.live.values().any(|live| !first.contains(&live.id));
        assert!(tracker.occupied() <= MAX_COSMETIC_EMITTERS);
        assert!(system.emitter_count() <= MAX_COSMETIC_EMITTERS + 1);
        assert!(
            system.is_emitting(other),
            "cosmetics never evict other effects"
        );
    }
    assert!(
        restarted,
        "finished emitters restart while their items are drawn"
    );
}
