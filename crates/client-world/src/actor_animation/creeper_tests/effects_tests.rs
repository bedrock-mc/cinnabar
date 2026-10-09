use super::*;

#[test]
fn swell_conditional_return_controls_later_assignments_at_the_frame_fraction() {
    let store = pack_swell_fixture_with(
        AuthoredSwellChannel {
            pre_animation: true,
            property: assets::EntityAnimationProperty::Translation,
            variable: true,
        },
        None,
        false,
        3,
        false,
        |compiled| {
            let scalar = |value| assets::EntityGeometryScalar::new(value).unwrap();
            compiled.molang_ops = vec![
                MolangOp::Push(scalar(0.0)),
                MolangOp::StoreVariable(3),
                MolangOp::LoadQuery(2),
                MolangOp::Push(scalar(0.08)),
                MolangOp::LessEqual,
                MolangOp::JumpIfFalse(9),
                MolangOp::Push(scalar(0.0)),
                MolangOp::Return,
                MolangOp::Jump(9),
                MolangOp::Push(scalar(2.0)),
                MolangOp::StoreVariable(3),
                MolangOp::Push(scalar(0.0)),
                MolangOp::LoadVariable(3),
            ]
            .into_boxed_slice();
            compiled.molang_expressions = [(0, 12, 2), (12, 1, 1)]
                .into_iter()
                .map(
                    |(first_op, op_count, max_stack)| assets::CompiledMolangExpression {
                        first_op,
                        op_count,
                        max_stack,
                    },
                )
                .collect::<Vec<_>>()
                .into_boxed_slice();
            compiled.animation_keyframes[0].expressions = [Some(1), None, None];
        },
    );
    let completed = store.actor_rig(1).unwrap();
    let tick = completed.completed_tick;
    for (alpha, expected) in [(0.0, 0.0), (0.1, 0.0), (0.5, -2.0), (1.0, -2.0)] {
        let layers = store.render_frame(alpha).layers(1).unwrap();
        for endpoint in [&layers[0].previous_pose[0], &layers[0].pose[0]] {
            assert_eq!(endpoint.translation_scale[0], expected);
        }
    }
    assert_eq!(store.actor_rig(1).unwrap().completed_tick, tick);
}

#[test]
fn swell_conditional_loop_break_controls_later_assignments_at_the_frame_fraction() {
    let store = pack_swell_fixture_with(
        AuthoredSwellChannel {
            pre_animation: true,
            property: assets::EntityAnimationProperty::Translation,
            variable: true,
        },
        None,
        false,
        3,
        false,
        |compiled| {
            let scalar = |value| assets::EntityGeometryScalar::new(value).unwrap();
            compiled.molang_ops = vec![
                MolangOp::Push(scalar(0.0)),
                MolangOp::StoreVariable(3),
                MolangOp::Push(scalar(1.0)),
                MolangOp::LoopStart(13),
                MolangOp::LoadQuery(2),
                MolangOp::Push(scalar(0.08)),
                MolangOp::LessEqual,
                MolangOp::JumpIfFalse(10),
                MolangOp::LoopBreak(13),
                MolangOp::Jump(10),
                MolangOp::Push(scalar(2.0)),
                MolangOp::StoreVariable(3),
                MolangOp::LoopNext(4),
                MolangOp::Push(scalar(0.0)),
                MolangOp::LoadVariable(3),
            ]
            .into_boxed_slice();
            compiled.molang_expressions = [(0, 14, 2), (14, 1, 1)]
                .into_iter()
                .map(
                    |(first_op, op_count, max_stack)| assets::CompiledMolangExpression {
                        first_op,
                        op_count,
                        max_stack,
                    },
                )
                .collect::<Vec<_>>()
                .into_boxed_slice();
            compiled.animation_keyframes[0].expressions = [Some(1), None, None];
        },
    );
    let completed = store.actor_rig(1).unwrap();
    let tick = completed.completed_tick;
    for (alpha, expected) in [(0.0, 0.0), (0.1, 0.0), (0.5, -2.0), (1.0, -2.0)] {
        let layers = store.render_frame(alpha).layers(1).unwrap();
        for endpoint in [&layers[0].previous_pose[0], &layers[0].pose[0]] {
            assert_eq!(endpoint.translation_scale[0], expected);
        }
    }
    assert_eq!(store.actor_rig(1).unwrap().completed_tick, tick);
}

