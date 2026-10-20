use super::*;
use assets::*;

mod pool;

/// Independent players whose frame programs read walk, swing, variables and random draws.
pub(super) fn crowd(count: u64, padding: usize) -> (ActorAnimationStore, HashMap<u64, TestActor>) {
    crowd_with_channel_name(count, padding, None)
}

/// Builds the same crowd with either numeric channels or a named skeleton binding.
pub(super) fn crowd_with_channel_name(
    count: u64,
    padding: usize,
    bone_name: Option<&str>,
) -> (ActorAnimationStore, HashMap<u64, TestActor>) {
    let mut compiled = crate::test_support::counting_random_compiled(
        assets::facts::VanillaEntity::Player.identifier(),
    );
    compiled.animation_channels[0].bone_name = bone_name.map(Into::into);
    compiled.molang_symbols[4].identifier = "query.modified_distance_moved".into();
    compiled.molang_symbols[7].identifier = "variable.attack_time".into();
    let mut ops = compiled.molang_ops.to_vec();
    let walk = ops.len() as u32;
    ops.push(MolangOp::LoadQuery(4));
    let mut expressions = compiled.molang_expressions.to_vec();
    expressions.push(CompiledMolangExpression {
        first_op: walk,
        op_count: 1,
        max_stack: 1,
    });
    if padding > 0 {
        // Extend the pre-animation program without changing its stack or side effects.
        ops.splice(9..9, std::iter::repeat_n(MolangOp::Negate, padding));
        for expression in &mut expressions[1..] {
            expression.first_op += padding as u32;
        }
        expressions[0].op_count += u16::try_from(padding).unwrap();
    }
    compiled.molang_expressions = expressions.into();
    compiled.molang_ops = ops.into();
    compiled.animation_keyframes[0].expressions = [Some(4), Some(1), Some(2)];
    if padding == 0 {
        add_controller(&mut compiled);
    }
    let assets = Arc::new(
        RuntimeEntityAssets::from_compiled(compiled.with_unresolved_admissions()).unwrap(),
    );
    let mut store = ActorAnimationStore::with_assets(assets);
    store.schedule.serial = true;
    store.schedule.world_budget = usize::MAX / 4;
    let mut actors: HashMap<_, _> = (1..=count)
        .map(|id| {
            (
                id,
                TestActor {
                    runtime_id: id,
                    unique_id: id as i64,
                    kind: ActorKind::Player {
                        uuid: (id as u128).to_le_bytes(),
                        username: "fixture".into(),
                    },
                    ..Default::default()
                },
            )
        })
        .collect();
    for actor in actors.values() {
        store.insert(1, 0, actor);
    }
    for tick in 0..3 {
        for actor in actors.values_mut() {
            actor.previous_position = actor.position;
            actor.position[0] += 0.1 + actor.runtime_id as f32 * 0.001;
            actor.velocity[0] = 2.0;
            if tick == 0 {
                store.start_swing(actor.runtime_id, 6, 6);
            }
        }
        store.advance_tick(&actors, None, None, true, false, |_| ActorTickContext {
            frame_alpha: if tick == 1 { 1.0 } else { 0.0 },
            ..Default::default()
        });
    }
    for &id in actors.keys() {
        assert!(store.get(id).unwrap().pose_evaluated);
    }
    if padding == 0 {
        assert!(
            store
                .rigs
                .values()
                .all(|state| state.controllers[0].state == 1)
        );
    }
    (store, actors)
}

/// Captures all public layer values and exact pose/scalar float bits in request order.
pub(super) fn output(frame: &mut ActorRenderFrame<'_>, ids: &[u64]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for &id in ids {
        let sample = frame.layers_with_skin(id);
        let Some(sample) = sample else {
            bytes.push(0);
            continue;
        };
        bytes.push(1);
        bytes.extend_from_slice(format!("{:?}{:?}", sample.render, sample.skin).as_bytes());
        bytes.extend_from_slice(
            &sample
                .held_item_scale
                .map(f32::to_bits)
                .unwrap_or(u32::MAX)
                .to_le_bytes(),
        );
        for layer in sample.render.iter() {
            for value in layer
                .color
                .into_iter()
                .chain(layer.overlay)
                .chain(layer.uv_anim)
                .chain([layer.light_color_multiplier])
                .chain(layer.sampled_scale.into_iter().flatten())
            {
                bytes.extend_from_slice(&value.to_bits().to_le_bytes());
            }
            for bone in layer.previous_pose.iter().chain(layer.pose.iter()) {
                for value in bone
                    .rotation
                    .into_iter()
                    .chain(bone.translation_scale)
                    .chain(bone.axis_scale)
                {
                    bytes.extend_from_slice(&value.to_bits().to_le_bytes());
                }
            }
        }
    }
    bytes
}

