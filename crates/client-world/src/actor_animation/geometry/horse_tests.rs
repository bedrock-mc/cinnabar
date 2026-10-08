use super::super::*;
use super::fixtures::entity_assets as fixture;

fn horse(flags: u64) -> ActorSnapshot {
    let mut actor =
        tests::actor_with_metadata(HashMap::from([(0, ActorMetadataValue::Flags(flags))]));
    actor.kind = ActorKind::Entity {
        identifier: "minecraft:horse".into(),
    };
    actor.on_ground = Some(true);
    actor
}

fn tick(store: &mut ActorAnimationStore, actor: &ActorSnapshot, has_rider: bool) {
    store.advance_tick(
        &HashMap::from([(actor.runtime_id, actor.clone())]),
        None,
        None,
        true,
        false,
        |_| ActorTickContext {
            has_rider,
            ..Default::default()
        },
    );
    let stats = store.stats();
    assert_eq!(stats.actor_budget_exhaustions, 0);
    assert_eq!(stats.world_budget_exhaustions, 0);
    assert_eq!(stats.frozen_actors, 0);
}

fn bone(rig: &ActorRigSnapshot<'_>, name: &str) -> usize {
    rig.bone_names
        .iter()
        .position(|candidate| candidate.as_ref() == name)
        .unwrap_or_else(|| panic!("horse rig has {name} bone"))
}

fn visible(rig: &ActorRigSnapshot<'_>, name: &str) -> bool {
    let index = bone(rig, name) as u32;
    let layers: Vec<_> = rig
        .render
        .iter()
        .filter(|layer| layer.geometry.is_none())
        .collect();
    assert!(!layers.is_empty(), "horse draws its selected geometry");
    layers
        .iter()
        .any(|layer| !layer.hidden_bones.contains(&index))
}

fn near(actual: f32, expected: f32) {
    assert!(
        (actual - expected).abs() < 1e-4,
        "expected {expected}, got {actual}"
    );
}

fn selected_geometry<'a>(assets: &'a RuntimeEntityAssets, rig: &ActorRigSnapshot<'_>) -> &'a str {
    let geometry = assets.rig_geometries()[rig.rig.0 as usize].geometry as usize;
    &assets.geometries()[geometry].identifier
}

#[test]
fn pinned_horse_rest_pose_preserves_the_adult_neck_and_head_origins() {
    let Some(assets) = fixture() else {
        return;
    };
    let actor = horse(0);
    let mut store = ActorAnimationStore::with_assets(Arc::clone(&assets));
    store.insert(1, 0, &actor);
    for _ in 0..3 {
        tick(&mut store, &actor, false);
        let rig = store.get(actor.runtime_id).expect("adult horse rig");
        assert_eq!(selected_geometry(&assets, &rig), "geometry.horse.v3");
        near(rig.scale, 1.0);
        let neck = bone(&rig, "neck");
        assert_eq!(rig.current[neck], rig.rest[neck]);
        for (actual, expected) in rig.current[neck].translation_scale[..3]
            .iter()
            .zip([0.0, 17.0, -8.0])
        {
            near(*actual, expected);
        }
        let (sin, cos) = (-15.0_f32).to_radians().sin_cos();
        for (actual, expected) in rig.current[neck].rotation.iter().zip([sin, 0.0, 0.0, cos]) {
            near(*actual, expected);
        }
        let head = &rig.current[bone(&rig, "head")];
        let (sin, cos) = (-30.0_f32).to_radians().sin_cos();
        near(head.translation_scale[0], 0.0);
        near(head.translation_scale[1], 17.0 + 11.0 * cos + 3.0 * sin);
        near(head.translation_scale[2], -8.0 + 11.0 * sin - 3.0 * cos);
    }
}

fn query_flag(name: &str) -> u64 {
    let input = ActorTickInput::default();
    let context = ActorTickContext::default();
    let flags: Vec<_> = (0..u64::BITS)
        .filter_map(|bit| {
            let flag = 1_u64 << bit;
            let actor = horse(flag);
            let inputs = query::QueryInputs {
                actor: &actor,
                input: &input,
                context: &context,
                anim_tick: 0,
                anim_time: None,
                life_tick: 0,
                finished: (false, false),
                bones: &[],
                bone_names: &[],
            };
            (query::query(&inputs, name, &[]).number() == 1.0).then_some(flag)
        })
        .collect();
    assert_eq!(flags.len(), 1, "{name} reads one actor flag");
    flags[0]
}

