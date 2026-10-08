use super::*;

#[test]
fn direct_swell_channel_samples_fraction() {
    let store = compiled_swell_fixture(false);
    let rig = store.actor_rig(1).unwrap();
    let layers = store.render_frame(0.5).layers(1).unwrap();
    let (previous, current) = if layers[0].pose.is_empty() {
        (&rig.previous[0], &rig.current[0])
    } else {
        (&layers[0].previous_pose[0], &layers[0].pose[0])
    };
    let drawn_x = (previous.translation_scale[0] + current.translation_scale[0]) * 0.5;
    assert_eq!(
        drawn_x,
        -2.5 / crate::actor_store::creeper::SWELL_FULL_TICKS
    );
}

#[test]
fn swell_translation_with_pre_animation_samples_fraction() {
    let store = compiled_swell_fixture(true);
    let layers = store.render_frame(0.5).layers(1).unwrap();
    let drawn_x = (layers[0].previous_pose[0].translation_scale[0]
        + layers[0].pose[0].translation_scale[0])
        * 0.5;
    assert_eq!(
        drawn_x,
        -2.5 / crate::actor_store::creeper::SWELL_FULL_TICKS
    );
}

#[test]
fn authored_swell_trs_channels_follow_query_and_pre_animation_variables() {
    use assets::EntityAnimationProperty::*;
    for variable in [false, true] {
        for property in [Translation, Rotation, Scale] {
            let store = authored_swell_fixture(variable, property, variable);
            for alpha in [0.0, 0.25, 0.75, 1.0] {
                let value = (2.0 + alpha) / crate::actor_store::creeper::SWELL_FULL_TICKS;
                let layers = store.render_frame(alpha).layers(1).unwrap();
                assert!(!layers[0].pose.is_empty());
                for pose in [&layers[0].previous_pose[0], &layers[0].pose[0]] {
                    match property {
                        Translation => assert_eq!(pose.translation_scale[0], -value),
                        Rotation => {
                            assert_eq!(pose.rotation, pose::quat_from_euler([-value, 0.0, 0.0]))
                        }
                        Scale => assert!((pose::total_scale(pose)[0] - value).abs() < 1e-6),
                    }
                }
            }
        }
    }
}

#[test]
fn swell_script_preserves_an_independent_motion_variable_channel() {
    let store = authored_swell_fixture(true, assets::EntityAnimationProperty::Translation, true);
    let rig = store.actor_rig(1).unwrap();
    assert_ne!(
        rig.previous[0].translation_scale[1],
        rig.current[0].translation_scale[1]
    );
    for alpha in [0.0, 0.25, 0.75, 1.0] {
        let layers = store.render_frame(alpha).layers(1).unwrap();
        assert_eq!(
            layers[0].previous_pose[0].translation_scale[1],
            rig.previous[0].translation_scale[1]
        );
        assert_eq!(
            layers[0].pose[0].translation_scale[1],
            rig.current[0].translation_scale[1]
        );
    }
}

#[test]
fn swell_layers_follow_geometry_specific_bone_order() {
    let store = pack_swell_fixture(
        false,
        assets::EntityAnimationProperty::Translation,
        false,
        None,
        true,
        3,
        false,
    );
    for alpha in [0.25, 0.5, 0.75] {
        let layers = store.render_frame(alpha).layers(1).unwrap();
        assert_eq!(layers.len(), 2);
        let expected = -(2.0 + alpha) / crate::actor_store::creeper::SWELL_FULL_TICKS;
        for pose in [&layers[1].previous_pose[1], &layers[1].pose[1]] {
            assert!((pose.translation_scale[0] - expected).abs() < 1e-6);
        }
    }
}

#[test]
fn swell_and_motion_share_one_axis_without_losing_motion_history() {
    for operation in [MolangOp::Add, MolangOp::Multiply] {
        let store = pack_swell_fixture_with(
            AuthoredSwellChannel {
                pre_animation: true,
                property: assets::EntityAnimationProperty::Translation,
                variable: true,
            },
            None,
            false,
            7,
            false,
            |compiled| {
                let mut ops = compiled.molang_ops.to_vec();
                ops.extend([
                    MolangOp::LoadVariable(3),
                    MolangOp::LoadVariable(4),
                    operation,
                ]);
                compiled.molang_ops = ops.into_boxed_slice();
                let mut expressions = compiled.molang_expressions.to_vec();
                expressions.push(assets::CompiledMolangExpression {
                    first_op: 7,
                    op_count: 3,
                    max_stack: 2,
                });
                compiled.molang_expressions = expressions.into_boxed_slice();
                compiled.animation_keyframes[0].expressions = [Some(3), None, None];
            },
        );
        for alpha in [0.0, 0.25, 0.75, 1.0] {
            let swell = (6.0 + alpha) / crate::actor_store::creeper::SWELL_FULL_TICKS;
            let layers = store.render_frame(alpha).layers(1).unwrap();
            for (pose, life_tick) in [
                (&layers[0].previous_pose[0], 6.0),
                (&layers[0].pose[0], 7.0),
            ] {
                let motion = life_tick * ACTOR_TICK_DURATION.as_secs_f32();
                let expected = -match operation {
                    MolangOp::Add => swell + motion,
                    _ => swell * motion,
                };
                assert!(
                    (pose.translation_scale[0] - expected).abs() < 1e-6,
                    "independent motion retains its endpoint while swell samples the fraction"
                );
            }
        }
    }
}

