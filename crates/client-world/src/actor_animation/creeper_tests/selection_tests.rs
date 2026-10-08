use super::*;

#[test]
fn swell_weights_sample_query_and_constant_channels_at_frame_fraction() {
    for query_channel in [true, false] {
        let store = pack_swell_fixture(
            false,
            assets::EntityAnimationProperty::Translation,
            false,
            Some(query_channel),
            false,
            3,
            false,
        );
        for alpha in [0.25, 0.5, 0.75] {
            let swell = (2.0 + alpha) / crate::actor_store::creeper::SWELL_FULL_TICKS;
            let expected = if query_channel { swell * swell } else { swell };
            let layers = store.render_frame(alpha).layers(1).unwrap();
            for pose in [&layers[0].previous_pose[0], &layers[0].pose[0]] {
                assert!((pose.translation_scale[0] + expected).abs() < 1e-6);
            }
        }
    }
}

#[test]
fn swell_weight_can_activate_a_clip_between_zero_weight_ticks() {
    let store = pack_swell_fixture(
        false,
        assets::EntityAnimationProperty::Translation,
        false,
        Some(false),
        false,
        1,
        false,
    );
    let tick = store.actor_rig(1).unwrap().completed_tick;
    for alpha in [0.25, 0.5, 0.75] {
        let layers = store.render_frame(alpha).layers(1).unwrap();
        let expected = -alpha / crate::actor_store::creeper::SWELL_FULL_TICKS;
        for pose in [&layers[0].previous_pose[0], &layers[0].pose[0]] {
            assert!((pose.translation_scale[0] - expected).abs() < 1e-6);
        }
    }
    assert_eq!(store.actor_rig(1).unwrap().completed_tick, tick);
}

#[test]
fn deactivated_swell_weights_remove_retained_channel_values() {
    for query_channel in [false, true] {
        let mut store = pack_swell_fixture(
            false,
            assets::EntityAnimationProperty::Translation,
            false,
            Some(query_channel),
            false,
            3,
            false,
        );
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
        store.advance_interpolation_ticks(3);
        let layers = store.render_frame(1.0).layers(1).unwrap();
        for pose in [&layers[0].previous_pose[0], &layers[0].pose[0]] {
            assert_eq!(
                pose.translation_scale[0], 0.0,
                "zero-weight channels return to rest"
            );
        }
        store.advance_interpolation_ticks(1);
        let layers = store.render_frame(0.25).layers(1).unwrap();
        for pose in [&layers[0].previous_pose[0], &layers[0].pose[0]] {
            assert_eq!(
                pose.translation_scale[0], 0.0,
                "the settling tick also stays at rest"
            );
        }
    }
}

#[test]
fn swell_weights_retain_each_endpoint_controller_state() {
    let store = swell_controller_fixture(3, 0.12, false);
    let tick = store.actor_rig(1).unwrap().completed_tick;
    for alpha in [0.0, 0.25, 0.75, 1.0] {
        let layers = store.render_frame(alpha).layers(1).unwrap();
        let swell = (2.0 + alpha) / crate::actor_store::creeper::SWELL_FULL_TICKS;
        assert!(
            (layers[0].previous_pose[0].translation_scale[0] + swell * swell).abs() < 1e-6,
            "previous endpoint keeps its earlier controller state"
        );
        assert!(
            (layers[0].pose[0].translation_scale[0] + (swell + 1.0) * swell).abs() < 1e-6,
            "current endpoint keeps its transitioned controller state"
        );
    }
    assert_eq!(store.actor_rig(1).unwrap().completed_tick, tick);
}

#[test]
fn previous_controller_can_activate_a_swell_channel_at_the_frame_fraction() {
    let store = swell_controller_fixture(2, 0.07, true);
    for alpha in [0.25, 0.5, 0.75] {
        let layers = store.render_frame(alpha).layers(1).unwrap();
        let swell = (1.0 + alpha) / crate::actor_store::creeper::SWELL_FULL_TICKS;
        assert!(
            (layers[0].previous_pose[0].translation_scale[0] + swell * swell).abs() < 1e-6,
            "newly active previous-endpoint channels join the mask: alpha {alpha}, value {}, swell {swell}",
            layers[0].previous_pose[0].translation_scale[0]
        );
        assert_eq!(
            layers[0].pose[0].translation_scale[0], 0.0,
            "transitioned zero-weight current state stays at rest"
        );
    }
}

