use super::*;

#[test]
fn swell_driven_clip_time_samples_fraction_without_advancing_tick_clock() {
    let store = pack_swell_fixture(
        false,
        assets::EntityAnimationProperty::Translation,
        false,
        None,
        false,
        3,
        true,
    );
    let tick = store.actor_rig(1).unwrap().completed_tick;
    for alpha in [0.25, 0.5, 0.75] {
        let layers = store.render_frame(alpha).layers(1).unwrap();
        let expected = -(2.0 + alpha) / crate::actor_store::creeper::SWELL_FULL_TICKS;
        for pose in [&layers[0].previous_pose[0], &layers[0].pose[0]] {
            assert!((pose.translation_scale[0] - expected).abs() < 1e-6);
        }
    }
    assert_eq!(store.actor_rig(1).unwrap().completed_tick, tick);
}

#[test]
fn cumulative_swell_clock_reuses_the_pre_update_baseline() {
    let store = pack_swell_fixture_with(
        AuthoredSwellChannel {
            pre_animation: false,
            property: assets::EntityAnimationProperty::Translation,
            variable: false,
        },
        None,
        false,
        7,
        true,
        |compiled| {
            compiled.molang_symbols = [
                (assets::MolangSymbolKind::Name, "wield"),
                (assets::MolangSymbolKind::Query, "query.anim_time"),
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
                MolangOp::LoadQuery(1),
                MolangOp::LoadQuery(2),
                MolangOp::Add,
            ]
            .into_boxed_slice();
            compiled.molang_expressions[0].op_count = 3;
            compiled.molang_expressions[0].max_stack = 2;
        },
    );
    let tick = store.actor_rig(1).unwrap().completed_tick;
    let completed = store.actor_rig(1).unwrap().current[0].translation_scale[0];
    for alpha in [0.0, 0.25, 0.75, 1.0] {
        let layers = store.render_frame(alpha).layers(1).unwrap();
        let expected = completed - alpha / crate::actor_store::creeper::SWELL_FULL_TICKS;
        assert!(
            (layers[0].pose[0].translation_scale[0] - expected).abs() < 1e-6,
            "scratch clock must recompute the same cumulative update"
        );
    }
    assert_eq!(store.actor_rig(1).unwrap().completed_tick, tick);
}

#[test]
fn swell_weight_and_clock_assignments_publish_without_advancing_state() {
    for clock in [false, true] {
        let store = pack_swell_fixture_with(
            AuthoredSwellChannel {
                pre_animation: false,
                property: assets::EntityAnimationProperty::Translation,
                variable: false,
            },
            (!clock).then_some(false),
            false,
            3,
            clock,
            |compiled| {
                let mut symbols = compiled.molang_symbols.to_vec();
                symbols.push(assets::MolangSymbol {
                    kind: assets::MolangSymbolKind::Variable,
                    identifier: "variable.color".into(),
                });
                compiled.molang_symbols = symbols.into_boxed_slice();
                compiled.molang_ops = vec![
                    MolangOp::LoadQuery(1),
                    MolangOp::StoreVariable(2),
                    MolangOp::LoadVariable(2),
                    MolangOp::LoadVariable(2),
                ]
                .into_boxed_slice();
                compiled.molang_expressions = [(0, 3), (3, 1)]
                    .into_iter()
                    .map(|(first_op, op_count)| assets::CompiledMolangExpression {
                        first_op,
                        op_count,
                        max_stack: 1,
                    })
                    .collect::<Vec<_>>()
                    .into_boxed_slice();
                compiled.render.layers[0].color = Some([1; 4]);
            },
        );
        let tick = store.actor_rig(1).unwrap().completed_tick;
        for alpha in [0.25, 0.5, 1.0, 0.5] {
            let amount = (2.0 + alpha) / crate::actor_store::creeper::SWELL_FULL_TICKS;
            let layers = store.render_frame(alpha).layers(1).unwrap();
            assert!(
                (layers[0].color[0] - amount).abs() < 1e-6,
                "weight/clock assignments reach render selection"
            );
        }
        assert_eq!(store.actor_rig(1).unwrap().completed_tick, tick);
    }
}

