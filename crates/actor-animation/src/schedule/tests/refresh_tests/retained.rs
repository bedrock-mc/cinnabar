//! Stateful controller and geometry inputs retain one original local evaluation per tick.
use super::*;
use assets::*;
use protocol_ids::ActorFlag;

/// Adds a terminal controller transition whose entry script increments the retained counter.
fn controller(compiled: &mut CompiledEntityAssets) {
    let mut sources = compiled.sources.to_vec();
    sources.insert(
        0,
        EntityAssetSource {
            path: "animation_controllers/counter.json".into(),
            source_bytes: 1,
            source_sha256: [1; 32],
        },
    );
    compiled.sources = sources.into();
    for geometry in &mut compiled.geometries {
        geometry.source_index += 1;
    }
    for clip in &mut compiled.animation_clips {
        clip.source += 1;
    }
    for candidate in &mut compiled.render.candidates {
        candidate.source += 1;
    }
    let mut symbols = compiled.symbols.to_vec();
    for symbol in &mut symbols {
        symbol.source_index += 1;
    }
    symbols.insert(
        4,
        EntityAssetSymbol {
            mount_equipment: assets::entity::mount::MountEquipment::Unresolved,
            kind: EntityAssetKind::AnimationController,
            identifier: "controller.animation.counter".into(),
            source_index: 0,
            dependencies: Box::new([]),
        },
    );
    compiled.symbols = symbols.into();
    *compiled.rig_bindings[0].render_controller.as_mut().unwrap() += 1;
    let mut names = compiled.molang_symbols.to_vec();
    names.insert(
        1,
        MolangSymbol {
            kind: MolangSymbolKind::Name,
            identifier: "zzz".into(),
        },
    );
    compiled.molang_symbols = names.into();
    for op in &mut compiled.molang_ops {
        match op {
            MolangOp::LoadQuery(index)
            | MolangOp::LoadVariable(index)
            | MolangOp::StoreVariable(index)
                if *index >= 1 =>
            {
                *index += 1
            }
            _ => {}
        }
    }
    let mut expressions = compiled.molang_expressions.to_vec();
    let mut ops = compiled.molang_ops.to_vec();
    let condition = expressions.len() as u32;
    expressions.push(CompiledMolangExpression {
        first_op: ops.len() as u32,
        op_count: 3,
        max_stack: 2,
    });
    ops.extend([
        MolangOp::LoadVariable(3),
        MolangOp::Push(EntityGeometryScalar::new(0.0).unwrap()),
        MolangOp::Greater,
    ]);
    compiled.molang_ops = ops.into();
    compiled.molang_expressions = expressions.into();
    compiled.controllers = vec![EntityAnimationController {
        symbol: 4,
        first_state: 0,
        state_count: 2,
        initial_state: 0,
    }]
    .into();
    compiled.controller_states = vec![
        EntityControllerState {
            name: 0,
            first_transition: 0,
            transition_count: 1,
            ..Default::default()
        },
        EntityControllerState {
            name: 1,
            first_transition: 1,
            on_entry: compiled.rig_bindings[0].pre_animation,
            ..Default::default()
        },
    ]
    .into();
    compiled.controller_transitions = vec![EntityControllerTransition {
        target_state: 1,
        condition,
    }]
    .into();
    compiled.rig_geometries[0].controller_count = 1;
    compiled.rig_controllers = vec![EntityRigControllerBinding {
        name: 0,
        controller: 0,
        weight: None,
        order: 1,
    }]
    .into();
}

/// Adds a candidate that would incorrectly become eligible after committing the script twice.
fn geometry(compiled: &mut CompiledEntityAssets) {
    let mut expressions = compiled.molang_expressions.to_vec();
    let mut ops = compiled.molang_ops.to_vec();
    let condition = expressions.len() as u32;
    expressions.push(CompiledMolangExpression {
        first_op: ops.len() as u32,
        op_count: 3,
        max_stack: 2,
    });
    ops.extend([
        MolangOp::LoadVariable(4),
        MolangOp::Push(EntityGeometryScalar::new(2.0).unwrap()),
        MolangOp::Greater,
    ]);
    compiled.molang_ops = ops.into();
    compiled.molang_expressions = expressions.into();
    let mut animations = compiled.rig_animations.to_vec();
    animations.push(animations[0]);
    compiled.rig_animations = animations.into();
    let mut geometries = compiled.rig_geometries.to_vec();
    geometries.push(EntityRigGeometryBinding {
        geometry: 1,
        condition: Some(condition),
        first_animation: 1,
        ..geometries[0]
    });
    compiled.rig_geometries = geometries.into();
    compiled.rig_bindings[0].geometry_count = 2;
    compiled.animation_clips[0].geometry = None;
}

