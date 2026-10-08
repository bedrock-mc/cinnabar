use super::*;
use crate::actor_store::ActorStore;

fn dragon_store(assets: Option<Arc<RuntimeEntityAssets>>) -> ActorStore {
    let mut store = assets.map_or_else(
        || ActorStore::new(1, 0),
        |assets| ActorStore::new_with_entity_assets(1, 0, assets),
    );
    store.apply(
        1,
        1,
        protocol::ActorEvent::Spawn(protocol::ActorSpawnEvent {
            dimension: 0,
            unique_id: 1,
            runtime_id: 1,
            kind: ActorKind::Entity {
                identifier: "minecraft:ender_dragon".into(),
            },
            position: [0.0, 64.0, 0.0],
            velocity: [0.0; 3],
            pitch: 0.0,
            yaw: 181.0,
            head_yaw: 181.0,
            body_yaw: 181.0,
            held_item: Default::default(),
            metadata: Arc::from([]),
            attributes: Arc::from([]),
            properties: Arc::from([]),
            links: Arc::from([]),
        }),
    );
    store
}

#[test]
fn dragon_query_and_engine_history_read_the_completed_actor_tick() {
    let mut store = dragon_store(None);
    store.advance_interpolation_ticks(1);
    let actor = store.get(1).unwrap();
    let input = ActorTickInput::default();
    let context = ActorTickContext::default();
    let inputs = query::QueryInputs {
        actor,
        input: &input,
        context: &context,
        anim_tick: 1,
        anim_time: None,
        life_tick: 1,
        finished: (false, false),
        bones: &[],
        bone_names: &[],
    };
    assert_eq!(
        query::query(&inputs, "query.wing_flap_position", &[]).number(),
        0.2
    );
    let engine = EngineSlots {
        dragon_history: vec![(0, 0, 0), (1, 0, 1), (2, 23, 1)],
        ..Default::default()
    };
    let mut variables = MolangVariables::slots(3);
    tick::apply_engine_variables(
        &engine,
        &mut variables,
        actor,
        &context,
        &input,
        &MotionState::default(),
    );
    assert_eq!(variables.number_at(0), Some(-179.0));
    assert_eq!(variables.number_at(1), Some(64.0));
    assert_eq!(variables.number_at(2), Some(64.0));
}

#[test]
fn installed_dragon_poses_its_wings_and_neck_within_the_actor_budget() {
    let Some(assets) = installed_assets() else {
        return;
    };
    let mut store = dragon_store(Some(assets));
    let mut previous_wing = None;
    for _ in 0..3 {
        store.advance_interpolation_ticks(1);
        let rig = store.actor_rig(1).expect("dragon rig");
        let wing = rig
            .bone_names
            .iter()
            .position(|name| name.as_ref() == "wing")
            .unwrap();
        let neck = rig
            .bone_names
            .iter()
            .position(|name| name.as_ref() == "neck1")
            .unwrap();
        assert_ne!(rig.current[wing], rig.rest[wing]);
        assert_ne!(rig.current[neck], rig.rest[neck]);
        if let Some(previous) = previous_wing {
            assert_ne!(
                rig.current[wing], previous,
                "flap phase advances the authored wing"
            );
        }
        previous_wing = Some(rig.current[wing]);
    }
    assert_eq!(store.animation_stats().actor_budget_exhaustions, 0);
    assert_eq!(store.animation_stats().world_budget_exhaustions, 0);
    assert_eq!(store.animation_stats().frozen_actors, 0);
}

#[test]
fn installed_dragon_death_controllers_keep_the_unclamped_dissolve_multiplier() {
    let Some(assets) = installed_assets() else {
        return;
    };
    let mut store = dragon_store(Some(assets));
    store.apply(
        1,
        2,
        protocol::ActorEvent::Status(protocol::ActorStatusEvent {
            runtime_id: 1,
            kind: protocol::ActorStatusKind::Death,
            data: 0,
        }),
    );
    store.advance_interpolation_ticks(99);
    let rig = store.actor_rig(1).expect("dying dragon rig");
    assert_eq!(rig.render.len(), 2);
    assert_eq!(
        rig.render[0].material,
        assets::EntityRenderMaterial::DissolveDepth
    );
    assert_eq!(
        rig.render[1].material,
        assets::EntityRenderMaterial::DissolveColor
    );
    assert!(
        (rig.render[0].overlay[3] - 1.3).abs() < 0.001,
        "the depth mask retains the authored multiplier above one"
    );
    assert_eq!(rig.render[1].overlay, [0.0; 4]);
}