#[test]
fn swell_keyframe_writes_feed_later_channels_and_render_without_staling_frame_variables() {
    let store = pack_swell_fixture_with(
        AuthoredSwellChannel {
            pre_animation: true,
            property: assets::EntityAnimationProperty::Translation,
            variable: false,
        },
        None,
        false,
        3,
        false,
        |compiled| {
            compiled.molang_symbols = [
                (assets::MolangSymbolKind::Name, "wield"),
                (assets::MolangSymbolKind::Query, "query.frame_alpha"),
                (assets::MolangSymbolKind::Query, "query.swell_amount"),
                (assets::MolangSymbolKind::Variable, "variable.color"),
                (assets::MolangSymbolKind::Variable, "variable.frame"),
            ]
            .into_iter()
            .map(|(kind, identifier)| assets::MolangSymbol {
                kind,
                identifier: identifier.into(),
            })
            .collect::<Vec<_>>()
            .into_boxed_slice();
            compiled.molang_ops = vec![
                MolangOp::LoadQuery(1),
                MolangOp::StoreVariable(4),
                MolangOp::Push(assets::EntityGeometryScalar::new(0.0).unwrap()),
                MolangOp::LoadQuery(2),
                MolangOp::StoreVariable(3),
                MolangOp::LoadVariable(3),
                MolangOp::LoadVariable(3),
                MolangOp::LoadVariable(4),
            ]
            .into_boxed_slice();
            compiled.molang_expressions = [(0, 3), (3, 3), (6, 1), (7, 1)]
                .into_iter()
                .map(|(first_op, op_count)| assets::CompiledMolangExpression {
                    first_op,
                    op_count,
                    max_stack: 1,
                })
                .collect::<Vec<_>>()
                .into_boxed_slice();
            compiled.animation_keyframes[0].expressions = [Some(1), None, None];
            let mut scale = compiled.animation_channels[0].clone();
            scale.property = assets::EntityAnimationProperty::Scale;
            scale.first_keyframe = 1;
            let mut key = compiled.animation_keyframes[0];
            key.value = [assets::EntityGeometryScalar::new(1.0).unwrap(); 3];
            key.expressions = [Some(2), None, None];
            compiled.animation_keyframes =
                vec![compiled.animation_keyframes[0], key].into_boxed_slice();
            compiled.animation_channels =
                vec![compiled.animation_channels[0].clone(), scale].into_boxed_slice();
            compiled.animation_clips[0].channel_count = 2;
            compiled.render.layers[0].color = Some([2, 3, 2, 3]);
        },
    );
    let tick = store.actor_rig(1).unwrap().completed_tick;
    for alpha in [0.0, 0.25, 0.5, 0.75, 1.0] {
        let amount = (2.0 + alpha) / crate::actor_store::creeper::SWELL_FULL_TICKS;
        let layers = store.render_frame(alpha).layers(1).unwrap();
        assert!(
            (layers[0].color[0] - amount).abs() < 1e-6,
            "keyframe writes reach render colors: {:?}",
            layers[0].color
        );
        assert_eq!(
            layers[0].color[1], alpha,
            "ordinary pre-animation uses the current frame"
        );
        for endpoint in [&layers[0].previous_pose[0], &layers[0].pose[0]] {
            assert!(
                (endpoint.axis_scale[0] * endpoint.translation_scale[3] - amount).abs() < 1e-6,
                "later channels read the sampled assignment"
            );
        }
    }
    assert_eq!(store.actor_rig(1).unwrap().completed_tick, tick);
}