#[test]
fn swell_on_an_alternate_only_bone_samples_the_frame_fraction() {
    let store = pack_swell_fixture_with(
        AuthoredSwellChannel {
            pre_animation: false,
            property: assets::EntityAnimationProperty::Translation,
            variable: false,
        },
        None,
        true,
        8,
        false,
        |compiled| {
            compiled.animation_clips[0].channel_count = 0;
            compiled.animation_clips[1].first_channel = 0;
            let mut channel = compiled.animation_channels[1].clone();
            channel.bone = 0;
            channel.first_keyframe = 0;
            compiled.animation_channels = vec![channel].into_boxed_slice();
            compiled.animation_keyframes = vec![compiled.animation_keyframes[1]].into_boxed_slice();
        },
    );
    for alpha in [0.0, 0.25, 0.75, 1.0] {
        let layers = store.render_frame(alpha).layers(1).unwrap();
        let expected = -(7.0 + alpha) / crate::actor_store::creeper::SWELL_FULL_TICKS;
        for pose in [&layers[1].previous_pose[0], &layers[1].pose[0]] {
            assert!(
                (pose.translation_scale[0] - expected).abs() < 1e-6,
                "alternate-only channel uses the fractional swell query"
            );
        }
    }
}

#[test]
fn swell_weighted_override_resets_every_transform_of_its_bone() {
    let store = swell_override_fixture(false, 1);
    for alpha in [0.25, 0.5, 0.75, 1.0] {
        let amount = alpha / crate::actor_store::creeper::SWELL_FULL_TICKS;
        let layers = store.render_frame(alpha).layers(1).unwrap();
        for endpoint in [&layers[0].previous_pose[0], &layers[0].pose[0]] {
            assert_eq!(
                endpoint.rotation,
                [0.0, 0.0, 0.0, 1.0],
                "override resets cached rotations"
            );
            assert_eq!(
                endpoint.axis_scale[0] * endpoint.translation_scale[3],
                1.0,
                "override resets cached scales"
            );
            assert!(
                (endpoint.translation_scale[0] + amount).abs() < 1e-6,
                "override resets prior translations"
            );
        }
    }
}

#[test]
fn swell_override_restores_the_inherited_rotation_and_scale_basis() {
    let store = swell_override_fixture(true, 1);
    for alpha in [0.25, 0.5, 0.75, 1.0] {
        let layers = store.render_frame(alpha).layers(1).unwrap();
        for pose in [&layers[0].previous_pose, &layers[0].pose] {
            assert_eq!(
                pose[1].rotation, pose[0].rotation,
                "override restores the parent rotation basis"
            );
            assert_eq!(
                pose[1].translation_scale[3], 2.0,
                "override restores inherited scale"
            );
        }
    }
}

#[test]
fn swell_override_deactivation_restores_previous_animation_channels() {
    let mut store = swell_override_fixture(false, 1);
    store.apply(
        1,
        2,
        protocol::ActorEvent::Metadata(protocol::ActorMetadataUpdateEvent {
            dimension: 0,
            runtime_id: 1,
            tick: 0,
            metadata: Arc::from([protocol::ActorMetadata {
                key: 0,
                value: ActorMetadataValue::Flags(0),
            }]),
            properties: Arc::from([]),
        }),
    );
    store.advance_interpolation_ticks(1);
    let layers = store.render_frame(1.0).layers(1).unwrap();
    let expected = pose::quat_from_euler([-45.0, -45.0, 45.0]);
    for endpoint in [&layers[0].previous_pose[0], &layers[0].pose[0]] {
        assert_eq!(
            endpoint.rotation, expected,
            "inactive override restores rotation from the earlier clip"
        );
        assert_eq!(
            endpoint.translation_scale[3], 2.0,
            "inactive override restores scale"
        );
        assert_eq!(
            endpoint.translation_scale[0], -3.0,
            "inactive override restores translation"
        );
    }
}

