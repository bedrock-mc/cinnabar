use super::*;

fn compile_swell_stop(source: &str, version: i32) -> Option<assets::MolangProgram> {
    assert_eq!(source, "variable.factor = 3; return 0;");
    assert_eq!(version, 0);
    let scalar = |value| assets::EntityGeometryScalar::new(value).unwrap();
    Some(assets::MolangProgram {
        symbols: vec![assets::MolangSymbol {
            kind: assets::MolangSymbolKind::Variable,
            identifier: "variable.factor".into(),
        }]
        .into(),
        expressions: vec![assets::CompiledMolangExpression {
            first_op: 0,
            op_count: 3,
            max_stack: 1,
        }]
        .into(),
        ops: vec![
            MolangOp::Push(scalar(3.0)),
            MolangOp::StoreVariable(0),
            MolangOp::Push(scalar(0.0)),
        ]
        .into(),
        collections: Box::new([]),
        collection_items: Box::new([]),
    })
}

#[test]
fn swell_replays_server_stop_assignments_at_the_selection_stage() {
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
            let mut symbols = compiled.molang_symbols.to_vec();
            symbols.push(assets::MolangSymbol {
                kind: assets::MolangSymbolKind::Variable,
                identifier: "variable.factor".into(),
            });
            compiled.molang_symbols = symbols.into();
            compiled.molang_ops = vec![
                MolangOp::LoadQuery(1),
                MolangOp::LoadVariable(2),
                MolangOp::Multiply,
                MolangOp::Push(scalar(1.0)),
                MolangOp::StoreVariable(2),
                MolangOp::Push(scalar(0.0)),
            ]
            .into();
            compiled.molang_expressions = [(0, 3, 2), (3, 3, 1)]
                .map(
                    |(first_op, op_count, max_stack)| assets::CompiledMolangExpression {
                        first_op,
                        op_count,
                        max_stack,
                    },
                )
                .into();
            compiled.rig_bindings[0].pre_animation = Some(1);
        },
    );
    store.set_server_animation_compiler(compile_swell_stop);
    store.apply_item_actor(
        1,
        2,
        protocol::ItemActorEvent::Action(protocol::ActorActionEvent {
            actor_runtime_ids: Arc::from([1]),
            kind: protocol::ActorActionKind::Custom {
                animation: "animation.item".into(),
                controller: "fixture.server".into(),
                next_state: "".into(),
                stop_expression: "variable.factor = 3; return 0;".into(),
                stop_expression_version: 0,
            },
            data: 0.0,
            swing_source: None,
        }),
    );
    store.advance_interpolation_ticks(1);
    let completed = store.actor_rig(1).unwrap().current[0].translation_scale[0];
    assert!(
        (completed + 18.0 / crate::actor_store::creeper::SWELL_FULL_TICKS).abs() < 1e-6,
        "server stop assignment reaches the tick pose"
    );
    for alpha in [0.25, 0.75, 1.0, 0.25] {
        let layers = store.render_frame(alpha).layers(1).unwrap();
        let amount = (3.0 + alpha) / crate::actor_store::creeper::SWELL_FULL_TICKS;
        assert!(
            (layers[0].pose[0].translation_scale[0] + 6.0 * amount).abs() < 1e-6,
            "completed server-selection writes reach the sampled pose"
        );
    }
    assert_eq!(
        store.actor_rig(1).unwrap().current[0].translation_scale[0],
        completed
    );
}

#[test]
fn swell_server_selected_clip_extends_the_authored_dependency_graph() {
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
            let mut symbols = compiled.symbols.to_vec();
            symbols.insert(
                3,
                assets::EntityAssetSymbol {
                    kind: assets::EntityAssetKind::Animation,
                    identifier: "animation.server".into(),
                    source_index: 0,
                    dependencies: Box::new([]),
                },
            );
            compiled.symbols = symbols.into();
            compiled.rig_bindings[0].render_controller = 4;
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
            compiled.molang_ops = vec![MolangOp::LoadQuery(1), MolangOp::LoadQuery(2)].into();
            compiled.molang_expressions = [0, 1]
                .map(|first_op| assets::CompiledMolangExpression {
                    first_op,
                    op_count: 1,
                    max_stack: 1,
                })
                .into();
            compiled.animation_keyframes[0].expressions = [Some(0), None, None];
            let mut key = compiled.animation_keyframes[0];
            key.expressions = [None, Some(1), None];
            compiled.animation_keyframes = vec![compiled.animation_keyframes[0], key].into();
            let mut channel = compiled.animation_channels[0].clone();
            channel.first_keyframe = 1;
            compiled.animation_channels =
                vec![compiled.animation_channels[0].clone(), channel].into();
            let mut clip = compiled.animation_clips[0];
            clip.symbol = 3;
            clip.first_channel = 1;
            compiled.animation_clips = vec![compiled.animation_clips[0], clip].into();
        },
    );
    store.apply_item_actor(
        1,
        2,
        protocol::ItemActorEvent::Action(protocol::ActorActionEvent {
            actor_runtime_ids: Arc::from([1]),
            kind: protocol::ActorActionKind::Custom {
                animation: "animation.server".into(),
                controller: "fixture.server".into(),
                next_state: "".into(),
                stop_expression: "".into(),
                stop_expression_version: 0,
            },
            data: 0.0,
            swing_source: None,
        }),
    );
    store.advance_interpolation_ticks(1);
    let tick = store.actor_rig(1).unwrap().completed_tick;
    for alpha in [0.25, 0.75, 1.0, 0.25] {
        let layers = store.render_frame(alpha).layers(1).unwrap();
        assert!(
            !layers[0].pose.is_empty(),
            "packet-only swell clips have a presentation pose"
        );
        let expected = (3.0 + alpha) / crate::actor_store::creeper::SWELL_FULL_TICKS;
        assert!(
            (layers[0].pose[0].translation_scale[1] - expected).abs() < 1e-6,
            "packet-only swell clips sample the frame query"
        );
    }
    assert_eq!(store.actor_rig(1).unwrap().completed_tick, tick);
}
