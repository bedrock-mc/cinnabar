use super::*;
use crate::actor_animation::{
    ActorTickContext, ActorTickInput, evaluation::MolangValue, query::QueryInputs,
    tests::actor_with_metadata,
};
use protocol::{ActorAttribute, ActorMetadataValue};
use std::{collections::HashMap, sync::Arc};

fn wolf(flags: u64, health: Option<(f32, f32)>) -> ActorSnapshot {
    let mut actor = actor_with_metadata(HashMap::from([(0, ActorMetadataValue::Flags(flags))]));
    actor.kind = ActorKind::Entity {
        identifier: "minecraft:wolf".into(),
    };
    if let Some((current, max)) = health {
        actor.attributes.insert(
            "minecraft:health".into(),
            ActorAttribute {
                name: "minecraft:health".into(),
                min: 0.0,
                max,
                current,
                default: None,
                modifiers: Arc::from([]),
            },
        );
    }
    actor
}

fn read_tail(actor: &ActorSnapshot, ticks: u64) -> f32 {
    let input = ActorTickInput::default();
    let context = ActorTickContext::default();
    let query = QueryInputs {
        actor,
        input: &input,
        context: &context,
        anim_tick: ticks,
        anim_time: None,
        swell_amount: None,
        life_tick: ticks,
        finished: (false, false),
        state_time: 0.0,
        bones: &[],
        bone_names: &[],
    };
    match super::super::named_query(&query, "query.tail_angle", &[]) {
        MolangValue::Number(value) => value,
        value => panic!("tail angle is not numeric: {value:?}"),
    }
}

#[test]
fn wild_and_angry_wolf_tail_angles_match_the_native_float_bits() {
    assert_eq!(read_tail(&wolf(0, None), 0).to_bits(), 0x3f20_d97c);
    for flags in [1 << FLAG_ANGRY, (1 << FLAG_ANGRY) | (1 << FLAG_TAMED)] {
        let actor = wolf(flags, Some((0.0, 40.0)));
        assert_eq!(read_tail(&actor, 0).to_bits(), 0x3fc5_0a6b);
        assert_eq!(read_tail(&actor, 103), read_tail(&actor, 0));
    }
}

#[test]
fn tamed_wolf_tail_uses_the_health_fraction_without_a_pose_clamp() {
    for (current, max) in [(0.0, 40.0), (20.0, 40.0), (40.0, 40.0), (48.0, 40.0)] {
        let actor = wolf(1 << FLAG_TAMED, Some((current, max)));
        let expected = (current / max * HEALTH_WEIGHT + HEALTH_BASE) * std::f32::consts::PI;
        assert_eq!(read_tail(&actor, 0), expected);
        assert_eq!(read_tail(&actor, 200), expected);
    }
}

#[test]
fn non_wolves_never_acquire_a_wolf_tail_default() {
    let mut actor = wolf((1 << FLAG_ANGRY) | (1 << FLAG_TAMED), Some((40.0, 40.0)));
    actor.kind = ActorKind::Entity {
        identifier: "minecraft:cat".into(),
    };
    assert_eq!(read_tail(&actor, 0), 0.0);
}

#[test]
fn incomplete_or_odd_health_keeps_a_finite_tail_until_authoritative_data_arrives() {
    for health in [
        None,
        Some((20.0, 0.0)),
        Some((f32::NAN, 40.0)),
        Some((20.0, f32::INFINITY)),
        Some((f32::MAX, f32::MIN_POSITIVE)),
    ] {
        assert_eq!(read_tail(&wolf(1 << FLAG_TAMED, health), 0), WILD_ANGLE);
    }
    let mut actor = wolf(1 << FLAG_TAMED, None);
    actor.attributes = wolf(1 << FLAG_TAMED, Some((40.0, 40.0))).attributes;
    assert_ne!(read_tail(&actor, 0), WILD_ANGLE);
}

#[test]
#[ignore = "requires CINNABAR_ENTITY_CARRIER pointing to the pinned compiled entity carrier"]
fn pinned_wolf_clips_rotate_the_existing_tail_for_wild_tame_angry_and_baby_actors() {
    use crate::actor_animation::{ActorAnimationStore, pose::quat_from_euler};
    use assets::RuntimeEntityAssets;

    let bytes = std::fs::read(std::env::var_os("CINNABAR_ENTITY_CARRIER").unwrap()).unwrap();
    let assets = Arc::new(RuntimeEntityAssets::decode(&bytes).unwrap());
    // This is the multiplier authored in the pinned wolf.tail_default clip. Its
    // slightly rounded degree conversion is pack behavior, not query behavior.
    const PACK_DEGREES_PER_RADIAN: f32 = 57.3;
    for (flags, health, pivot) in [
        (0, None, [1.0, 12.0, 8.0]),
        (1 << FLAG_TAMED, Some((40.0, 40.0)), [1.0, 12.0, 8.0]),
        (1 << FLAG_TAMED, Some((20.0, 40.0)), [1.0, 12.0, 8.0]),
        (
            (1 << FLAG_TAMED) | (1 << FLAG_ANGRY),
            Some((40.0, 40.0)),
            [1.0, 12.0, 8.0],
        ),
        (
            1 << crate::actor_animation::query::FLAG_BABY,
            None,
            [0.0, 5.0, 3.0],
        ),
    ] {
        let actor = wolf(flags, health);
        let mut store = ActorAnimationStore::with_assets(Arc::clone(&assets));
        store.insert(1, 0, &actor);
        let expected = quat_from_euler([-tail_angle(&actor) * PACK_DEGREES_PER_RADIAN, 0.0, 0.0]);
        for _ in 0..4 {
            store.advance_tick(
                &HashMap::from([(actor.runtime_id, actor.clone())]),
                None,
                None,
                true,
                true,
                |_| ActorTickContext::default(),
            );
            let snapshot = store.get(actor.runtime_id).expect("native wolf rig");
            let tail = snapshot
                .bone_names
                .iter()
                .position(|name| name.as_ref() == "tail")
                .expect("the model already contains a tail");
            assert_eq!(snapshot.current[tail].translation_scale[..3], pivot);
            for (actual, expected) in snapshot.current[tail].rotation.into_iter().zip(expected) {
                assert!(
                    (actual - expected).abs() < 1e-5,
                    "flags {flags}: {actual} != {expected}"
                );
            }
            assert_ne!(
                snapshot.current[tail].rotation,
                snapshot.rest[tail].rotation
            );
        }
    }
}
