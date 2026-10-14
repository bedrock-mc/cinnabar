use super::*;

/// Selects an Always-depth layer only between ticks and samples scale from head yaw.
fn admission_fixture() -> CompiledEntityAssets {
    let mut compiled = compiled_fixture(180.0);
    let frame_alpha = compiled
        .molang_symbols
        .iter()
        .position(|symbol| symbol.identifier.as_ref() == "query.frame_alpha")
        .unwrap() as u32;
    let visible = expression(&mut compiled, &[MolangOp::LoadQuery(frame_alpha)], 1);
    let mut layers = compiled.render.layers.to_vec();
    let mut layer = layers[0];
    layer.condition = Some(visible);
    layer.material_state = Some(assets::EntityRenderMaterialState {
        depth_always: true,
        ..Default::default()
    });
    let mut slots = compiled.render.slots.to_vec();
    let mut slot = slots[0];
    let mut candidates = compiled.render.candidates.to_vec();
    slot.first_candidate = candidates.len() as u32;
    candidates.push(candidates[0]);
    layer.first_slot = slots.len() as u32;
    slots.push(slot);
    layers.push(layer);
    compiled.render.layers = layers.into();
    compiled.render.slots = slots.into();
    compiled.render.candidates = candidates.into();
    let head = compiled
        .molang_symbols
        .iter()
        .position(|symbol| symbol.identifier.as_ref() == "query.head_y_rotation")
        .unwrap() as u32;
    let scale = expression(
        &mut compiled,
        &[
            MolangOp::Push(crate::test_support::scalar(180.0)),
            MolangOp::CallQuery(MolangCall {
                symbol: head,
                arguments: 1,
            }),
            MolangOp::Push(crate::test_support::scalar(0.01)),
            MolangOp::Multiply,
            MolangOp::Push(crate::test_support::scalar(1.0)),
            MolangOp::Add,
        ],
        2,
    );
    let unit = expression(
        &mut compiled,
        &[MolangOp::Push(crate::test_support::scalar(1.0))],
        1,
    );
    compiled.rig_bindings[0].scale_expressions = Some([scale, unit, unit, unit]);
    compiled
}

#[test]
fn admission_body_and_draw_share_material_scale_and_persona_after_budget_exhaustion() {
    for scale_admission in [false, true] {
        for persona in [false, true] {
            let context = ActorTickContext {
                skin_geometry: persona.then(persona_source),
                ..Default::default()
            };
            let (mut store, mut actors) = fixture_with(admission_fixture(), 0.0, context.clone());
            actors.get_mut(&1).unwrap().head_yaw = 90.0;
            store.advance_tick(&actors, None, None, true, true, |_| context.clone());
            let rig = store.get(1).unwrap();
            assert!(
                !rig.render
                    .iter()
                    .any(|layer| { layer.material_state.is_some_and(|state| state.depth_always) })
            );
            let expected_scale = 1.0
                + (actors[&1].head_yaw * 0.5 - (rig.previous_body_yaw + rig.body_yaw) * 0.5) * 0.01;
            let authored = authored_state(&store);
            let mut frame = ActorRenderFrame::new(&store, &actors, [0.0; 2], [0.0; 3], 0.5);
            if scale_admission {
                let admitted = frame.sample_rig_scale(rig);
                assert!((admitted.scale - expected_scale).abs() < 1e-5);
                assert_ne!(admitted.scale, rig.scale);
            } else {
                assert!(frame.has_always_depth_material(1));
            }
            let after_admission = frame.remaining_ops();
            let first_body = frame.layers_with_skin(1).unwrap();
            assert_eq!(frame.remaining_ops(), after_admission);
            frame.set_remaining_ops(0);
            let body = frame.layers_with_skin(1).unwrap();
            assert_eq!(body.render, first_body.render);
            assert!(
                body.render
                    .iter()
                    .any(|layer| { layer.material_state.is_some_and(|state| state.depth_always) })
            );
            let scale = body.render[0].sampled_scale.unwrap();
            assert!((scale[0] - expected_scale).abs() < 1e-5);
            if persona {
                assert_eq!(body.skin.len(), 1);
                assert!(Arc::ptr_eq(
                    &body.skin[0].previous,
                    &first_body.skin[0].previous
                ));
                assert!(Arc::ptr_eq(
                    &body.skin[0].current,
                    &first_body.skin[0].current
                ));
                assert_ne!(body.skin[0].current, rig.skin_layers[0].current);
            }
            let draw = frame.layers(1).unwrap();
            assert_eq!(draw, body.render);
            assert!(frame.has_always_depth_material(1));
            assert_eq!(frame.sample_rig_scale(rig).scale, scale[0]);
            assert_eq!(frame.layers_with_skin(1).unwrap().render, draw);
            assert_eq!(frame.remaining_ops(), 0);
            assert_eq!(authored_state(&store), authored);
        }
    }
}

/// Supplies valid persona skeletons whose combined endpoint work exceeds the frame limit.
fn expensive_persona() -> Arc<render_api::SkinGeometrySource> {
    let mut source = persona_source().as_ref().clone();
    let extra = (1..assets::MAX_SKIN_GEOMETRY_BONES)
        .map(|index| format!(r#"{{"name":"extra_{index}","pivot":[0,0,0]}}"#))
        .collect::<Vec<_>>()
        .join(",");
    source.geometry_data = source
        .geometry_data
        .replace(
            r#""bones":[{"name":"rightitem","pivot":[0,2,0]"#,
            &format!(r#""bones":[{extra},{{"name":"rightitem","pivot":[0,2,0]"#),
        )
        .into();
    let count = MAX_RUNTIME_POSE_WORK_PER_ACTOR_FRAME / (assets::MAX_SKIN_GEOMETRY_BONES * 2) + 1;
    source.animations = vec![source.animations[0].clone(); count].into();
    Arc::new(source)
}

#[test]
fn persona_work_exhaustion_preserves_successful_body_and_admission_layers() {
    let context = ActorTickContext {
        skin_geometry: Some(expensive_persona()),
        ..Default::default()
    };
    let (mut store, mut actors) = fixture_with(admission_fixture(), 0.0, context.clone());
    actors.get_mut(&1).unwrap().head_yaw = 90.0;
    store.advance_tick(&actors, None, None, true, true, |_| context.clone());
    let rig = store.get(1).unwrap();
    assert!(!rig.skin_layers.is_empty());
    assert_eq!(
        rig.skin_layers[0].current.len(),
        assets::MAX_SKIN_GEOMETRY_BONES
    );
    let mut frame = ActorRenderFrame::new(&store, &actors, [0.0; 2], [0.0; 3], 0.5);
    assert!(frame.has_always_depth_material(1));
    frame.set_remaining_ops(0);
    let body = frame.layers_with_skin(1).unwrap();
    assert!(matches!(body.render, Cow::Owned(_)));
    assert!(matches!(body.skin, Cow::Borrowed(_)));
    assert_ne!(body.render[0].pose[0], rig.current[0]);
    assert!(Arc::ptr_eq(
        &body.skin[0].current,
        &rig.skin_layers[0].current
    ));
    assert!(
        frame
            .layers(1)
            .unwrap()
            .iter()
            .any(|layer| { layer.material_state.is_some_and(|state| state.depth_always) })
    );
}
