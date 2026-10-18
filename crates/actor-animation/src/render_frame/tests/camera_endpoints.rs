use super::*;
use assets::*;

/// A non-player rig whose only frame input is the camera: a controller weight reads the camera
/// pitch when `controller`, otherwise the clip reads the camera distance.
fn camera_rig(controller: bool) -> Arc<RuntimeEntityAssets> {
    let scalar = crate::test_support::scalar;
    let mut compiled = crate::test_support::counting_random_compiled("minecraft:test");
    compiled.rig_bindings[0].initialize = None;
    compiled.rig_bindings[0].pre_animation = None;
    compiled.render.layers[0].color = None;
    compiled.molang_symbols = [
        (MolangSymbolKind::Name, "state"),
        (MolangSymbolKind::Name, "wield"),
        (MolangSymbolKind::Query, "query.camera_rotation"),
        (MolangSymbolKind::Query, "query.distance_from_camera"),
    ]
    .map(|(kind, identifier)| MolangSymbol {
        kind,
        identifier: identifier.into(),
    })
    .into();
    compiled.molang_ops = vec![
        MolangOp::Push(scalar(0.0)),
        MolangOp::CallQuery(MolangCall {
            symbol: 2,
            arguments: 1,
        }),
        MolangOp::Push(scalar(0.02)),
        MolangOp::Multiply,
        MolangOp::Push(scalar(90.0)),
        MolangOp::LoadQuery(3),
        MolangOp::Push(scalar(10.0)),
        MolangOp::Multiply,
    ]
    .into();
    compiled.molang_expressions = [(0, 4, 2), (4, 1, 1), (5, 3, 2)]
        .map(|(first_op, op_count, max_stack)| CompiledMolangExpression {
            first_op,
            op_count,
            max_stack,
        })
        .into();
    compiled.animation_channels[0].property = EntityAnimationProperty::Rotation;
    compiled.animation_keyframes[0].expressions =
        [Some(if controller { 1 } else { 2 }), None, None];
    compiled.rig_animations[0].name = 1;
    if controller {
        let mut sources = compiled.sources.to_vec();
        sources.insert(
            0,
            EntityAssetSource {
                path: "animation_controllers/camera.json".into(),
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
            3,
            EntityAssetSymbol {
                mount_equipment: assets::entity::mount::MountEquipment::Unresolved,
                kind: EntityAssetKind::AnimationController,
                identifier: "controller.animation.camera".into(),
                source_index: 0,
                dependencies: Box::new([]),
            },
        );
        compiled.symbols = symbols.into();
        *compiled.rig_bindings[0].render_controller.as_mut().unwrap() += 1;
        compiled.controllers = vec![EntityAnimationController {
            symbol: 3,
            first_state: 0,
            state_count: 1,
            initial_state: 0,
        }]
        .into();
        compiled.controller_states = vec![EntityControllerState {
            name: 0,
            first_animation: 0,
            animation_count: 1,
            ..Default::default()
        }]
        .into();
        compiled.controller_animations = vec![EntityControllerAnimation {
            target: EntityControllerAnimationTarget::Clip(0),
            weight: Some(0),
        }]
        .into();
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
    Arc::new(RuntimeEntityAssets::from_compiled(compiled.with_unresolved_admissions()).unwrap())
}

/// The drawn rotation of the first bone after `ticks` at `tick_camera`, sampled at `camera`.
fn drawn_rotation(
    assets: &Arc<RuntimeEntityAssets>,
    tick_camera: ([f32; 2], [f32; 3]),
    camera: ([f32; 2], [f32; 3]),
    alpha: f32,
) -> [f32; 4] {
    let actor = crate::tests::actor_with_metadata(HashMap::new());
    let actors = HashMap::from([(actor.runtime_id, actor.clone())]);
    let mut store = ActorAnimationStore::with_assets(Arc::clone(assets));
    store.insert(1, 0, &actor);
    for _ in 0..3 {
        store.advance_tick(&actors, None, None, true, true, |_| ActorTickContext {
            camera_rotation: tick_camera.0,
            camera_position: tick_camera.1,
            ..Default::default()
        });
    }
    let rig = store.get(actor.runtime_id).unwrap();
    let mut budget = MAX_MOLANG_OPS_PER_RENDER_FRAME;
    let layers = store
        .render_layers(&actor, alpha, camera.0, camera.1, &mut budget, false)
        .unwrap();
    let (previous, current) = match &layers.render {
        Cow::Owned(layers) => (layers[0].previous_pose[0], layers[0].pose[0]),
        Cow::Borrowed(_) => (rig.previous[0], rig.current[0]),
    };
    std::array::from_fn(|axis| {
        previous.rotation[axis] + (current.rotation[axis] - previous.rotation[axis]) * alpha
    })
}

/// A camera move between ticks draws what a tick at the new camera would have drawn.
fn assert_live_camera(assets: Arc<RuntimeEntityAssets>, label: &str) {
    let tick = ([10.0, 0.0], [0.0, 0.0, 5.0]);
    let live = ([30.0, 0.0], [0.0, 0.0, 9.0]);
    for alpha in [0.25, 0.75] {
        let expected = drawn_rotation(&assets, live, live, alpha);
        let stale = drawn_rotation(&assets, tick, tick, alpha);
        assert_ne!(expected, stale, "{label}: the camera must change the pose");
        let actual = drawn_rotation(&assets, tick, live, alpha);
        for (actual, expected) in actual.into_iter().zip(expected) {
            assert!(
                (actual - expected).abs() < 1.0e-5,
                "{label} at {alpha}: {actual} vs {expected}"
            );
        }
    }
}

#[test]
fn camera_controller_weights_resample_with_the_live_camera() {
    assert_live_camera(camera_rig(true), "controller weight");
}

#[test]
fn camera_distance_queries_resample_with_the_live_camera() {
    assert_live_camera(camera_rig(false), "camera distance");
}