/// Compares independent histories so warmed caches cannot hide different budget consumption.
fn compare(
    serial: &ActorAnimationStore,
    parallel: &ActorAnimationStore,
    actors: &HashMap<u64, TestActor>,
    ids: &[u64],
    alpha: f32,
    budget: usize,
) {
    let before = (serial.frame_fallbacks(), parallel.frame_fallbacks());
    let mut expected =
        ActorRenderFrame::new(serial, actors, [12.0, -179.0], [1.0, 2.0, 3.0], alpha);
    let mut actual =
        ActorRenderFrame::new(parallel, actors, [12.0, -179.0], [1.0, 2.0, 3.0], alpha);
    expected.set_remaining_ops(budget);
    actual.set_remaining_ops(budget);
    let expected_bytes = output(&mut expected, ids);
    actual.sample_ordered(ids);
    let actual_bytes = output(&mut actual, ids);
    assert!(
        actual_bytes == expected_bytes,
        "different output at byte {:?}; alpha={alpha}, budget={budget}",
        actual_bytes
            .iter()
            .zip(&expected_bytes)
            .position(|(a, b)| a != b)
    );
    assert_eq!(actual.remaining_ops(), expected.remaining_ops());
    let remaining = actual.remaining_ops();
    actual.sample_ordered(ids);
    assert_eq!(output(&mut actual, ids), expected_bytes);
    assert_eq!(
        actual.remaining_ops(),
        remaining,
        "repeated consumers spend no budget"
    );
    assert_eq!(
        serial.frame_fallbacks() - before.0,
        parallel.frame_fallbacks() - before.1
    );
}

/// Reorders a crowd without a dependency on a shared random stream.
fn shuffle(ids: &mut [u64], seed: &mut u64) {
    for index in (1..ids.len()).rev() {
        *seed ^= *seed << 13;
        *seed ^= *seed >> 7;
        *seed ^= *seed << 17;
        ids.swap(index, *seed as usize % (index + 1));
    }
}

#[test]
fn crowds_match_serial_bytes_with_one_two_and_four_workers() {
    for workers in [1, 2, 4] {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(workers)
            .build()
            .unwrap();
        pool.install(|| {
            for count in [1, 128, 512] {
                let (serial, actors) = crowd(count, 0);
                let (parallel, _) = crowd(count, 0);
                let committed = authored_state(&serial);
                let mut ids: Vec<_> = (1..=count).collect();
                let mut seed = 0x7138_1234_9876;
                for alpha in [0.0, 0.25, 0.25, 0.5, 0.75, 1.0, 0.75, 0.0] {
                    shuffle(&mut ids, &mut seed);
                    compare(&serial, &parallel, &actors, &ids, alpha, usize::MAX / 4);
                }
                assert_eq!(authored_state(&serial), committed);
                assert_eq!(authored_state(&parallel), committed);
                assert_eq!(serial.frame_fallbacks(), 0);
                assert_eq!(parallel.frame_fallbacks(), 0);
            }
        });
    }
}

#[test]
fn every_expression_budget_boundary_and_parallel_reservation_matches_serial() {
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(4)
        .build()
        .unwrap();
    pool.install(|| {
        let (serial, actors) = crowd(16, 0);
        let (parallel, _) = crowd(16, 0);
        let ids: Vec<_> = (1..=16).collect();
        let (measure_store, _) = crowd(16, 0);
        let mut measure = ActorRenderFrame::new(&measure_store, &actors, [0.0; 2], [0.0; 3], 0.37);
        measure.set_remaining_ops(usize::MAX / 4);
        output(&mut measure, &ids);
        let total = usize::MAX / 4 - measure.remaining_ops();
        assert!(total > 0);
        for budget in 0..=total + 1 {
            compare(
                &serial,
                &parallel,
                &actors,
                &(1..=16).collect::<Vec<_>>(),
                0.37,
                budget,
            );
        }
        for boundary in [MIN_PARALLEL_SAMPLES, MIN_PARALLEL_SAMPLES + 1, 16] {
            for delta in [-1isize, 0, 1] {
                let budget = (boundary * MAX_MOLANG_OPS_PER_ACTOR_FRAME)
                    .checked_add_signed(delta)
                    .unwrap();
                let (serial, actors) = crowd(64, 2000);
                let (parallel, _) = crowd(64, 2000);
                compare(
                    &serial,
                    &parallel,
                    &actors,
                    &(1..=64).rev().collect::<Vec<_>>(),
                    0.37,
                    budget,
                );
                assert!(
                    serial.frame_fallbacks() > 0,
                    "must reach the serial budget tail"
                );
            }
        }
    });
}