#[test]
fn pinned_horse_equipment_visibility_follows_saddle_and_rider_state() {
    let Some(assets) = fixture() else {
        return;
    };
    let mut actor = horse(0);
    let mut store = ActorAnimationStore::with_assets(assets);
    store.insert(1, 0, &actor);
    let saddled_flag = query_flag("is_saddled");
    for (saddled, rider) in [(false, false), (false, true), (true, false), (true, true)] {
        actor.metadata.insert(
            0,
            ActorMetadataValue::Flags(if saddled { saddled_flag } else { 0 }),
        );
        tick(&mut store, &actor, rider);
        let rig = store.get(actor.runtime_id).expect("horse equipment rig");
        for name in ["body", "head", "mane", "earl", "earr"] {
            assert!(visible(&rig, name), "horse keeps {name} visible");
        }
        for name in ["bagl", "bagr", "muleearl", "muleearr"] {
            assert!(!visible(&rig, name), "horse never draws {name}");
        }
        for name in ["saddle", "bridle", "bitl", "bitr"] {
            assert_eq!(visible(&rig, name), saddled, "{name} follows saddle");
        }
        for name in ["reinsl", "reinsr"] {
            assert_eq!(
                visible(&rig, name),
                saddled && rider,
                "{name} needs a saddle and rider"
            );
        }
    }
}

#[test]
fn pinned_horse_baby_selects_its_own_proportions_and_adult_legs_walk() {
    let Some(assets) = fixture() else {
        return;
    };
    let mut actor = horse(1 << query::FLAG_BABY);
    let mut store = ActorAnimationStore::with_assets(Arc::clone(&assets));
    store.insert(1, 0, &actor);
    tick(&mut store, &actor, false);
    let rig = store.get(actor.runtime_id).expect("baby horse rig");
    assert_eq!(selected_geometry(&assets, &rig), "geometry.horse.baby");
    near(rig.scale, 2.0);
    let body = &rig.current[bone(&rig, "body")];
    near(body.translation_scale[1], 11.5);
    near(body.translation_scale[2], 0.0);

    actor.metadata.insert(0, ActorMetadataValue::Flags(0));
    tick(&mut store, &actor, false);
    let rig = store.get(actor.runtime_id).expect("grown horse rig");
    assert_eq!(selected_geometry(&assets, &rig), "geometry.horse.v3");
    near(rig.scale, 1.0);
    let leg_names = ["legfl", "legfr", "legbl", "legbr"];
    let mut moved = [false; 4];
    for step in 1..=8 {
        actor.position[2] = -(step as f32) * 0.15;
        tick(&mut store, &actor, false);
        let rig = store.get(actor.runtime_id).expect("walking horse rig");
        for (slot, name) in leg_names.iter().enumerate() {
            let index = bone(&rig, name);
            let leg = &rig.current[index];
            moved[slot] |= leg.rotation != rig.rest[index].rotation;
            near(leg.rotation[1], 0.0);
            near(leg.rotation[2], 0.0);
            assert!(
                leg.rotation[0].abs() <= 23.0_f32.to_radians().sin() + 1e-4,
                "walking leg stays within its authored swing"
            );
            assert_eq!(
                leg.translation_scale, rig.rest[index].translation_scale,
                "walking rotates {name} about its authored hip"
            );
        }
        for (left, right) in [("legfl", "legfr"), ("legbl", "legbr")] {
            let left = rig.current[bone(&rig, left)].rotation;
            let right = rig.current[bone(&rig, right)].rotation;
            near(left[0], -right[0]);
            near(left[3], right[3]);
        }
    }
    assert!(moved.into_iter().all(|moved| moved), "all four legs walk");
}

#[test]
fn pinned_horse_rears_and_grazes_from_its_synced_state() {
    let Some(assets) = fixture() else {
        return;
    };
    let mut actor = horse(0);
    let mut store = ActorAnimationStore::with_assets(assets);
    store.insert(1, 0, &actor);
    tick(&mut store, &actor, false);
    actor
        .metadata
        .insert(0, ActorMetadataValue::Flags(query_flag("is_standing")));
    for _ in 0..12 {
        tick(&mut store, &actor, false);
    }
    let rig = store.get(actor.runtime_id).unwrap();
    let body = &rig.current[bone(&rig, "body")];
    let (sin, cos) = 22.5_f32.to_radians().sin_cos();
    near(body.rotation[0], sin);
    near(body.rotation[3], cos);
    actor.metadata.insert(0, ActorMetadataValue::Flags(0));
    for _ in 0..30 {
        tick(&mut store, &actor, false);
    }
    let rig = store.get(actor.runtime_id).unwrap();
    assert_eq!(
        rig.current[bone(&rig, "body")],
        rig.rest[bone(&rig, "body")]
    );
    actor.metadata.insert(
        super::super::horse::KEY_FLAGS,
        ActorMetadataValue::Long(1 << 5),
    );
    for _ in 0..12 {
        tick(&mut store, &actor, false);
    }
    let rig = store.get(actor.runtime_id).unwrap();
    assert_ne!(
        rig.current[bone(&rig, "neck")].rotation,
        rig.rest[bone(&rig, "neck")].rotation,
        "grazing bends the neck from horse-specific flags"
    );
}
