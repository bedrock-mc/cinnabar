use super::*;
use assets::{EntityAnimationProperty, EntityAssetKind, MolangOp, MolangSymbolKind};

/// Models a player pack that drives its own arm through the standard attack clip names.
fn attack_assets(property: EntityAnimationProperty) -> Arc<RuntimeEntityAssets> {
    let mut compiled = crate::test_support::compiled_fixture();
    compiled.sources[1].path = "entity/player.entity.json".into();
    compiled.symbols[4].kind = EntityAssetKind::Entity;
    compiled.symbols[4].identifier = "minecraft:player".into();
    compiled.symbols[1].identifier = match property {
        EntityAnimationProperty::Rotation => "animation.player.attack.rotations",
        EntityAnimationProperty::Translation => "animation.player.attack.positions",
        EntityAnimationProperty::Scale => unreachable!(),
    }
    .into();
    compiled.symbols.rotate_right(1);
    compiled.rig_bindings[0].entity_symbol = 0;
    compiled.rig_bindings[0].render_controller = Some(3);
    compiled.rig_bindings[0].pre_animation = None;
    compiled.animation_clips[0].symbol = 2;
    compiled.geometries[0].bones[0].name = "strike_arm".into();
    compiled.animation_channels[0].bone_name = Some("strike_arm".into());
    compiled.animation_channels[0].property = property;
    compiled.animation_keyframes[0].value = [crate::test_support::scalar(0.0); 3];
    compiled.animation_keyframes[0].expressions = [Some(0), None, None];
    compiled.molang_symbols = [
        (MolangSymbolKind::Name, "attack"),
        (MolangSymbolKind::Query, "query.camera_rotation"),
        (MolangSymbolKind::Variable, "variable.attack_time"),
    ]
    .into_iter()
    .map(|(kind, identifier)| assets::MolangSymbol {
        kind,
        identifier: identifier.into(),
    })
    .collect();
    compiled.molang_ops = vec![
        MolangOp::LoadVariable(2),
        MolangOp::Push(crate::test_support::scalar(90.0)),
        MolangOp::Multiply,
        MolangOp::Push(crate::test_support::scalar(0.0)),
        MolangOp::CallQuery(assets::MolangCall {
            symbol: 1,
            arguments: 1,
        }),
        MolangOp::Add,
    ]
    .into_boxed_slice();
    compiled.molang_expressions = vec![assets::CompiledMolangExpression {
        first_op: 0,
        op_count: compiled.molang_ops.len() as u16,
        max_stack: 2,
    }]
    .into_boxed_slice();
    Arc::new(RuntimeEntityAssets::from_compiled(compiled.with_unresolved_admissions()).unwrap())
}

/// Admits the fixture as a session-pack rig and advances one remote swing with the default profile.
fn remote_swing(property: EntityAnimationProperty) -> (ActorAnimationStore, TestActor) {
    let assets = attack_assets(property);
    let actor = TestActor {
        kind: ActorKind::Player {
            uuid: [0; 16],
            username: "test".into(),
        },
        ..Default::default()
    };
    let mut store = ActorAnimationStore::with_assets(Arc::clone(&assets));
    store.set_pack(Some((assets, vec![0])));
    store.insert(1, 0, &actor);
    assert!(store.get(actor.runtime_id).unwrap().rig.0 >= assets::PACK_RIG_ID_BASE);
    store.start_swing(actor.runtime_id, ACTOR_SWING_TICKS, ACTOR_SWING_TICKS);
    let actors = HashMap::from([(actor.runtime_id, actor.clone())]);
    for _ in 0..4 {
        store.advance_tick(&actors, None, None, true, false, |_| {
            ActorTickContext::default()
        });
    }
    (store, actor)
}

#[test]
fn remote_pack_attacks_pose_the_authored_arm_in_the_default_combat_profile() {
    for property in [
        EntityAnimationProperty::Rotation,
        EntityAnimationProperty::Translation,
    ] {
        let (store, actor) = remote_swing(property);
        let rig = store.get(actor.runtime_id).unwrap();
        assert!(rig.pose_evaluated);
        assert_eq!(rig.item_animation[1].attack_time, 0.5);
        match property {
            EntityAnimationProperty::Rotation => {
                assert_eq!(
                    rig.current[0].rotation,
                    pose::quat_from_euler([-45.0, 0.0, 0.0])
                );
            }
            EntityAnimationProperty::Translation => {
                assert_eq!(rig.current[0].translation_scale[0], -45.0);
            }
            EntityAnimationProperty::Scale => unreachable!(),
        }
        assert_ne!(rig.current[0], rig.rest[0]);
        let actors = HashMap::from([(actor.runtime_id, actor.clone())]);
        let mut store = store;
        for _ in 0..4 {
            store.advance_tick(&actors, None, None, true, false, |_| {
                ActorTickContext::default()
            });
        }
        let rig = store.get(actor.runtime_id).unwrap();
        assert_eq!(rig.item_animation[1].attack_time, 0.0);
        assert_eq!(rig.current[0], rig.rest[0]);
    }
}

#[test]
fn frame_sampling_keeps_pack_attack_channels_on_scratch_poses() {
    let (store, actor) = remote_swing(EntityAnimationProperty::Rotation);
    let committed = store.get(actor.runtime_id).unwrap().current.to_vec();
    let mut remaining_ops = MAX_MOLANG_OPS_PER_WORLD_TICK;
    let layers = store
        .render_layers(
            &actor,
            0.5,
            [10.0, 20.0],
            [0.0; 3],
            &mut remaining_ops,
            false,
        )
        .unwrap();
    assert_eq!(layers.render.len(), 1);
    // The attack samples halfway between its 2/6 and 3/6 ticks; the camera adds its own -10.
    let expected = pose::quat_from_euler([-47.5, 0.0, 0.0]);
    for (actual, expected) in layers.render[0].pose[0].rotation.into_iter().zip(expected) {
        assert!((actual - expected).abs() < 1.0e-5, "{actual} vs {expected}");
    }
    assert_ne!(layers.render[0].pose[0].rotation, committed[0].rotation);
    assert_eq!(
        store.get(actor.runtime_id).unwrap().current,
        committed.as_slice()
    );
}