#[test]
fn swell_endpoint_retains_controller_entry_and_exit_assignments_after_pre_animation() {
    for weighted in [false, true] {
        let mut store = swell_controller_fixture_with(3, 0.12, false, |compiled| {
            let scalar = |value| assets::EntityGeometryScalar::new(value).unwrap();
            let mut symbols = compiled.molang_symbols.to_vec();
            symbols.push(assets::MolangSymbol {
                kind: assets::MolangSymbolKind::Variable,
                identifier: "variable.factor".into(),
            });
            compiled.molang_symbols = symbols.into_boxed_slice();
            let mut ops = compiled.molang_ops.to_vec();
            ops.extend([
                MolangOp::Push(scalar(1.0)),
                MolangOp::StoreVariable(5),
                MolangOp::Push(scalar(0.0)),
                MolangOp::Push(scalar(2.0)),
                MolangOp::StoreVariable(5),
                MolangOp::Push(scalar(0.0)),
                MolangOp::Push(scalar(3.0)),
                MolangOp::StoreVariable(5),
                MolangOp::Push(scalar(0.0)),
                MolangOp::LoadQuery(4),
                MolangOp::LoadVariable(5),
                MolangOp::Multiply,
            ]);
            compiled.molang_ops = ops.into_boxed_slice();
            let mut expressions = compiled.molang_expressions.to_vec();
            expressions.extend([(7, 3, 1), (10, 3, 1), (13, 3, 1), (16, 3, 2)].map(
                |(first_op, op_count, max_stack)| assets::CompiledMolangExpression {
                    first_op,
                    op_count,
                    max_stack,
                },
            ));
            compiled.molang_expressions = expressions.into_boxed_slice();
            compiled.rig_bindings[0].pre_animation = Some(3);
            compiled.controller_states[0].on_exit = Some(4);
            compiled.controller_states[1].on_entry = Some(5);
            compiled.animation_keyframes[0].expressions = [Some(6), None, None];
            for animation in &mut compiled.controller_animations {
                animation.weight = weighted.then_some(0);
            }
        });
        for (before, after) in [(1.0, 3.0), (3.0, 1.0)] {
            let tick = store.actor_rig(1).unwrap().completed_tick;
            let amount = (tick as f32 - 1.0 + 0.5) / crate::actor_store::creeper::SWELL_FULL_TICKS;
            let factor = if weighted { amount * amount } else { amount };
            let layers = store.render_frame(0.5).layers(1).unwrap();
            for (pose, multiplier) in [
                (&layers[0].previous_pose[0], before),
                (&layers[0].pose[0], after),
            ] {
                assert!(
                    (pose.translation_scale[0] + factor * multiplier).abs() < 1e-6,
                    "entry/exit writes follow pre-animation at their endpoint"
                );
            }
            store.advance_interpolation_ticks(1);
        }
    }
}