#[test]
fn local_refresh_replaces_controller_transition_and_entry_script_once() {
    let (actors, mut store) = local_fixture_with(true, true, controller);
    refresh_swing(&actors, &mut store);
    let count = store
        .layout
        .named_slot(store.assets.as_ref().unwrap(), "variable.refresh_count");
    for _ in 0..2 {
        refresh_again(&actors, &mut store);
        let state = store.rigs.values().next().unwrap();
        assert_eq!(state.controllers[0].state, 1);
        assert_eq!(state.variables.get(count), Some(4.0));
        assert_eq!(state.current[0].translation_scale[2], 4.0);
        assert_eq!(state.ui_pose.as_ref().unwrap()[0].translation_scale[2], 4.0);
    }
}

#[test]
fn local_refresh_selects_geometry_from_original_retained_script_inputs() {
    let (actors, mut store) = local_fixture_with(true, true, geometry);
    assert_eq!(store.rigs.values().next().unwrap().geometry_binding, 0);
    refresh_swing(&actors, &mut store);
    for _ in 0..2 {
        refresh_again(&actors, &mut store);
        let state = store.rigs.values().next().unwrap();
        assert_eq!(state.geometry_binding, 0);
        assert_eq!(state.current[0].translation_scale[2], 3.0);
        assert_eq!(state.ui_pose.as_ref().unwrap()[0].translation_scale[2], 3.0);
    }
}

/// Selects the second geometry from a newly observed actor flag rather than an accumulator.
fn changing_geometry(compiled: &mut CompiledEntityAssets) {
    geometry(compiled);
    let mut symbols = compiled.molang_symbols.to_vec();
    symbols.insert(
        2,
        MolangSymbol {
            kind: MolangSymbolKind::Query,
            identifier: "query.is_baby".into(),
        },
    );
    compiled.molang_symbols = symbols.into();
    for op in &mut compiled.molang_ops {
        match op {
            MolangOp::LoadQuery(index)
            | MolangOp::LoadVariable(index)
            | MolangOp::StoreVariable(index)
                if *index >= 2 =>
            {
                *index += 1
            }
            _ => {}
        }
    }
    let expression = compiled.rig_geometries[1].condition.unwrap() as usize;
    let first = compiled.molang_expressions[expression].first_op as usize;
    compiled.molang_ops[first] = MolangOp::LoadQuery(2);
    compiled.molang_ops[first + 1] = MolangOp::Push(EntityGeometryScalar::new(0.0).unwrap());
}

#[test]
fn local_refresh_changed_geometry_seeds_ui_from_original_authored_inputs() {
    let (mut actors, mut store) = local_fixture_with(true, true, changing_geometry);
    actors.get_mut(&LOCAL).unwrap().metadata.insert(
        0,
        protocol::ActorMetadataValue::Flags(1 << ActorFlag::Baby.wire_value()),
    );
    refresh_swing(&actors, &mut store);
    for _ in 0..2 {
        let state = store.rigs.values().next().unwrap();
        assert_eq!(state.geometry_binding, 1);
        assert_eq!(state.current[0].translation_scale[2], 3.0);
        assert_eq!(state.ui_pose.as_ref().unwrap()[0].translation_scale[2], 3.0);
        refresh_again(&actors, &mut store);
    }
}

/// Returning local rigs retain the same controller, geometry, HUD and replay effects as serial.
#[test]
fn returning_local_rig_keeps_stateful_refresh_and_geometry_effects_serial() {
    for configure in [controller, geometry] {
        let (actors, mut serial) = local_fixture_with(true, true, configure);
        let (_, mut parallel) = local_fixture_with(true, true, configure);
        serial.schedule.serial = true;
        let skin = crate::test_support::skin_source(crate::test_support::SKIN_MODEL);
        let away = ActorAnimationView {
            planes: [[-1.0, 0.0, 0.0, -100.0]; 6],
            camera: [0.0; 3],
            player_distance: 100.0,
            entity_radius: 72.0,
        };
        for tick in 0..10 {
            for store in [&mut serial, &mut parallel] {
                let context = |_: &TestActor| ActorTickContext {
                    is_local: true,
                    is_local_first_person: tick % 2 == 0,
                    animation_elapsed_ticks: Some(if tick == 6 { 4 } else { 1 }),
                    skin_geometry: (tick >= 6).then(|| Arc::clone(&skin)),
                    ..Default::default()
                };
                let hidden = (2..6).contains(&tick);
                store.advance_tick(
                    &actors,
                    hidden.then_some(&away),
                    (!hidden).then_some(1),
                    true,
                    true,
                    context,
                );
                if !hidden {
                    store.refresh_local_view(&actors, 1, context);
                }
            }
            assert_eq!(
                crate::schedule::tests::fingerprint(&parallel),
                crate::schedule::tests::fingerprint(&serial),
                "tick {tick}"
            );
        }
    }
}