#[test]
fn reset_removal_respawn_duplicates_and_missing_actors_match_serial() {
    let (mut serial, mut actors) = crowd(32, 0);
    let (mut parallel, _) = crowd(32, 0);
    let mut ids: Vec<_> = (1..=32).rev().collect();
    ids.extend([1, 4, 999, 1]);
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(4)
        .build()
        .unwrap();
    for stage in 0..5 {
        for store in [&mut serial, &mut parallel] {
            match stage {
                0 => store.mark_reset(3),
                1 => store.remove_runtime(4),
                2 => {
                    store.insert(1, 0, &actors[&4]);
                }
                3 => store.clear(),
                _ => {
                    for actor in actors.values() {
                        store.insert(1, 0, actor);
                    }
                }
            }
        }
        if stage == 1 {
            actors.get_mut(&4).unwrap().spawn_revision += 1;
        }
        if stage == 2 || stage == 4 {
            for store in [&mut serial, &mut parallel] {
                store.advance_tick(&actors, None, None, true, false, |_| {
                    ActorTickContext::default()
                });
            }
        }
        pool.install(|| {
            compare(
                &serial,
                &parallel,
                &actors,
                &ids,
                0.75,
                MAX_MOLANG_OPS_PER_RENDER_FRAME,
            )
        });
    }
}

/// Adds independent controller transitions, blend history and effectful entry scripts.
fn add_controller(compiled: &mut CompiledEntityAssets) {
    let mut sources = compiled.sources.to_vec();
    sources.insert(
        0,
        EntityAssetSource {
            path: "animation_controllers/fixture.json".into(),
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
    let symbol = symbols
        .iter()
        .position(|s| s.kind == EntityAssetKind::RenderController)
        .unwrap() as u32;
    symbols.insert(
        symbol as usize,
        EntityAssetSymbol {
            mount_equipment: assets::entity::mount::MountEquipment::Unresolved,
            kind: EntityAssetKind::AnimationController,
            identifier: "controller.animation.fixture".into(),
            source_index: 0,
            dependencies: Box::new([]),
        },
    );
    compiled.symbols = symbols.into();
    *compiled.rig_bindings[0].render_controller.as_mut().unwrap() += 1;
    compiled.controllers = vec![EntityAnimationController {
        symbol,
        first_state: 0,
        state_count: 2,
        initial_state: 0,
    }]
    .into();
    compiled.controller_states = (0..2)
        .map(|state| EntityControllerState {
            name: 0,
            first_animation: state,
            animation_count: 1,
            first_transition: state,
            transition_count: 1,
            on_entry: Some(0),
            on_exit: Some(0),
            blend_transition: EntityGeometryScalar::new(0.2).unwrap(),
            blend_via_shortest_path: true,
        })
        .collect();
    compiled.controller_animations = vec![
        EntityControllerAnimation {
            target: EntityControllerAnimationTarget::Clip(0),
            weight: Some(2),
        };
        2
    ]
    .into();
    let mut expressions = compiled.molang_expressions.to_vec();
    let never = expressions.len() as u32;
    let mut ops = compiled.molang_ops.to_vec();
    expressions.push(CompiledMolangExpression {
        first_op: ops.len() as u32,
        op_count: 1,
        max_stack: 1,
    });
    ops.push(MolangOp::Push(EntityGeometryScalar::new(0.0).unwrap()));
    compiled.molang_ops = ops.into();
    compiled.molang_expressions = expressions.into();
    compiled.controller_transitions = (0..2)
        .map(|state| EntityControllerTransition {
            target_state: 1 - state,
            condition: if state == 0 { 3 } else { never },
        })
        .collect();
    compiled.rig_animations = Box::new([]);
    compiled.rig_geometries[0].animation_count = 0;
    compiled.rig_geometries[0].controller_count = 1;
    compiled.rig_controllers = vec![EntityRigControllerBinding {
        name: 0,
        controller: 0,
        weight: None,
        order: 0,
    }]
    .into();
}

/// Captures committed variables, RNG, controller clocks and poses without frame scratch caches.
fn authored_state(store: &ActorAnimationStore) -> String {
    format!(
        "{:?}",
        store
            .rigs
            .values()
            .map(|state| (
                &state.variables,
                &state.controllers,
                &state.clip_clocks,
                &state.current,
            ))
            .collect::<Vec<_>>()
    )
}