#[test]
fn swell_endpoint_retains_ordinary_actor_hurt_query_history() {
    let mut store = pack_swell_fixture_with(
        AuthoredSwellChannel {
            pre_animation: false,
            property: assets::EntityAnimationProperty::Translation,
            variable: false,
        },
        None,
        false,
        0,
        false,
        |compiled| {
            compiled.molang_symbols = [
                (assets::MolangSymbolKind::Name, "wield"),
                (assets::MolangSymbolKind::Query, "query.hurt_time"),
                (assets::MolangSymbolKind::Query, "query.swell_amount"),
            ]
            .into_iter()
            .map(|(kind, identifier)| assets::MolangSymbol {
                kind,
                identifier: identifier.into(),
            })
            .collect::<Vec<_>>()
            .into_boxed_slice();
            compiled.molang_ops = vec![
                MolangOp::LoadQuery(2),
                MolangOp::LoadQuery(1),
                MolangOp::Add,
            ]
            .into_boxed_slice();
            compiled.molang_expressions[0].op_count = 3;
            compiled.molang_expressions[0].max_stack = 2;
        },
    );
    store.apply(
        1,
        2,
        protocol::ActorEvent::Status(protocol::ActorStatusEvent {
            runtime_id: 1,
            kind: protocol::ActorStatusKind::Hurt,
            data: 0,
        }),
    );
    store.advance_interpolation_ticks(2);
    let hurt = f32::from(store.get(1).unwrap().status.hurt_time);
    let layers = store.render_frame(0.5).layers(1).unwrap();
    let amount = 1.5 / crate::actor_store::creeper::SWELL_FULL_TICKS;
    for (pose, value) in [
        (&layers[0].previous_pose[0], hurt + 1.0),
        (&layers[0].pose[0], hurt),
    ] {
        assert!(
            (pose.translation_scale[0] + amount + value).abs() < 1e-6,
            "ordinary hurt query stays at its pose endpoint"
        );
    }
}

#[test]
fn swell_dependencies_flow_through_this_into_later_bones() {
    let store = pack_swell_fixture_with(
        AuthoredSwellChannel {
            pre_animation: false,
            property: assets::EntityAnimationProperty::Translation,
            variable: false,
        },
        None,
        false,
        3,
        false,
        |compiled| {
            let scalar = |value| assets::EntityGeometryScalar::new(value).unwrap();
            let mut symbols = compiled.molang_symbols.to_vec();
            symbols.push(assets::MolangSymbol {
                kind: assets::MolangSymbolKind::Variable,
                identifier: "variable.copy".into(),
            });
            compiled.molang_symbols = symbols.into_boxed_slice();
            let mut second = compiled.geometries[0].bones[0].clone();
            second.name = "second".into();
            compiled.geometries[0].bones =
                vec![compiled.geometries[0].bones[0].clone(), second].into_boxed_slice();
            compiled.molang_ops = vec![
                MolangOp::LoadQuery(1),
                MolangOp::LoadThis,
                MolangOp::StoreVariable(2),
                MolangOp::Push(scalar(0.0)),
                MolangOp::LoadVariable(2),
            ]
            .into_boxed_slice();
            compiled.molang_expressions = [(0, 1), (1, 3), (4, 1)]
                .into_iter()
                .map(|(first_op, op_count)| assets::CompiledMolangExpression {
                    first_op,
                    op_count,
                    max_stack: 1,
                })
                .collect::<Vec<_>>()
                .into_boxed_slice();
            let original = compiled.animation_channels[0].clone();
            compiled.animation_channels = (0..3)
                .map(|index| assets::EntityAnimationChannel {
                    bone: u32::from(index == 2),
                    first_keyframe: index,
                    ..original.clone()
                })
                .collect::<Vec<_>>()
                .into_boxed_slice();
            let key = compiled.animation_keyframes[0];
            compiled.animation_keyframes = (0..3)
                .map(|index| assets::EntityAnimationKeyframe {
                    expressions: [Some(index), None, None],
                    ..key
                })
                .collect::<Vec<_>>()
                .into_boxed_slice();
            compiled.animation_clips[0].channel_count = 3;
        },
    );
    let layers = store.render_frame(0.5).layers(1).unwrap();
    for pose in [&layers[0].previous_pose[1], &layers[0].pose[1]] {
        assert!(
            (pose.translation_scale[0] + 2.5 / crate::actor_store::creeper::SWELL_FULL_TICKS).abs()
                < 1e-6,
            "this-derived assignments reach later bones"
        );
    }
}