#[test]
fn installed_dragon_death_material_samples_frame_alpha_without_advancing_tick_state() {
    let Some(assets) = installed_assets() else {
        return;
    };
    let mut store = dragon_store(Some(assets));
    store.apply(
        1,
        2,
        protocol::ActorEvent::Status(protocol::ActorStatusEvent {
            runtime_id: 1,
            kind: protocol::ActorStatusKind::Death,
            data: 0,
        }),
    );
    store.advance_interpolation_ticks(99);
    let before_actor = store.get(1).unwrap().clone();
    let before_rig = store.actor_rig(1).unwrap();
    let before_pose = before_rig.current.to_vec();
    let before_tick = before_rig.completed_tick;
    let tick_layers = before_rig.render.to_vec();
    let frame = store.render_frame(0.75).layers(1).unwrap();
    assert!(
        (frame[0].overlay[3] - 1.2925).abs() < 0.0001,
        "the authored dissolve formula must sample query.frame_alpha"
    );
    assert_eq!(store.get(1).unwrap(), &before_actor);
    assert_eq!(store.actor_rig(1).unwrap().completed_tick, before_tick);
    assert_eq!(store.actor_rig(1).unwrap().current, before_pose);
    assert_eq!(store.actor_rig(1).unwrap().render, tick_layers);
    assert_eq!(
        store.render_frame(0.0).layers(1).unwrap().as_ref(),
        tick_layers
    );
}

fn installed_assets() -> Option<Arc<RuntimeEntityAssets>> {
    let root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.local/assets/compiled");
    let entries = match std::fs::read_dir(&root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            eprintln!(
                "skipping dragon animation fixture: {} is absent",
                root.display()
            );
            return None;
        }
        Err(error) => panic!("read {}: {error}", root.display()),
    };
    let paths = entries
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "mcbeent")
        })
        .collect::<Vec<_>>();
    if paths.is_empty() {
        eprintln!("skipping dragon animation fixture: no entity carrier");
        return None;
    }
    assert_eq!(
        paths.len(),
        1,
        "ambiguous installed entity carrier fixture: {paths:?}"
    );
    Some(Arc::new(
        RuntimeEntityAssets::decode(&std::fs::read(&paths[0]).unwrap()).unwrap(),
    ))
}

#[test]
fn installed_dragon_wing_tips_follow_the_rotating_inner_spar_endpoints() {
    let Some(assets) = installed_assets() else {
        return;
    };
    let geometry = assets
        .geometries()
        .iter()
        .find(|geometry| geometry.identifier.as_ref() == "geometry.dragon")
        .unwrap();
    let spans = ["wing", "wing1"].map(|name| {
        geometry
            .bones
            .iter()
            .find(|bone| bone.name.as_ref() == name)
            .unwrap()
            .cubes[0]
            .size[0]
            .get()
    });
    let mut store = dragon_store(Some(assets));
    for _ in 0..48 {
        store.advance_interpolation_ticks(1);
        let rig = store.actor_rig(1).expect("dragon rig");
        for ((wing_name, tip_name), span) in [("wing", "wingtip"), ("wing1", "wingtip1")]
            .into_iter()
            .zip(spans)
        {
            let index = |name| {
                rig.bone_names
                    .iter()
                    .position(|candidate| candidate.as_ref() == name)
                    .unwrap()
            };
            for pose in [rig.previous, rig.current] {
                let wing = pose[index(wing_name)];
                let tip = pose[index(tip_name)];
                let [x, y, z, w] = wing.rotation;
                let direction = [
                    1.0 - 2.0 * (y * y + z * z),
                    2.0 * (x * y + w * z),
                    2.0 * (x * z - w * y),
                ];
                for (axis, direction) in direction.into_iter().enumerate() {
                    let endpoint = wing.translation_scale[axis] + span * direction;
                    assert!(
                        (tip.translation_scale[axis] - endpoint).abs() < 1e-3,
                        "{tip_name} separated from {wing_name} at tick {}: {:?} != {endpoint}",
                        rig.completed_tick,
                        tip.translation_scale,
                    );
                }
            }
        }
    }
}