#[test]
fn swell_render_writes_keep_each_geometry_motion_endpoint() {
    let store = pack_swell_fixture_with(
        AuthoredSwellChannel {
            pre_animation: false,
            property: assets::EntityAnimationProperty::Translation,
            variable: false,
        },
        None,
        true,
        7,
        false,
        |compiled| {
            compiled.molang_symbols = [
                (assets::MolangSymbolKind::Name, "wield"),
                (assets::MolangSymbolKind::Query, "query.life_time"),
                (assets::MolangSymbolKind::Query, "query.swell_amount"),
                (assets::MolangSymbolKind::Variable, "variable.layer"),
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
                MolangOp::LoadQuery(2),
                MolangOp::LoadQuery(1),
                MolangOp::Add,
                MolangOp::StoreVariable(3),
                MolangOp::LoadVariable(3),
                MolangOp::LoadVariable(3),
            ]
            .into_boxed_slice();
            compiled.molang_expressions = [(0, 1, 1), (1, 5, 2), (6, 1, 1)]
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
            compiled.animation_keyframes[1].expressions = [Some(2), None, None];
        },
    );
    for alpha in [0.0, 0.25, 0.75, 1.0] {
        let amount = (6.0 + alpha) / crate::actor_store::creeper::SWELL_FULL_TICKS;
        let layers = store.render_frame(alpha).layers(1).unwrap();
        for (endpoint, tick) in [
            (&layers[1].previous_pose[1], 6.0),
            (&layers[1].pose[1], 7.0),
        ] {
            let expected = -(amount + tick * ACTOR_TICK_DURATION.as_secs_f32());
            assert!(
                (endpoint.translation_scale[0] - expected).abs() < 1e-6,
                "render assignment keeps the geometry's ordinary endpoint: actual {} expected {}",
                endpoint.translation_scale[0],
                expected
            );
        }
    }
}

#[test]
fn swell_selected_geometry_synthesizes_both_ordinary_motion_endpoints() {
    let store = pack_swell_fixture_with(
        AuthoredSwellChannel {
            pre_animation: false,
            property: assets::EntityAnimationProperty::Translation,
            variable: false,
        },
        None,
        true,
        3,
        false,
        |compiled| {
            let scalar = |value| assets::EntityGeometryScalar::new(value).unwrap();
            compiled.molang_symbols = [
                (assets::MolangSymbolKind::Name, "wield"),
                (assets::MolangSymbolKind::Query, "query.life_time"),
                (assets::MolangSymbolKind::Query, "query.swell_amount"),
            ]
            .map(|(kind, identifier)| assets::MolangSymbol {
                kind,
                identifier: identifier.into(),
            })
            .into();
            compiled.molang_ops = vec![
                MolangOp::LoadQuery(2),
                MolangOp::LoadQuery(1),
                MolangOp::LoadQuery(2),
                MolangOp::Push(scalar(0.08)),
                MolangOp::Greater,
            ]
            .into();
            compiled.molang_expressions = [(0, 1, 1), (1, 1, 1), (2, 3, 2)]
                .map(
                    |(first_op, op_count, max_stack)| assets::CompiledMolangExpression {
                        first_op,
                        op_count,
                        max_stack,
                    },
                )
                .into();
            for key in &mut compiled.animation_keyframes {
                key.expressions = [Some(0), Some(1), None];
            }
            compiled.render.layers[1].condition = Some(2);
        },
    );
    assert_eq!(store.actor_rig(1).unwrap().render.len(), 1);
    for alpha in [0.5, 0.75, 1.0, 0.5] {
        let layers = store.render_frame(alpha).layers(1).unwrap();
        assert_eq!(layers.len(), 2);
        for (pose, tick) in [
            (&layers[1].previous_pose[1], 2.0),
            (&layers[1].pose[1], 3.0),
        ] {
            assert!(
                (pose.translation_scale[1] - tick * ACTOR_TICK_DURATION.as_secs_f32()).abs() < 1e-6,
                "new geometry retains both ordinary motion endpoints: actual {} tick {}",
                pose.translation_scale[1],
                tick,
            );
        }
    }
}

#[test]
fn swell_camera_capability_keeps_unchanged_camera_motion_history() {
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
            compiled.molang_symbols = [
                (assets::MolangSymbolKind::Name, "wield"),
                (
                    assets::MolangSymbolKind::Query,
                    "query.distance_from_camera",
                ),
                (assets::MolangSymbolKind::Query, "query.life_time"),
                (assets::MolangSymbolKind::Query, "query.swell_amount"),
            ]
            .map(|(kind, identifier)| assets::MolangSymbol {
                kind,
                identifier: identifier.into(),
            })
            .into();
            compiled.molang_ops = vec![
                MolangOp::LoadQuery(3),
                MolangOp::LoadQuery(2),
                MolangOp::LoadQuery(1),
            ]
            .into();
            compiled.molang_expressions = [0, 1, 2]
                .map(|first_op| assets::CompiledMolangExpression {
                    first_op,
                    op_count: 1,
                    max_stack: 1,
                })
                .into();
            compiled.animation_keyframes[0].expressions = [Some(0), Some(1), Some(2)];
        },
    );
    for alpha in [0.0, 0.25, 0.75, 1.0] {
        let layers = store.render_frame(alpha).layers(1).unwrap();
        for (pose, tick) in [
            (&layers[0].previous_pose[0], 2.0),
            (&layers[0].pose[0], 3.0),
        ] {
            assert!(
                (pose.translation_scale[1] - tick * ACTOR_TICK_DURATION.as_secs_f32()).abs() < 1e-6,
                "unchanged camera keeps ordinary motion endpoints"
            );
        }
    }
}