#[test]
fn swell_keyframe_random_draw_precedes_the_render_random_draw() {
    let store = pack_swell_fixture_with(
        AuthoredSwellChannel {
            pre_animation: false,
            property: assets::EntityAnimationProperty::Translation,
            variable: false,
        },
        None,
        false,
        3,
        false,
        |compiled| {
            let scalar = |value| assets::EntityGeometryScalar::new(value).unwrap();
            let mut symbols = compiled.molang_symbols.to_vec();
            symbols.push(assets::MolangSymbol {
                kind: assets::MolangSymbolKind::Variable,
                identifier: "variable.random".into(),
            });
            compiled.molang_symbols = symbols.into_boxed_slice();
            compiled.molang_ops = vec![
                MolangOp::Push(scalar(0.0)),
                MolangOp::Push(scalar(1.0)),
                MolangOp::Call(assets::MolangFunction::Random),
                MolangOp::StoreVariable(2),
                MolangOp::LoadQuery(1),
                MolangOp::LoadVariable(2),
                MolangOp::Multiply,
                MolangOp::Push(scalar(0.0)),
                MolangOp::Push(scalar(1.0)),
                MolangOp::Call(assets::MolangFunction::Random),
            ]
            .into_boxed_slice();
            compiled.molang_expressions = [(0, 7, 2), (7, 3, 2)]
                .into_iter()
                .map(
                    |(first_op, op_count, max_stack)| assets::CompiledMolangExpression {
                        first_op,
                        op_count,
                        max_stack,
                    },
                )
                .collect::<Vec<_>>()
                .into_boxed_slice();
            compiled.render.layers[0].color = Some([1; 4]);
        },
    );
    let completed = store.actor_rig(1).unwrap().render[0].color;
    for alpha in [0.25, 0.5, 1.0, 0.5] {
        assert_eq!(
            store.render_frame(alpha).layers(1).unwrap()[0].color,
            completed,
            "body and render share one scratch random sequence"
        );
    }
    assert_eq!(store.actor_rig(1).unwrap().render[0].color, completed);
}

#[test]
fn swell_render_random_preserves_frame_pre_animation_draws() {
    let store = pack_swell_fixture_with(
        AuthoredSwellChannel {
            pre_animation: false,
            property: assets::EntityAnimationProperty::Translation,
            variable: false,
        },
        None,
        false,
        3,
        false,
        |compiled| {
            let scalar = |value| assets::EntityGeometryScalar::new(value).unwrap();
            compiled.molang_symbols = [
                (assets::MolangSymbolKind::Name, "wield"),
                (assets::MolangSymbolKind::Query, "query.frame_alpha"),
                (assets::MolangSymbolKind::Query, "query.swell_amount"),
            ]
            .map(|(kind, identifier)| assets::MolangSymbol {
                kind,
                identifier: identifier.into(),
            })
            .into();
            compiled.molang_ops = vec![
                MolangOp::LoadQuery(1),
                MolangOp::JumpIfFalse(6),
                MolangOp::Push(scalar(0.0)),
                MolangOp::Push(scalar(1.0)),
                MolangOp::Call(assets::MolangFunction::Random),
                MolangOp::Pop,
                MolangOp::Push(scalar(0.0)),
                MolangOp::LoadQuery(2),
                MolangOp::Push(scalar(0.0)),
                MolangOp::Push(scalar(1.0)),
                MolangOp::Call(assets::MolangFunction::Random),
            ]
            .into();
            compiled.molang_expressions = [(0, 7, 2), (7, 1, 1), (8, 3, 2)]
                .map(
                    |(first_op, op_count, max_stack)| assets::CompiledMolangExpression {
                        first_op,
                        op_count,
                        max_stack,
                    },
                )
                .into();
            compiled.rig_bindings[0].pre_animation = Some(0);
            compiled.animation_keyframes[0].expressions = [Some(1), None, None];
            compiled.render.layers[0].color = Some([2; 4]);
        },
    );
    let completed = store.actor_rig(1).unwrap().render[0].color;
    for alpha in [0.25, 0.5, 0.75] {
        let layers = store.render_frame(alpha).layers(1).unwrap();
        assert_eq!(
            &layers[0].color[..3],
            &completed[1..],
            "the pre-animation frame draw precedes body and render effects"
        );
    }
    assert_eq!(store.actor_rig(1).unwrap().render[0].color, completed);
}

