use super::*;
use assets::*;

/// Builds a static actor whose material selector reads one query, directly or through a script.
fn material_assets(query: MolangQuery, threshold: f32, indirect: bool) -> Arc<RuntimeEntityAssets> {
    let mut compiled = crate::test_support::counting_random_compiled("minecraft:test");
    compiled.rig_bindings[0].pre_animation = None;
    compiled.render.layers[0].color = None;
    compiled.animation_keyframes[0].expressions = [None; 3];
    compiled.molang_symbols = [
        (MolangSymbolKind::Name, "wield"),
        (MolangSymbolKind::Query, query.descriptor().name),
        (MolangSymbolKind::Variable, "variable.material_choice"),
    ]
    .map(|(kind, identifier)| MolangSymbol {
        kind,
        identifier: identifier.into(),
    })
    .into();
    compiled.molang_ops = vec![
        MolangOp::LoadQuery(1),
        MolangOp::Push(crate::test_support::scalar(threshold)),
        MolangOp::Less,
    ]
    .into();
    compiled.molang_expressions = vec![CompiledMolangExpression {
        first_op: 0,
        op_count: 3,
        max_stack: 2,
    }]
    .into();
    if indirect {
        let mut ops = compiled.molang_ops.into_vec();
        ops.extend([
            MolangOp::StoreVariable(2),
            MolangOp::Push(crate::test_support::scalar(0.0)),
            MolangOp::LoadVariable(2),
        ]);
        compiled.molang_ops = ops.into();
        compiled.molang_expressions = vec![
            CompiledMolangExpression {
                first_op: 0,
                op_count: 5,
                max_stack: 2,
            },
            CompiledMolangExpression {
                first_op: 5,
                op_count: 1,
                max_stack: 1,
            },
        ]
        .into();
        compiled.rig_bindings[0].pre_animation = Some(0);
    }
    compiled.render.material_selections = vec![
        EntityRenderMaterialSelection::Branch {
            condition: u32::from(indirect),
            yes: 1,
            no: 2,
        },
        EntityRenderMaterialSelection::Raster {
            material: EntityRenderMaterial::Default,
            state: Some(EntityRenderMaterialState::default()),
        },
        EntityRenderMaterialSelection::Raster {
            material: EntityRenderMaterial::Default,
            state: Some(EntityRenderMaterialState {
                blend: true,
                ..Default::default()
            }),
        },
    ]
    .into();
    compiled.render.layers[0].material_selection = Some(0);
    Arc::new(RuntimeEntityAssets::from_compiled(compiled.with_unresolved_admissions()).unwrap())
}

#[test]
fn camera_only_material_selector_changes_between_ticks() {
    let actor = crate::tests::actor_with_metadata(HashMap::new());
    let actors = HashMap::from([(actor.runtime_id, actor.clone())]);
    let mut store = ActorAnimationStore::with_assets(material_assets(
        MolangQuery::DistanceFromCamera,
        8.0,
        false,
    ));
    store.insert(1, 0, &actor);
    store.advance_tick(&actors, None, None, true, true, |_| ActorTickContext {
        camera_position: [0.0, 0.0, 5.0],
        ..Default::default()
    });
    let completed = store.get(1).unwrap().render.to_vec();
    assert!(!completed[0].material_state.unwrap().blend);
    let authored = authored_state(&store);
    for alpha in [0.0, 0.25, 0.75, 1.0] {
        for (distance, blend) in [(5.0, false), (9.0, true), (5.0, false)] {
            let mut budget = MAX_MOLANG_OPS_PER_RENDER_FRAME;
            let layers = store
                .render_layers(
                    &actor,
                    alpha,
                    [0.0; 2],
                    [0.0, 0.0, distance],
                    &mut budget,
                    false,
                )
                .unwrap();
            assert_eq!(layers.render.len(), 1);
            assert_eq!(layers.render[0].material_state.unwrap().blend, blend);
        }
    }
    assert_eq!(store.get(1).unwrap().render, completed);
    assert_eq!(authored_state(&store), authored);
}

#[test]
fn frame_time_material_selectors_follow_authored_variable_dependencies() {
    for (query, threshold) in [
        (MolangQuery::FrameAlpha, 0.5),
        (MolangQuery::LifeTime, 0.075),
    ] {
        let actor = crate::tests::actor_with_metadata(HashMap::new());
        let actors = HashMap::from([(actor.runtime_id, actor.clone())]);
        let mut store = ActorAnimationStore::with_assets(material_assets(query, threshold, true));
        store.insert(1, 0, &actor);
        store.advance_tick(&actors, None, None, true, true, |_| Default::default());
        let authored = authored_state(&store);
        for (alpha, blend) in [(0.25, false), (0.75, true), (0.25, false)] {
            let mut budget = MAX_MOLANG_OPS_PER_RENDER_FRAME;
            let layers = store
                .render_layers(&actor, alpha, [0.0; 2], [0.0; 3], &mut budget, false)
                .unwrap();
            assert_eq!(
                layers.render[0].material_state.unwrap().blend,
                blend,
                "{query:?}"
            );
        }
        assert_eq!(authored_state(&store), authored);
    }
}