#[test]
fn swell_assignments_drive_geometry_texture_and_visibility_selection() {
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
            let mut symbols = compiled.molang_symbols.to_vec();
            symbols.push(assets::MolangSymbol {
                kind: assets::MolangSymbolKind::Variable,
                identifier: "variable.selection".into(),
            });
            compiled.molang_symbols = symbols.into_boxed_slice();
            compiled.molang_ops = vec![
                MolangOp::LoadQuery(1),
                MolangOp::StoreVariable(2),
                MolangOp::LoadVariable(2),
                MolangOp::LoadVariable(2),
                MolangOp::LoadVariable(2),
                MolangOp::Push(scalar(0.06)),
                MolangOp::Greater,
            ]
            .into_boxed_slice();
            compiled.molang_expressions = [(0, 3, 1), (3, 1, 1), (4, 3, 2)]
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
            compiled.render.layers[1].condition = Some(2);
            compiled.render.geometries[0].condition = Some(2);
            for candidate in &mut compiled.render.candidates {
                candidate.condition = Some(2);
            }
            compiled.render.visibility = vec![assets::EntityRenderVisibility {
                pattern: "*".into(),
                condition: 2,
            }]
            .into_boxed_slice();
            compiled.render.layers[0].visibility_count = 1;
            compiled.render.layers[1].first_visibility = 1;
        },
    );
    let layers = store.render_frame(0.5).layers(1).unwrap();
    assert_eq!(
        layers.len(),
        2,
        "sampled assignment enables both render layers and texture choices"
    );
    assert_eq!(
        layers[1].geometry,
        Some(1),
        "sampled assignment selects the alternate geometry"
    );
    assert!(
        layers[0].hidden_bones.is_empty(),
        "sampled assignment controls bone visibility"
    );
    assert!(
        (layers[0].color[0] - 2.5 / crate::actor_store::creeper::SWELL_FULL_TICKS).abs() < 1e-6
    );
}

#[test]
fn swell_render_controller_writes_feed_selected_geometry_channels() {
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
            let mut symbols = compiled.molang_symbols.to_vec();
            symbols.push(assets::MolangSymbol {
                kind: assets::MolangSymbolKind::Variable,
                identifier: "variable.layer".into(),
            });
            compiled.molang_symbols = symbols.into_boxed_slice();
            compiled.molang_ops = vec![
                MolangOp::LoadQuery(1),
                MolangOp::LoadQuery(1),
                MolangOp::StoreVariable(2),
                MolangOp::LoadVariable(2),
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
            compiled.render.layers[0].color = Some([1; 4]);
            compiled.animation_keyframes[1].expressions = [Some(2), None, None];
        },
    );
    let tick = store.actor_rig(1).unwrap().completed_tick;
    for alpha in [0.25, 0.5, 1.0, 0.5] {
        let amount = (2.0 + alpha) / crate::actor_store::creeper::SWELL_FULL_TICKS;
        let layers = store.render_frame(alpha).layers(1).unwrap();
        for endpoint in [&layers[1].previous_pose[1], &layers[1].pose[1]] {
            assert!(
                (endpoint.translation_scale[0] + amount).abs() < 1e-6,
                "render writes reach selected geometry channels"
            );
        }
    }
    assert_eq!(store.actor_rig(1).unwrap().completed_tick, tick);
}