#[test]
fn swell_controlled_assignment_taints_later_unconditional_bone_channels() {
    let store = pack_swell_fixture_with(
        AuthoredSwellChannel {
            pre_animation: false,
            property: assets::EntityAnimationProperty::Translation,
            variable: false,
        },
        None,
        false,
        3,
        false,
        |compiled| {
            let scalar = |value| assets::EntityGeometryScalar::new(value).unwrap();
            compiled.molang_symbols = [
                (assets::MolangSymbolKind::Name, "other"),
                (assets::MolangSymbolKind::Name, "wield"),
                (assets::MolangSymbolKind::Query, "query.swell_amount"),
                (assets::MolangSymbolKind::Variable, "variable.factor"),
            ]
            .map(|(kind, identifier)| assets::MolangSymbol {
                kind,
                identifier: identifier.into(),
            })
            .into();
            compiled.molang_ops = vec![
                MolangOp::Push(scalar(2.0)),
                MolangOp::StoreVariable(3),
                MolangOp::Push(scalar(0.0)),
                MolangOp::LoadVariable(3),
                MolangOp::LoadQuery(2),
                MolangOp::Push(scalar(1.5 / crate::actor_store::creeper::SWELL_FULL_TICKS)),
                MolangOp::Greater,
                MolangOp::Push(scalar(0.0)),
                MolangOp::StoreVariable(3),
                MolangOp::Push(scalar(0.0)),
            ]
            .into();
            compiled.molang_expressions = [(0, 3, 1), (3, 1, 1), (4, 3, 2), (7, 3, 1)]
                .map(
                    |(first_op, op_count, max_stack)| assets::CompiledMolangExpression {
                        first_op,
                        op_count,
                        max_stack,
                    },
                )
                .into();
            compiled.rig_bindings[0].pre_animation = Some(3);
            let mut symbols = compiled.symbols.to_vec();
            symbols.insert(
                3,
                assets::EntityAssetSymbol {
                    kind: assets::EntityAssetKind::Animation,
                    identifier: "animation.second".into(),
                    source_index: symbols[2].source_index,
                    dependencies: Box::new([]),
                },
            );
            compiled.symbols = symbols.into();
            compiled.rig_bindings[0].render_controller += 1;
            let mut child = compiled.geometries[0].bones[0].clone();
            child.name = "second".into();
            child.parent = None;
            compiled.geometries[0].bones =
                vec![compiled.geometries[0].bones[0].clone(), child].into();
            let clip = compiled.animation_clips[0];
            compiled.animation_clips = vec![
                clip,
                assets::EntityAnimationClip {
                    symbol: 3,
                    first_channel: 1,
                    ..clip
                },
            ]
            .into();
            let channel = compiled.animation_channels[0].clone();
            compiled.animation_channels = vec![
                channel.clone(),
                assets::EntityAnimationChannel {
                    bone: 1,
                    first_keyframe: 1,
                    ..channel
                },
            ]
            .into();
            let key = compiled.animation_keyframes[0];
            compiled.animation_keyframes = vec![
                key,
                assets::EntityAnimationKeyframe {
                    expressions: [Some(1), None, None],
                    ..key
                },
            ]
            .into();
            compiled.rig_animations = vec![
                assets::EntityRigAnimationBinding {
                    name: 0,
                    clip: 0,
                    weight: Some(2),
                    order: 0,
                },
                assets::EntityRigAnimationBinding {
                    name: 1,
                    clip: 1,
                    weight: None,
                    order: 1,
                },
            ]
            .into();
            compiled.rig_geometries[0].animation_count = 2;
        },
    );
    let layers = store.render_frame(0.5).layers(1).unwrap();
    for pose in [&layers[0].previous_pose[1], &layers[0].pose[1]] {
        assert!(
            (pose.translation_scale[0] + 2.0).abs() < 1e-6,
            "swell-controlled writes reach later independently weighted clips"
        );
    }
}

