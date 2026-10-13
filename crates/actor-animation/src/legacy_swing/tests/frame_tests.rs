use super::*;
use assets::{
    CompiledMolangExpression, EntityAssetKind, EntityAssetSymbol, MolangOp, MolangSymbol,
    MolangSymbolKind,
};

/// Supplies an attack clip alongside a tick-owned arm rotation.
fn assets() -> Arc<RuntimeEntityAssets> {
    let mut compiled = crate::test_support::counting_random_compiled("minecraft:player");
    compiled.rig_bindings[0].initialize = None;
    compiled.rig_bindings[0].pre_animation = None;
    compiled.render.layers[0].color = None;
    let (bones, names) = skeleton();
    let template = compiled.geometries[0].bones[0].clone();
    compiled.geometries[0].bones = bones
        .iter()
        .zip(&names)
        .map(|(bone, name)| assets::EntityGeometryBone {
            name: name.clone(),
            parent: bone.parent.map(|index| names[index].clone()),
            pivot: Some(bone.pivot.map(crate::test_support::scalar)),
            ..template.clone()
        })
        .collect();
    let mut symbols = compiled.symbols.into_vec();
    symbols.insert(
        3,
        EntityAssetSymbol {
            kind: EntityAssetKind::Animation,
            identifier: "animation.player.attack.rotations".into(),
            ..symbols[2].clone()
        },
    );
    compiled.symbols = symbols.into();
    compiled.rig_bindings[0].render_controller += 1;
    compiled.molang_symbols = vec![
        MolangSymbol {
            kind: MolangSymbolKind::Name,
            identifier: "wield".into(),
        },
        MolangSymbol {
            kind: MolangSymbolKind::Query,
            identifier: "query.life_time".into(),
        },
        MolangSymbol {
            kind: MolangSymbolKind::Variable,
            identifier: "variable.attack_time".into(),
        },
    ]
    .into();
    compiled.molang_ops = vec![MolangOp::LoadQuery(1), MolangOp::LoadVariable(2)].into();
    compiled.molang_expressions = [0, 1]
        .map(|first_op| CompiledMolangExpression {
            first_op,
            op_count: 1,
            max_stack: 1,
        })
        .into();
    compiled.animation_channels[0].bone_name = Some("rightarm".into());
    compiled.animation_channels[0].property = EntityAnimationProperty::Rotation;
    compiled.animation_keyframes[0].expressions = [Some(0), None, None];
    let mut channels = compiled.animation_channels.into_vec();
    let mut attack = channels[0].clone();
    attack.first_keyframe = 1;
    channels.push(attack);
    compiled.animation_channels = channels.into();
    let mut keys = compiled.animation_keyframes.into_vec();
    let mut attack = keys[0];
    attack.expressions = [Some(1), None, None];
    keys.push(attack);
    compiled.animation_keyframes = keys.into();
    let mut clips = compiled.animation_clips.into_vec();
    let mut attack = clips[0];
    attack.symbol = 3;
    attack.first_channel = 1;
    clips.push(attack);
    compiled.animation_clips = clips.into();
    let mut bindings = compiled.rig_animations.into_vec();
    let mut attack = bindings[0];
    attack.clip = 1;
    attack.order = 1;
    bindings.push(attack);
    compiled.rig_animations = bindings.into();
    compiled.rig_geometries[0].animation_count += 1;
    Arc::new(RuntimeEntityAssets::from_compiled(compiled.with_unresolved_admissions()).unwrap())
}

#[test]
fn third_person_swing_keeps_smooth_tick_motion_at_every_physics_fraction() {
    for combat_visuals in [CombatVisuals::Legacy112, CombatVisuals::Modern] {
        let actor = player();
        let actors = HashMap::from([(actor.runtime_id, actor.clone())]);
        let mut store = ActorAnimationStore::with_assets(assets());
        store.insert(1, 0, &actor);
        let context = ActorTickContext {
            is_local: true,
            is_local_player: true,
            combat_visuals,
            ..Default::default()
        };
        for _ in 0..3 {
            store.advance_tick(&actors, None, Some(1), true, true, |_| context.clone());
        }
        let rig = store.get(1).unwrap();
        assert_ne!(rig.previous[2].rotation, rig.current[2].rotation);
        let completed_tick = rig.completed_tick;
        let ordinary = [rig.previous[2].rotation, rig.current[2].rotation];
        for progress in [[0.25, 0.5], [0.75, 0.0]] {
            store.sync_local_swing(
                1,
                LocalSwingProgress {
                    bedrock: progress,
                    java: progress,
                    frame_alpha: Some(0.0),
                },
            );
            store.prepare_frame(&actors, None, Some(1), true, |_| context.clone());
            let rig = store.get(1).unwrap();
            let committed = (rig.previous.to_vec(), rig.current.to_vec());
            for physics_alpha in [0.0, 0.1, 0.5, 0.9, 1.0, 0.1] {
                let progress = LocalSwingProgress {
                    bedrock: progress,
                    java: progress,
                    frame_alpha: Some(physics_alpha),
                };
                store.sync_local_swing(1, progress);
                let mut budget = MAX_MOLANG_OPS_PER_RENDER_FRAME;
                let sampled = store
                    .render_layers(&actor, 0.75, [0.0; 2], [0.0; 3], &mut budget, false)
                    .unwrap();
                let yaw = match combat_visuals {
                    CombatVisuals::Legacy112 => {
                        render_model::legacy_combat::legacy_swing_pose(
                            progress.bedrock_progress(0.75),
                            0.0,
                        )
                        .body_yaw
                    }
                    CombatVisuals::Modern => 0.0,
                };
                let expected = pose::quat_from_euler([0.0, -yaw.to_degrees(), 0.0]);
                for bones in [&sampled.render[0].previous_pose, &sampled.render[0].pose] {
                    for (actual, expected) in bones[0].rotation.into_iter().zip(expected) {
                        assert!(
                            (actual - expected).abs() < 1.0e-6,
                            "body rotation {combat_visuals:?}, {progress:?}: {actual} vs {expected}"
                        );
                    }
                }
                if combat_visuals == CombatVisuals::Modern {
                    let attack =
                        pose::quat_from_euler([-progress.bedrock_progress(0.75), 0.0, 0.0]);
                    for (bones, ordinary) in
                        [&sampled.render[0].previous_pose, &sampled.render[0].pose]
                            .into_iter()
                            .zip(ordinary)
                    {
                        let expected = pose::quat_multiply(ordinary, attack);
                        for (actual, expected) in bones[2].rotation.into_iter().zip(expected) {
                            assert!(
                                (actual - expected).abs() < 1.0e-6,
                                "arm rotation {combat_visuals:?}, {progress:?}: {actual} vs {expected}"
                            );
                        }
                    }
                }
                assert_ne!(
                    sampled.render[0].previous_pose[2].rotation, sampled.render[0].pose[2].rotation,
                    "ordinary arm motion must retain both tick endpoints during the frame swing"
                );
                let unchanged = store.get(1).unwrap();
                assert_eq!(unchanged.completed_tick, completed_tick);
                assert_eq!(unchanged.previous, committed.0);
                assert_eq!(unchanged.current, committed.1);
            }
        }
    }
}
