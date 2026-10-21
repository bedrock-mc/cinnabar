use super::*;

/// Keeps one small rig while adding named channels belonging to unrelated catalog content.
fn catalog(unrelated: usize) -> Arc<RuntimeEntityAssets> {
    let mut compiled = crate::test_support::counting_random_compiled("minecraft:test");
    compiled.animation_channels[0].bone_name = Some("rightitem".into());
    let seed = compiled.animation_channels[0].clone();
    let keys = compiled.animation_keyframes.to_vec();
    let mut channels = compiled.animation_channels.into_vec();
    channels.extend((0..unrelated).map(|index| EntityAnimationChannel {
        bone_name: Some(format!("unrelated_{index}").into()),
        first_keyframe: ((index + 1) * keys.len()) as u32,
        ..seed.clone()
    }));
    compiled.animation_channels = channels.into();
    compiled.animation_keyframes = keys.repeat(unrelated + 1).into();
    if unrelated > 0 {
        let mut geometry = compiled.geometries[0].clone();
        geometry.identifier = format!("{}.unrelated", geometry.identifier).into();
        let mut symbols = compiled.symbols.into_vec();
        symbols.insert(
            2,
            EntityAssetSymbol {
                kind: EntityAssetKind::Geometry,
                identifier: geometry.identifier.clone(),
                source_index: geometry.source_index,
                dependencies: Box::new([]),
                mount_equipment: assets::entity::mount::MountEquipment::Unresolved,
            },
        );
        compiled.symbols = symbols.into();
        *compiled.rig_bindings[0].render_controller.as_mut().unwrap() += 1;
        compiled.animation_clips[0].symbol += 1;
        compiled.geometries = vec![compiled.geometries[0].clone(), geometry].into();
        compiled.animation_clips = vec![
            compiled.animation_clips[0],
            EntityAnimationClip {
                geometry: Some(1),
                first_channel: 1,
                channel_count: unrelated as u32,
                ..compiled.animation_clips[0]
            },
        ]
        .into();
    }
    compiled.render.layers[0].first_geometry = 0;
    compiled.render.layers[0].geometry_count = 1;
    compiled.render.geometries = vec![EntityRenderGeometry {
        geometry: 0,
        condition: None,
    }]
    .into();
    Arc::new(RuntimeEntityAssets::from_compiled(compiled.with_unresolved_admissions()).unwrap())
}

#[test]
fn binding_payload_is_independent_of_unrelated_catalog_channels() {
    let payload = |unrelated| {
        let assets = catalog(unrelated);
        let layout = VariableLayout::new(&assets);
        layout
            .rig_plan(&assets, 0, 0)
            .unwrap()
            .channels
            .retained_bytes()
    };
    assert_eq!(payload(180_000), payload(0));
}

#[test]
fn binding_build_work_is_independent_of_unrelated_catalog_channels() {
    let work = |unrelated| {
        let assets = catalog(unrelated);
        let layout = VariableLayout::new(&assets);
        let before = pose::binding_build_work();
        let plan = layout.rig_plan(&assets, 0, 0).unwrap();
        let work = pose::binding_build_work() - before;
        assert!(work <= plan.names.len() * 2, "visited {work} records");
        work
    };
    assert_eq!(work(180_000), work(0));
}

#[test]
fn actors_share_sparse_bindings_in_a_large_catalog() {
    let assets = catalog(180_000);
    let mut store = ActorAnimationStore::with_assets(assets);
    for runtime_id in [1, 2] {
        store.insert(
            1,
            0,
            &TestActor {
                runtime_id,
                ..Default::default()
            },
        );
    }
    let states: Vec<_> = store.rigs.values().collect();
    assert_eq!(states.len(), 2);
    assert!(Arc::ptr_eq(
        &states[0].channel_bindings,
        &states[1].channel_bindings
    ));
}

#[test]
fn alternate_geometry_bindings_are_shared_after_owner_root_binding() {
    let assets = catalog(0);
    let mut store = ActorAnimationStore::with_assets(assets);
    let actors: HashMap<_, _> = [1, 2]
        .map(|runtime_id| {
            (
                runtime_id,
                TestActor {
                    runtime_id,
                    ..Default::default()
                },
            )
        })
        .into();
    for actor in actors.values() {
        store.insert(1, 0, actor);
    }
    store.advance_tick(&actors, None, None, true, false, |_| {
        ActorTickContext::default()
    });
    for state in store.rigs.values_mut() {
        render::bind_attachable_roots(state, &["rightitem".into()]);
    }
    let states: Vec<_> = store.rigs.values().collect();
    let first = states[0].layer_skeletons[&0].as_ref().unwrap();
    let second = states[1].layer_skeletons[&0].as_ref().unwrap();
    assert!(first.channels.shares_data(&second.channels));
}

#[test]
fn unbound_named_channels_keep_the_reference_lookup() {
    let assets = catalog(0);
    let names = vec!["rightitem".into(), "socket".into(), "rightitem".into()];
    let empty = pose::ChannelBindings::default();
    let mut resolver = pose::ChannelBones::bound(&names, &empty);
    let channel = &assets.animation_channels()[0];
    let before = pose::name_lookups();
    assert_eq!(resolver.resolve(channel), Some(0));
    assert!(
        pose::name_lookups() > before,
        "must exercise the lossless fallback"
    );
    let prepared = pose::ChannelBindings::new(&names);
    let mut bound = pose::ChannelBones::bound(&names, &prepared);
    let mut reference = pose::ChannelBones::new(&names);
    let mut channels = vec![channel.clone(); 4];
    channels[1].bone_name = None;
    channels[1].bone = 99;
    channels[2].bone_name = Some("absent".into());
    channels[3].bone_name = Some("RIGHTITEM".into());
    for channel in &channels {
        assert_eq!(bound.resolve(channel), reference.resolve(channel));
    }
}