#[test]
fn swell_controller_effects_stay_at_their_authored_reference_when_an_earlier_gate_activates() {
    let store = swell_controller_fixture_with(3, 0.12, false, |compiled| {
        let scalar = |value| assets::EntityGeometryScalar::new(value).unwrap();
        let mut symbols = compiled.molang_symbols.to_vec();
        symbols.push(assets::MolangSymbol {
            kind: assets::MolangSymbolKind::Variable,
            identifier: "variable.factor".into(),
        });
        compiled.molang_symbols = symbols.into();
        let mut ops = compiled.molang_ops.to_vec();
        ops.extend([
            MolangOp::Push(scalar(0.0)),
            MolangOp::Push(scalar(1.0)),
            MolangOp::StoreVariable(5),
            MolangOp::Push(scalar(0.0)),
            MolangOp::Push(scalar(3.0)),
            MolangOp::StoreVariable(5),
            MolangOp::Push(scalar(0.0)),
            MolangOp::LoadQuery(4),
            MolangOp::Push(scalar(0.08)),
            MolangOp::Greater,
            MolangOp::LoadVariable(5),
        ]);
        compiled.molang_ops = ops.into();
        let mut expressions = compiled.molang_expressions.to_vec();
        expressions.extend(
            [(7, 1, 1), (8, 3, 1), (11, 3, 1), (14, 3, 2), (17, 1, 1)].map(
                |(first_op, op_count, max_stack)| assets::CompiledMolangExpression {
                    first_op,
                    op_count,
                    max_stack,
                },
            ),
        );
        compiled.molang_expressions = expressions.into();
        compiled.rig_bindings[0].pre_animation = Some(4);
        compiled.controller_states[1].on_entry = Some(5);
        for animation in &mut compiled.controller_animations {
            animation.weight = Some(3);
        }
        compiled.rig_controllers = vec![
            assets::EntityRigControllerBinding {
                name: 0,
                controller: 0,
                weight: Some(6),
                order: 0,
            },
            assets::EntityRigControllerBinding {
                name: 1,
                controller: 0,
                weight: None,
                order: 2,
            },
        ]
        .into();
        compiled.rig_animations = vec![assets::EntityRigAnimationBinding {
            name: 2,
            clip: 0,
            weight: Some(7),
            order: 1,
        }]
        .into();
        compiled.rig_geometries[0].controller_count = 2;
        compiled.rig_geometries[0].animation_count = 1;
    });
    for alpha in [0.5, 0.75, 1.0] {
        let layers = store.render_frame(alpha).layers(1).unwrap();
        let expected = -(2.0 + alpha) / crate::actor_store::creeper::SWELL_FULL_TICKS;
        for pose in [&layers[0].previous_pose[0], &layers[0].pose[0]] {
            assert!(
                (pose.translation_scale[0] - expected).abs() < 1e-6,
                "a newly selected reference cannot consume a later reference's completed event"
            );
        }
    }
}

#[test]
fn swell_controller_entry_effects_survive_reference_deactivation() {
    assert_controller_entry_effects_survive_deactivation(false);
}

#[test]
fn swell_controller_entry_effects_survive_nested_reference_deactivation() {
    assert_controller_entry_effects_survive_deactivation(true);
}

