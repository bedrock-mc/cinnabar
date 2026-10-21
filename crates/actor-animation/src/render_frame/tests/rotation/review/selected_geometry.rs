use super::*;

mod swing;

/// Builds a swelling rig whose selected geometry alone reads the camera.
fn swelling_geometry(active: bool) -> assets::CompiledEntityAssets {
    let mut compiled = compiled_fixture(180.0);
    compiled.symbols[0].identifier = "minecraft:creeper".into();
    let camera = symbol(
        &mut compiled,
        MolangSymbolKind::Query,
        "query.camera_rotation",
    );
    let camera = expression(
        &mut compiled,
        &[
            MolangOp::Push(crate::test_support::scalar(0.0)),
            MolangOp::CallQuery(MolangCall {
                symbol: camera,
                arguments: 1,
            }),
        ],
        1,
    );
    let swell = symbol(&mut compiled, MolangSymbolKind::Query, "query.swell_amount");
    let swell = expression(&mut compiled, &[MolangOp::LoadQuery(swell)], 1);
    compiled.animation_keyframes[0].expressions = [Some(swell), None, None];
    let mut geometries = compiled.geometries.to_vec();
    let mut alternate = geometries[0].clone();
    alternate.identifier = "geometry.zz_alternate".into();
    geometries.push(alternate.clone());
    compiled.geometries = geometries.into();
    let mut symbols = compiled.symbols.to_vec();
    symbols.insert(
        2,
        EntityAssetSymbol {
            kind: EntityAssetKind::Geometry,
            identifier: alternate.identifier,
            source_index: alternate.source_index,
            dependencies: Box::new([]),
            mount_equipment: assets::entity::mount::MountEquipment::Unresolved,
        },
    );
    compiled.symbols = symbols.into();
    *compiled.rig_bindings[0].render_controller.as_mut().unwrap() += 1;
    compiled.animation_clips[0].symbol += 1;
    let mut keys = compiled.animation_keyframes.to_vec();
    let mut key = keys[0];
    key.expressions = [Some(swell), Some(camera), None];
    let first_keyframe = keys.len() as u32;
    keys.push(key);
    compiled.animation_keyframes = keys.into();
    let mut channels = compiled.animation_channels.to_vec();
    let mut channel = channels[0].clone();
    channel.first_keyframe = first_keyframe;
    let first_channel = channels.len() as u32;
    channels.push(channel);
    compiled.animation_channels = channels.into();
    let mut clips = compiled.animation_clips.to_vec();
    clips.push(assets::EntityAnimationClip {
        first_channel,
        channel_count: 1,
        geometry: Some(1),
        ..clips[0]
    });
    compiled.animation_clips = clips.into();
    let enabled = expression(
        &mut compiled,
        &[MolangOp::Push(crate::test_support::scalar(f32::from(
            active,
        )))],
        1,
    );
    compiled.render.geometries = vec![assets::EntityRenderGeometry {
        geometry: 1,
        condition: Some(enabled),
    }]
    .into();
    compiled.render.layers[0].geometry_count = 1;
    compiled
}

#[test]
fn selected_geometry_camera_inputs_follow_the_frame_during_swelling() {
    for active in [false, true] {
        let assets = Arc::new(
            RuntimeEntityAssets::from_compiled(
                swelling_geometry(active).with_unresolved_admissions(),
            )
            .unwrap(),
        );
        let mut actor = TestActor {
            kind: protocol::ActorKind::Entity {
                identifier: "minecraft:creeper".into(),
            },
            ..Default::default()
        };
        let mut store = ActorAnimationStore::with_assets(assets);
        store.insert(1, 0, &actor);
        let mut actors = HashMap::from([(1, actor.clone())]);
        store.advance_tick(&actors, None, None, true, true, |_| Default::default());
        actor.status.creeper_swell = [0.0, 1.0];
        actors.insert(1, actor.clone());
        store.advance_tick(&actors, None, None, true, true, |_| Default::default());
        let rig = store.get(1).unwrap();
        let previous = rig.previous[0].rotation;
        let current = rig.current[0].rotation;
        assert_ne!(previous, current);
        let committed = authored_state(&store);
        for alpha in [0.25, 0.75] {
            let mut budget = MAX_MOLANG_OPS_PER_RENDER_FRAME;
            let sampled = store
                .render_layers(&actor, alpha, [35.0, 10.0], [0.0; 3], &mut budget, false)
                .unwrap();
            let layer = &sampled.render[0];
            assert_eq!(layer.geometry, active.then_some(1));
            assert!(
                (layer.pose[0].translation_scale[1] - if active { 35.0 } else { 0.0 }).abs() < 1e-4
            );
            if !active {
                assert_eq!(layer.previous_pose[0].rotation, previous);
                assert_eq!(layer.pose[0].rotation, current);
            }
            assert_eq!(authored_state(&store), committed);
        }
    }
}
