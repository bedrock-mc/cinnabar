use super::*;
use particles::EmptyWorld;

struct Floor;
impl ParticleWorld for Floor {
    fn solid_boxes(&self, _: [f32; 3], _: [f32; 3], out: &mut Vec<[f32; 6]>) {
        out.push([-100.0, -1.0, -100.0, 100.0, 0.0, 100.0]);
    }
    fn light(&self, _: [i32; 3]) -> (u8, u8) {
        (0, 15)
    }
    fn fluid(&self, _: [i32; 3]) -> Fluid {
        Fluid::None
    }
}

/// Routes the same descending remote motion under both render cadences.
fn descent(system: &mut ParticleSystem, hz: u32) {
    let mut track = Track::default();
    let mut draining = Vec::new();
    let mut rejected = HashSet::new();
    let wearer = GroundEffectWearer {
        runtime_id: 1,
        spawn_revision: 1,
        item: hats::fixture::ground_effect(),
    };
    for tick in 0..=hz {
        let feet = [0.0, (0.6 - tick as f32 * 1.2 / hz as f32).max(0.0), 0.0];
        // Server's landing target arrives while the body still visibly falls.
        let grounded = remote_grounded(&Floor, feet, tick as f32 / hz as f32 >= 0.15);
        contact(
            system,
            &Floor,
            wearer,
            &mut track,
            feet,
            grounded,
            true,
            1.0 / hz as f32,
            tick as u64,
            &mut draining,
            &mut rejected,
        );
    }
}

#[test]
fn a_remote_early_ground_flag_keeps_its_landing_until_visible_feet_reach_support() {
    for hz in [20, 120] {
        let mut system = ParticleSystem::default();
        descent(&mut system, hz);
        assert_eq!(system.emitter_count(), 1, "one landing at {hz} FPS");
    }
}

#[test]
fn ground_cosmetics_do_not_evict_world_emitters_when_the_global_table_is_full() {
    let mut system = ParticleSystem::default();
    assert!(system.register_effect(hats::fixture::EMITTER_EFFECT.as_bytes()));
    let ids: Vec<_> = (0..particles::MAX_EMITTERS)
        .map(|_| {
            system
                .spawn(&SpawnRequest {
                    effect: "fixture:spark".into(),
                    ..Default::default()
                })
                .unwrap()
        })
        .collect();
    descent(&mut system, 20);
    assert_eq!(system.emitter_count(), particles::MAX_EMITTERS);
    assert!(ids.into_iter().all(|id| system.contains_emitter(id)));
    system.tick(0.05, &EmptyWorld);
}