#[test]
fn swell_pre_animation_samples_presentation_alpha_while_retaining_ordinary_queries() {
    let store = pack_swell_fixture_with(
        AuthoredSwellChannel {
            pre_animation: false,
            property: assets::EntityAnimationProperty::Translation,
            variable: false,
        },
        None,
        false,
        3,
        false,
        |compiled| {
            let scalar = |value| assets::EntityGeometryScalar::new(value).unwrap();
            compiled.molang_symbols = [
                (assets::MolangSymbolKind::Name, "wield"),
                (assets::MolangSymbolKind::Query, "query.frame_alpha"),
                (assets::MolangSymbolKind::Query, "query.swell_amount"),
                (assets::MolangSymbolKind::Variable, "variable.frame"),
            ]
            .map(|(kind, identifier)| assets::MolangSymbol {
                kind,
                identifier: identifier.into(),
            })
            .into();
            compiled.molang_ops = vec![
                MolangOp::LoadQuery(1),
                MolangOp::StoreVariable(3),
                MolangOp::Push(scalar(0.0)),
                MolangOp::LoadQuery(2),
                MolangOp::LoadVariable(3),
                MolangOp::Multiply,
            ]
            .into();
            compiled.molang_expressions = [(0, 3, 1), (3, 3, 2)]
                .map(
                    |(first_op, op_count, max_stack)| assets::CompiledMolangExpression {
                        first_op,
                        op_count,
                        max_stack,
                    },
                )
                .into();
            compiled.rig_bindings[0].pre_animation = Some(0);
            compiled.animation_keyframes[0].expressions = [Some(1), None, None];
        },
    );
    for alpha in [0.25, 0.5, 0.75] {
        let layers = store.render_frame(alpha).layers(1).unwrap();
        let expected = -alpha * (2.0 + alpha) / crate::actor_store::creeper::SWELL_FULL_TICKS;
        for pose in [&layers[0].previous_pose[0], &layers[0].pose[0]] {
            assert!(
                (pose.translation_scale[0] - expected).abs() < 1e-6,
                "presentation alpha is fresh for each ordinary motion endpoint"
            );
        }
    }
}

#[test]
fn swell_controlled_random_draws_taint_later_random_channels() {
    let store = pack_swell_fixture_with(
        AuthoredSwellChannel {
            pre_animation: false,
            property: assets::EntityAnimationProperty::Translation,
            variable: false,
        },
        None,
        false,
        3,
        false,
        |compiled| {
            let scalar = |value| assets::EntityGeometryScalar::new(value).unwrap();
            compiled.molang_symbols = [
                (assets::MolangSymbolKind::Name, "wield"),
                (assets::MolangSymbolKind::Query, "query.swell_amount"),
                (assets::MolangSymbolKind::Variable, "variable.random"),
            ]
            .map(|(kind, identifier)| assets::MolangSymbol {
                kind,
                identifier: identifier.into(),
            })
            .into();
            compiled.molang_ops = vec![
                MolangOp::LoadQuery(1),
                MolangOp::Push(scalar(0.08)),
                MolangOp::Greater,
                MolangOp::JumpIfFalse(8),
                MolangOp::Push(scalar(0.0)),
                MolangOp::Push(scalar(1.0)),
                MolangOp::Call(assets::MolangFunction::Random),
                MolangOp::Pop,
                MolangOp::Push(scalar(0.0)),
                MolangOp::Push(scalar(1.0)),
                MolangOp::Call(assets::MolangFunction::Random),
                MolangOp::StoreVariable(2),
                MolangOp::LoadVariable(2),
                MolangOp::LoadVariable(2),
            ]
            .into();
            compiled.molang_expressions = [(0, 13, 2), (13, 1, 1)]
                .map(
                    |(first_op, op_count, max_stack)| assets::CompiledMolangExpression {
                        first_op,
                        op_count,
                        max_stack,
                    },
                )
                .into();
            compiled.rig_bindings[0].pre_animation = Some(0);
            compiled.animation_keyframes[0].expressions = [Some(1), None, None];
            compiled.render.layers[0].color = Some([1; 4]);
        },
    );
    let completed = store.actor_rig(1).unwrap().current[0].translation_scale[0];
    for alpha in [0.5, 0.75, 1.0, 0.5] {
        let layers = store.render_frame(alpha).layers(1).unwrap();
        assert!(
            (layers[0].pose[0].translation_scale[0] + layers[0].color[0]).abs() < 1e-6,
            "body and render consume the same conditional random sequence"
        );
    }
    assert_eq!(
        store.actor_rig(1).unwrap().current[0].translation_scale[0],
        completed
    );
}