fn assert_controller_entry_effects_survive_deactivation(nested: bool) {
    let store = swell_controller_fixture_with(3, 0.12, false, |compiled| {
        let scalar = |value| assets::EntityGeometryScalar::new(value).unwrap();
        let mut symbols = compiled.molang_symbols.to_vec();
        symbols.push(assets::MolangSymbol {
            kind: assets::MolangSymbolKind::Variable,
            identifier: "variable.factor".into(),
        });
        compiled.molang_symbols = symbols.into();
        let mut ops = compiled.molang_ops.to_vec();
        ops.extend([
            MolangOp::Push(scalar(0.0)),
            MolangOp::Push(scalar(1.0)),
            MolangOp::StoreVariable(5),
            MolangOp::Push(scalar(0.0)),
            MolangOp::Push(scalar(3.0)),
            MolangOp::StoreVariable(5),
            MolangOp::Push(scalar(0.0)),
            MolangOp::LoadQuery(4),
            MolangOp::Push(scalar(0.08)),
            MolangOp::Less,
            MolangOp::LoadVariable(5),
        ]);
        compiled.molang_ops = ops.into();
        let mut expressions = compiled.molang_expressions.to_vec();
        expressions.extend(
            [(7, 1, 1), (8, 3, 1), (11, 3, 1), (14, 3, 2), (17, 1, 1)].map(
                |(first_op, op_count, max_stack)| assets::CompiledMolangExpression {
                    first_op,
                    op_count,
                    max_stack,
                },
            ),
        );
        compiled.molang_expressions = expressions.into();
        compiled.rig_bindings[0].pre_animation = Some(4);
        compiled.controller_states[1].on_entry = Some(5);
        for animation in &mut compiled.controller_animations {
            animation.weight = Some(3);
        }
        compiled.rig_controllers = vec![
            assets::EntityRigControllerBinding {
                name: 0,
                controller: 0,
                weight: Some(6),
                order: 0,
            },
            assets::EntityRigControllerBinding {
                name: 1,
                controller: 0,
                weight: None,
                order: 2,
            },
        ]
        .into();
        compiled.rig_animations = vec![assets::EntityRigAnimationBinding {
            name: 2,
            clip: 0,
            weight: Some(7),
            order: 1,
        }]
        .into();
        compiled.rig_geometries[0].controller_count = 2;
        compiled.rig_geometries[0].animation_count = 1;
        if nested {
            let mut symbols = compiled.symbols.to_vec();
            symbols.insert(
                4,
                assets::EntityAssetSymbol {
                    kind: assets::EntityAssetKind::AnimationController,
                    identifier: "controller.animation.wrapper".into(),
                    source_index: 0,
                    dependencies: Box::new([]),
                },
            );
            compiled.symbols = symbols.into();
            compiled.rig_bindings[0].render_controller += 1;
            let mut controllers = compiled.controllers.to_vec();
            controllers.push(assets::EntityAnimationController {
                symbol: 4,
                first_state: 2,
                state_count: 1,
                initial_state: 0,
            });
            compiled.controllers = controllers.into();
            let mut states = compiled.controller_states.to_vec();
            states.push(assets::EntityControllerState {
                name: 0,
                first_animation: 2,
                animation_count: 1,
                first_transition: 1,
                ..Default::default()
            });
            compiled.controller_states = states.into();
            let mut animations = compiled.controller_animations.to_vec();
            animations.push(assets::EntityControllerAnimation {
                target: assets::EntityControllerAnimationTarget::Controller(0),
                weight: Some(6),
            });
            compiled.controller_animations = animations.into();
            compiled.rig_controllers[0].controller = 1;
            compiled.rig_controllers[0].weight = None;
        }
    });
    for alpha in [0.5, 0.75, 1.0] {
        let layers = store.render_frame(alpha).layers(1).unwrap();
        let expected = -(2.0 + alpha) / crate::actor_store::creeper::SWELL_FULL_TICKS;
        for (pose, factor) in [
            (&layers[0].previous_pose[0], 1.0),
            (&layers[0].pose[0], 3.0),
        ] {
            assert!(
                (pose.translation_scale[0] - factor * expected).abs() < 1e-6,
                "completed entry effects remain part of the ordinary endpoint when its reference stops drawing"
            );
        }
    }
}

#[test]
fn swell_frame_alpha_weights_activate_between_ticks() {
    for variable in [false, true] {
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
                    (assets::MolangSymbolKind::Query, "query.frame_alpha"),
                    (assets::MolangSymbolKind::Query, "query.swell_amount"),
                    (assets::MolangSymbolKind::Variable, "variable.weight"),
                ]
                .map(|(kind, identifier)| assets::MolangSymbol {
                    kind,
                    identifier: identifier.into(),
                })
                .into();
                compiled.molang_ops = vec![
                    MolangOp::LoadQuery(2),
                    MolangOp::LoadQuery(1),
                    MolangOp::StoreVariable(3),
                    MolangOp::Push(assets::EntityGeometryScalar::new(0.0).unwrap()),
                    MolangOp::LoadVariable(3),
                    MolangOp::LoadQuery(1),
                ]
                .into();
                compiled.molang_expressions = [(0, 1), (1, 3), (4, 1), (5, 1)]
                    .map(|(first_op, op_count)| assets::CompiledMolangExpression {
                        first_op,
                        op_count,
                        max_stack: 1,
                    })
                    .into();
                compiled.rig_bindings[0].pre_animation = variable.then_some(1);
                compiled.rig_animations[0].weight = Some(if variable { 2 } else { 3 });
            },
        );
        for alpha in [0.25, 0.75, 1.0, 0.25] {
            let layers = store.render_frame(alpha).layers(1).unwrap();
            let expected = -(2.0 + alpha) / crate::actor_store::creeper::SWELL_FULL_TICKS * alpha;
            for pose in [&layers[0].previous_pose[0], &layers[0].pose[0]] {
                assert!(
                    (pose.translation_scale[0] - expected).abs() < 1e-6,
                    "fresh presentation weight activates the swell clip"
                );
            }
        }
    }
}