#[test]
fn swell_shared_clock_updates_execute_once_per_endpoint() {
    let store = pack_swell_fixture_with(
        AuthoredSwellChannel {
            pre_animation: false,
            property: assets::EntityAnimationProperty::Translation,
            variable: false,
        },
        None,
        false,
        3,
        true,
        |compiled| {
            compiled.molang_symbols = [
                (assets::MolangSymbolKind::Name, "other"),
                (assets::MolangSymbolKind::Name, "wield"),
                (assets::MolangSymbolKind::Query, "query.swell_amount"),
                (assets::MolangSymbolKind::Variable, "variable.counter"),
            ]
            .into_iter()
            .map(|(kind, identifier)| assets::MolangSymbol {
                kind,
                identifier: identifier.into(),
            })
            .collect::<Vec<_>>()
            .into_boxed_slice();
            compiled.molang_ops = vec![
                MolangOp::LoadVariable(3),
                MolangOp::LoadQuery(2),
                MolangOp::Add,
                MolangOp::StoreVariable(3),
                MolangOp::LoadVariable(3),
                MolangOp::LoadVariable(3),
            ]
            .into_boxed_slice();
            compiled.molang_expressions = [(0, 5, 2), (5, 1, 1)]
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
            let binding = compiled.rig_animations[0];
            compiled.rig_animations = vec![
                assets::EntityRigAnimationBinding { name: 0, ..binding },
                assets::EntityRigAnimationBinding {
                    name: 1,
                    order: 1,
                    ..binding
                },
            ]
            .into_boxed_slice();
            compiled.rig_geometries[0].animation_count = 2;
            compiled.render.layers[0].color = Some([1; 4]);
        },
    );
    let completed = store.actor_rig(1).unwrap().render[0].color[0];
    for alpha in [0.25, 0.5, 1.0, 0.5] {
        let layers = store.render_frame(alpha).layers(1).unwrap();
        let expected = completed + alpha / crate::actor_store::creeper::SWELL_FULL_TICKS;
        assert!(
            (layers[0].color[0] - expected).abs() < 1e-6,
            "duplicate clock references publish one update: {} expected {} completed {} alpha {}",
            layers[0].color[0],
            expected,
            completed,
            alpha
        );
        assert!(
            (layers[0].pose[0].translation_scale[0] + 2.0 * expected).abs() < 1e-6,
            "duplicate references share the same sampled time"
        );
    }
    assert_eq!(store.actor_rig(1).unwrap().render[0].color[0], completed);
}

fn assert_swell_stage_assignments(scale: bool) {
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
                identifier: "variable.factor".into(),
            });
            compiled.molang_symbols = symbols.into_boxed_slice();
            compiled.molang_ops = vec![
                MolangOp::LoadQuery(1),
                MolangOp::LoadVariable(2),
                MolangOp::Multiply,
                MolangOp::Push(scalar(2.0)),
                MolangOp::StoreVariable(2),
                MolangOp::Push(scalar(if scale { 1.0 } else { 0.0 })),
                MolangOp::Push(scalar(1.0)),
                MolangOp::StoreVariable(2),
                MolangOp::Push(scalar(0.0)),
            ]
            .into_boxed_slice();
            compiled.molang_expressions = [(0, 3, 2), (3, 3, 1), (6, 3, 1)]
                .map(
                    |(first_op, op_count, max_stack)| assets::CompiledMolangExpression {
                        first_op,
                        op_count,
                        max_stack,
                    },
                )
                .into();
            compiled.rig_bindings[0].pre_animation = Some(2);
            if scale {
                compiled.rig_bindings[0].scale_expressions = Some([1; 4]);
            } else {
                compiled.animation_clips[0].anim_time_update = Some(1);
            }
        },
    );
    for alpha in [0.25, 0.5, 1.0] {
        let layers = store.render_frame(alpha).layers(1).unwrap();
        let expected = -2.0 * (2.0 + alpha) / crate::actor_store::creeper::SWELL_FULL_TICKS;
        for pose in [&layers[0].previous_pose[0], &layers[0].pose[0]] {
            assert!(
                (pose.translation_scale[0] - expected).abs() < 1e-6,
                "assignments preceding bones retain their authored stage"
            );
        }
    }
}

#[test]
fn swell_channels_retain_assignments_from_ordinary_clocks() {
    assert_swell_stage_assignments(false);
}

#[test]
fn swell_channels_retain_assignments_from_rig_scale() {
    assert_swell_stage_assignments(true);
}