#[test]
fn swell_sampled_rig_and_bone_scales_preserve_authored_cancellation() {
    for slot in 0..4 {
        let store = pack_swell_fixture_with(
            AuthoredSwellChannel {
                pre_animation: true,
                property: assets::EntityAnimationProperty::Scale,
                variable: true,
            },
            None,
            false,
            3,
            false,
            |compiled| {
                let scalar = |value| assets::EntityGeometryScalar::new(value).unwrap();
                compiled.rig_bindings[0].pre_animation = None;
                compiled.molang_ops = vec![
                    MolangOp::LoadQuery(2),
                    MolangOp::Push(scalar(1.0)),
                    MolangOp::Add,
                    MolangOp::StoreVariable(3),
                    MolangOp::LoadVariable(3),
                    MolangOp::Push(scalar(1.0)),
                    MolangOp::Push(scalar(1.0)),
                    MolangOp::LoadVariable(3),
                    MolangOp::Divide,
                ]
                .into_boxed_slice();
                compiled.molang_expressions = [(0, 5, 2), (5, 1, 1), (6, 3, 2)]
                    .into_iter()
                    .map(
                        |(first_op, op_count, max_stack)| assets::CompiledMolangExpression {
                            first_op,
                            op_count,
                            max_stack,
                        },
                    )
                    .collect::<Vec<_>>()
                    .into_boxed_slice();
                let mut scales = [1; 4];
                scales[slot] = 0;
                compiled.rig_bindings[0].scale_expressions = Some(scales);
                compiled.animation_keyframes[0].value = [scalar(1.0); 3];
                compiled.animation_keyframes[0].expressions = if slot == 0 {
                    [Some(2); 3]
                } else {
                    let mut axes = [None; 3];
                    axes[slot - 1] = Some(2);
                    axes
                };
            },
        );
        let rig = store.actor_rig(1).unwrap();
        let tick_scale = [
            rig.scale,
            rig.axis_scale[0],
            rig.axis_scale[1],
            rig.axis_scale[2],
        ];
        for alpha in [0.0, 0.25, 0.5, 0.75, 1.0] {
            let layers = store.render_frame(alpha).layers(1).unwrap();
            for layer in layers.iter() {
                let scale = layer.sampled_scale.unwrap_or(tick_scale);
                for pose in [&layer.previous_pose[0], &layer.pose[0]] {
                    for (axis, bone) in pose::total_scale(pose).into_iter().enumerate() {
                        assert!(
                            (scale[0] * scale[axis + 1] * bone - 1.0).abs() < 1e-5,
                            "rig and bone scales cancel at the same frame: slot={slot},alpha={alpha},scale={scale:?},bone={bone}"
                        );
                    }
                }
            }
        }
        let after = store.actor_rig(1).unwrap();
        assert_eq!(
            [
                after.scale,
                after.axis_scale[0],
                after.axis_scale[1],
                after.axis_scale[2]
            ],
            tick_scale
        );
    }
}
