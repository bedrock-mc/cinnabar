use super::*;
use assets::{
    CompiledMolangExpression, EntityAssetKind, EntityAssetSymbol, MolangOp, MolangSymbol,
    MolangSymbolKind,
};

/// Supplies an attack clip alongside a tick-owned arm rotation.
fn assets() -> Arc<RuntimeEntityAssets> {
    assets_for("minecraft:player")
}

/// The attack fixture under another entity identifier.
fn assets_for(identifier: &str) -> Arc<RuntimeEntityAssets> {
    let mut compiled = crate::test_support::counting_random_compiled(identifier);
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
    *compiled.rig_bindings[0].render_controller.as_mut().unwrap() += 1;
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

#[test]
fn local_player_swing_samples_native_item_rotation_alias() {
    let mut compiled = crate::test_support::counting_random_compiled("minecraft:player");
    compiled.rig_bindings[0].initialize = None;
    compiled.rig_bindings[0].pre_animation = Some(0);
    compiled.render.layers[0].color = None;
    compiled.molang_symbols = [
        (MolangSymbolKind::Name, "wield"),
        (MolangSymbolKind::Variable, "variable.attack_time"),
        (
            MolangSymbolKind::Variable,
            "variable.first_person_item_rotation_factor",
        ),
        (
            MolangSymbolKind::Variable,
            "variable.first_person_rotation_factor",
        ),
    ]
    .map(|(kind, identifier)| MolangSymbol {
        kind,
        identifier: identifier.into(),
    })
    .into();
    compiled.molang_ops = vec![
        MolangOp::LoadVariable(1),
        MolangOp::Push(crate::test_support::scalar(90.0)),
        MolangOp::Multiply,
        MolangOp::StoreVariable(3),
        MolangOp::Push(crate::test_support::scalar(0.0)),
        MolangOp::LoadVariable(2),
    ]
    .into();
    compiled.molang_expressions = [(0, 5, 2), (5, 1, 1)]
        .map(|(first_op, op_count, max_stack)| CompiledMolangExpression {
            first_op,
            op_count,
            max_stack,
        })
        .into();
    compiled.animation_channels[0].property = EntityAnimationProperty::Rotation;
    compiled.animation_keyframes[0].expressions = [Some(1), None, None];
    let assets = Arc::new(
        RuntimeEntityAssets::from_compiled(compiled.with_unresolved_admissions()).unwrap(),
    );
    let actor = player();
    let actors = HashMap::from([(actor.runtime_id, actor.clone())]);
    let mut store = ActorAnimationStore::with_assets(assets);
    store.insert(1, 0, &actor);
    let context = ActorTickContext {
        is_local: true,
        is_local_player: true,
        combat_visuals: CombatVisuals::Modern,
        ..Default::default()
    };
    for _ in 0..3 {
        store.advance_tick(&actors, None, Some(1), true, true, |_| context.clone());
    }
    let rig = store.get(1).unwrap();
    let committed = (rig.previous.to_vec(), rig.current.to_vec());
    for physics_alpha in [0.0, 0.25, 0.75, 1.0, 0.25] {
        let progress = LocalSwingProgress {
            bedrock: [0.2, 0.6],
            java: [0.2, 0.6],
            frame_alpha: Some(physics_alpha),
        };
        store.sync_local_swing(1, progress);
        let mut budget = MAX_MOLANG_OPS_PER_RENDER_FRAME;
        let sampled = store
            .render_layers(&actor, 0.5, [0.0; 2], [0.0; 3], &mut budget, false)
            .unwrap();
        let expected = pose::quat_from_euler([-90.0 * progress.bedrock_progress(0.5), 0.0, 0.0]);
        for bones in [&sampled.render[0].previous_pose, &sampled.render[0].pose] {
            for (actual, expected) in bones[0].rotation.into_iter().zip(expected) {
                assert!(
                    (actual - expected).abs() < 1.0e-6,
                    "item rotation at {physics_alpha}: {actual} vs {expected}"
                );
            }
        }
    }
    let unchanged = store.get(1).unwrap();
    assert_eq!(unchanged.previous, committed.0);
    assert_eq!(unchanged.current, committed.1);
}

/// A remote attack samples its swing curve at the frame fraction on both drawn endpoints.
#[test]
fn remote_swing_samples_the_attack_at_every_frame_fraction() {
    for combat_visuals in [CombatVisuals::Legacy112, CombatVisuals::Modern] {
        let actor = player();
        let actors = HashMap::from([(actor.runtime_id, actor.clone())]);
        let mut store = ActorAnimationStore::with_assets(assets());
        store.insert(1, 0, &actor);
        let context = ActorTickContext {
            combat_visuals,
            ..Default::default()
        };
        for _ in 0..3 {
            store.advance_tick(&actors, None, None, true, true, |_| context.clone());
        }
        store.start_swing(1, 6, 6);
        for _ in 0..3 {
            store.advance_tick(&actors, None, None, true, true, |_| context.clone());
        }
        let rig = store.get(1).unwrap();
        let attack = rig.hand.map(|hand| hand.attack_time);
        // Each committed arm composes its own tick's attack; remove it to recover the motion.
        let ordinary = [
            (rig.previous[2].rotation, attack[0]),
            (rig.current[2].rotation, attack[1]),
        ]
        .map(|(rotation, attack)| {
            let [x, y, z, w] = pose::quat_from_euler([-attack, 0.0, 0.0]);
            pose::quat_multiply(rotation, [-x, -y, -z, w])
        });
        assert!(attack[1] > attack[0], "the swing is mid-curve: {attack:?}");
        for alpha in [0.0, 0.25, 0.5, 0.75] {
            let progress = LocalSwingProgress::interpolate(attack, alpha);
            let mut budget = MAX_MOLANG_OPS_PER_RENDER_FRAME;
            let sampled = store
                .render_layers(&actor, alpha, [0.0; 2], [0.0; 3], &mut budget, false)
                .unwrap();
            // Presentation draws a frame-sampled body, otherwise the committed tick pair.
            let rig = store.get(1).unwrap();
            let drawn = match &sampled.render {
                std::borrow::Cow::Owned(layers) if !layers[0].pose.is_empty() => {
                    [&*layers[0].previous_pose, &*layers[0].pose]
                }
                _ => [rig.previous, rig.current],
            };
            let yaw = match combat_visuals {
                CombatVisuals::Legacy112 => {
                    render_model::legacy_combat::legacy_swing_pose(progress, 0.0).body_yaw
                }
                CombatVisuals::Modern => 0.0,
            };
            let expected = pose::quat_from_euler([0.0, -yaw.to_degrees(), 0.0]);
            for bones in drawn {
                for (actual, expected) in bones[0].rotation.into_iter().zip(expected) {
                    assert!(
                        (actual - expected).abs() < 1.0e-5,
                        "body {combat_visuals:?} at {alpha}: {actual} vs {expected}"
                    );
                }
            }
            if combat_visuals == CombatVisuals::Modern {
                let attack = pose::quat_from_euler([-progress, 0.0, 0.0]);
                for (bones, ordinary) in drawn.into_iter().zip(ordinary) {
                    let expected = pose::quat_multiply(ordinary, attack);
                    for (actual, expected) in bones[2].rotation.into_iter().zip(expected) {
                        assert!(
                            (actual - expected).abs() < 1.0e-5,
                            "arm at {alpha}: {actual} vs {expected}"
                        );
                    }
                }
            }
        }
    }
}

/// An attacking mob keeps its ordinary tick motion on both drawn endpoints.
#[test]
fn attacking_mobs_keep_ordinary_motion_between_ticks() {
    let identifier = "minecraft:zombie";
    let actor = TestActor {
        kind: ActorKind::Entity {
            identifier: identifier.into(),
        },
        ..Default::default()
    };
    let actors = HashMap::from([(actor.runtime_id, actor.clone())]);
    let mut store = ActorAnimationStore::with_assets(assets_for(identifier));
    store.insert(1, 0, &actor);
    for _ in 0..3 {
        store.advance_tick(&actors, None, None, true, true, |_| {
            ActorTickContext::default()
        });
    }
    store.start_swing(1, 6, 6);
    for _ in 0..3 {
        store.advance_tick(&actors, None, None, true, true, |_| {
            ActorTickContext::default()
        });
    }
    let rig = store.get(1).unwrap();
    assert!(rig.hand[1].attack_time > rig.hand[0].attack_time);
    for alpha in [0.25, 0.5, 0.75] {
        let mut budget = MAX_MOLANG_OPS_PER_RENDER_FRAME;
        let sampled = store
            .render_layers(&actor, alpha, [0.0; 2], [0.0; 3], &mut budget, false)
            .unwrap();
        let rig = store.get(1).unwrap();
        let [previous, current] = match &sampled.render {
            std::borrow::Cow::Owned(layers) if !layers[0].pose.is_empty() => {
                [&*layers[0].previous_pose, &*layers[0].pose]
            }
            _ => [rig.previous, rig.current],
        };
        assert_ne!(
            previous[2].rotation, current[2].rotation,
            "the arm must keep both tick endpoints at {alpha}"
        );
    }
}