#[test]
fn swell_activated_paused_clock_resumes_from_its_completed_time() {
    let store = pack_swell_fixture_with(
        AuthoredSwellChannel {
            pre_animation: false,
            property: assets::EntityAnimationProperty::Translation,
            variable: false,
        },
        None,
        false,
        3,
        true,
        |compiled| {
            let scalar = |value| assets::EntityGeometryScalar::new(value).unwrap();
            compiled.molang_symbols = [
                (assets::MolangSymbolKind::Name, "wield"),
                (assets::MolangSymbolKind::Query, "query.anim_time"),
                (assets::MolangSymbolKind::Query, "query.swell_amount"),
            ]
            .map(|(kind, identifier)| assets::MolangSymbol {
                kind,
                identifier: identifier.into(),
            })
            .into();
            compiled.molang_ops = vec![
                MolangOp::LoadQuery(1),
                MolangOp::LoadQuery(2),
                MolangOp::Add,
                MolangOp::LoadQuery(2),
                MolangOp::Push(scalar(0.05)),
                MolangOp::LessEqual,
                MolangOp::LoadQuery(2),
                MolangOp::Push(scalar(0.08)),
                MolangOp::Greater,
                MolangOp::Add,
            ]
            .into();
            compiled.molang_expressions = [(0, 3, 2), (3, 7, 3)]
                .map(
                    |(first_op, op_count, max_stack)| assets::CompiledMolangExpression {
                        first_op,
                        op_count,
                        max_stack,
                    },
                )
                .into();
            compiled.rig_animations[0].weight = Some(1);
        },
    );
    let completed_time = -store.actor_rig(1).unwrap().previous[0].translation_scale[0];
    assert!(completed_time > 0.0);
    for alpha in [0.5, 0.75, 1.0] {
        let layers = store.render_frame(alpha).layers(1).unwrap();
        let expected =
            -(completed_time + (2.0 + alpha) / crate::actor_store::creeper::SWELL_FULL_TICKS);
        assert!(
            (layers[0].pose[0].translation_scale[0] - expected).abs() < 1e-6,
            "a paused clock starts after its last committed increment"
        );
    }
    assert_eq!(
        -store.actor_rig(1).unwrap().previous[0].translation_scale[0],
        completed_time
    );
}

#[test]
fn camera_and_swell_share_one_authored_clock_update_per_frame() {
    for clock_reads_swell in [false, true] {
        let mut store = pack_swell_fixture_with(
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
                    (assets::MolangSymbolKind::Query, "query.anim_time"),
                    (assets::MolangSymbolKind::Query, "query.camera_rotation"),
                    (
                        assets::MolangSymbolKind::Query,
                        "query.modified_distance_moved",
                    ),
                    (assets::MolangSymbolKind::Query, "query.swell_amount"),
                    (assets::MolangSymbolKind::Variable, "variable.counter"),
                ]
                .map(|(kind, identifier)| assets::MolangSymbol {
                    kind,
                    identifier: identifier.into(),
                })
                .into();
                let mut ops = vec![
                    MolangOp::LoadVariable(5),
                    MolangOp::Push(scalar(1.0)),
                    MolangOp::Add,
                    MolangOp::StoreVariable(5),
                    MolangOp::LoadVariable(5),
                    MolangOp::LoadQuery(3),
                    MolangOp::Add,
                ];
                if clock_reads_swell {
                    ops.extend([MolangOp::LoadQuery(4), MolangOp::Add]);
                }
                let count = ops.len();
                ops.extend([
                    MolangOp::LoadQuery(1),
                    MolangOp::LoadQuery(2),
                    MolangOp::Add,
                    MolangOp::LoadVariable(5),
                    MolangOp::LoadQuery(4),
                    MolangOp::Push(scalar(0.0)),
                    MolangOp::Multiply,
                    MolangOp::Push(scalar(1.0)),
                    MolangOp::Add,
                ]);
                compiled.molang_ops = ops.into();
                compiled.molang_expressions = [
                    (0, count, 2),
                    (count, 3, 2),
                    (count + 3, 1, 1),
                    (count + 4, 5, 2),
                ]
                .map(
                    |(first_op, op_count, max_stack)| assets::CompiledMolangExpression {
                        first_op: first_op as u32,
                        op_count: op_count as u16,
                        max_stack,
                    },
                )
                .into();
                compiled.animation_clips[0].anim_time_update = Some(0);
                compiled.animation_clips[0].length_seconds = scalar(0.0);
                compiled.animation_keyframes[0].expressions = [Some(1), None, None];
                compiled.rig_animations[0].weight = Some(3);
                compiled.render.layers[0].color = Some([2; 4]);
            },
        );
        store.set_camera_rotation([0.0, 90.0]);
        let completed = store.actor_rig(1).unwrap().render[0].color[0];
        assert_eq!(completed, 3.0);
        for alpha in [0.25, 0.75, 0.25] {
            let layers = store.render_frame(alpha).layers(1).unwrap();
            assert_eq!(layers[0].color[0], 3.0, "clock writes at {alpha}");
            let time = 3.0
                + if clock_reads_swell {
                    (2.0 + alpha) / crate::actor_store::creeper::SWELL_FULL_TICKS
                } else {
                    0.0
                };
            assert!(
                (layers[0].pose[0].translation_scale[0] + time).abs() < 1e-6,
                "sampled time at {alpha}"
            );
        }
        assert_eq!(store.actor_rig(1).unwrap().render[0].color[0], completed);
    }
}
