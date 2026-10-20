use super::*;

/// Supplies conditional named clips and an owner locator on an original two-bone rig.
fn named_catalog(reverse: bool) -> Arc<RuntimeEntityAssets> {
    let mut compiled = crate::test_support::counting_random_compiled(
        assets::facts::VanillaEntity::Player.identifier(),
    );
    compiled.molang_symbols[4].identifier = "query.modified_distance_moved".into();
    compiled.molang_ops[11] = MolangOp::LoadQuery(4);
    compiled.animation_channels[0].bone_name = Some("rightitem".into());
    compiled.animation_keyframes[0].expressions = [Some(3), None, None];
    compiled.rig_animations[0].weight = Some(3);
    let mut socket = compiled.geometries[0].bones[0].clone();
    socket.name = "socket".into();
    socket.parent = Some("rightitem".into());
    socket.locators = vec![EntityGeometryLocator {
        name: "emitter".into(),
        offset: [crate::test_support::scalar(1.0); 3],
        rotation: [crate::test_support::scalar(0.0); 3],
        ignore_inherited_scale: false,
    }]
    .into();
    let mut bones = compiled.geometries[0].bones.to_vec();
    bones.push(socket);
    if reverse {
        bones.reverse();
    }
    compiled.geometries[0].bones = bones.into();
    Arc::new(RuntimeEntityAssets::from_compiled(compiled.with_unresolved_admissions()).unwrap())
}

/// Retains dependency/effect state while selecting the pre-optimization channel and mask paths.
fn use_reference(store: &mut ActorAnimationStore) {
    for state in store.rigs.values_mut() {
        state.channel_bindings = Arc::new(pose::ChannelBindings::reference());
        state.motion_sampling = state
            .motion_sampling
            .as_ref()
            .map(|sampling| sampling.reference_masks());
    }
}

#[test]
fn named_frames_preserve_selection_skin_and_owner_locator_outputs_without_lookups() {
    let actor = TestActor {
        kind: ActorKind::Player {
            uuid: [0; 16],
            username: "fixture".into(),
        },
        ..Default::default()
    };
    let mut actors = HashMap::from([(actor.runtime_id, actor.clone())]);
    for reverse in [false, true] {
        let assets = named_catalog(reverse);
        let mut bound = ActorAnimationStore::with_assets(Arc::clone(&assets));
        let mut reference = ActorAnimationStore::with_assets(Arc::clone(&assets));
        for store in [&mut bound, &mut reference] {
            store.insert(1, 0, &actor);
            for tick in 0..3 {
                let moving = actors.get_mut(&actor.runtime_id).unwrap();
                moving.position[0] = tick as f32 * 0.2;
                moving.velocity[0] = 4.0;
                store.advance_tick(&actors, None, None, true, false, |_| {
                    ActorTickContext::default()
                });
            }
        }
        let changed = crate::test_support::SKIN_MODEL.replace("\"body\"", "\"rightitem\"");
        for (appearance, model) in [
            None,
            Some(crate::test_support::SKIN_MODEL),
            Some(changed.as_str()),
        ]
        .into_iter()
        .enumerate()
        {
            let source = model.map(crate::test_support::skin_source);
            for store in [&mut bound, &mut reference] {
                if let Some(source) = &source {
                    store.request_skin_preparation(source);
                    store.submit_skin_preparation();
                    store.finish_skin_fixture_batch();
                    store.sync_skin_model(actor.runtime_id, Some(source));
                }
            }
            // The reset tick seeds equal endpoints; another moved tick supplies an interval.
            for _ in 0..2 {
                let moving = actors.get_mut(&actor.runtime_id).unwrap();
                moving.previous_position = moving.position;
                moving.position[0] += 0.2;
                for store in [&mut bound, &mut reference] {
                    store.advance_tick(&actors, None, None, true, false, |_| ActorTickContext {
                        skin_geometry: source.clone(),
                        ..Default::default()
                    });
                }
            }
            let actor = &actors[&actor.runtime_id];
            let mut reference_scans = 0;
            use_reference(&mut reference);
            for alpha in [0.0, 0.25, 0.5, 0.75, 1.0, 0.25] {
                let before = pose::name_lookups();
                let mut budget = MAX_MOLANG_OPS_PER_RENDER_FRAME;
                let actual = bound
                    .render_layers(actor, alpha, [0.0; 2], [0.0; 3], &mut budget, true)
                    .unwrap();
                assert_eq!(
                    pose::name_lookups(),
                    before,
                    "bound frame sampling searched bone names"
                );
                let mut budget = MAX_MOLANG_OPS_PER_RENDER_FRAME;
                let reference_before = pose::name_lookups();
                let expected = reference
                    .render_layers(actor, alpha, [0.0; 2], [0.0; 3], &mut budget, true)
                    .unwrap();
                reference_scans += pose::name_lookups() - reference_before;
                assert_eq!(actual.render, expected.render);
                assert_eq!(actual.skin.len(), expected.skin.len());
                for (actual, expected) in actual.skin.iter().zip(expected.skin.iter()) {
                    assert_eq!(actual.previous, expected.previous);
                    assert_eq!(actual.current, expected.current);
                    assert_eq!(actual.hidden_bones, expected.hidden_bones);
                }
                if model.is_none() {
                    let locator = assets.geometry_locator(0, "emitter").unwrap();
                    let sample = |store: &ActorAnimationStore, layers: &[RenderTextureLayer]| {
                        let rig = store.get(actor.runtime_id).unwrap();
                        let bones = if layers[0].pose.is_empty() {
                            rig.current
                        } else {
                            &layers[0].pose
                        };
                        let bone = bones[locator.bone_index as usize];
                        let bone = render_model::RenderBoneTransform::from_model_space_scaled(
                            bone.rotation,
                            bone.translation_scale,
                            bone.axis_scale,
                        )
                        .unwrap();
                        render_model::sampled_locator_transform(
                            &locator.locator,
                            locator.pivot,
                            bone,
                            bone,
                            [
                                [1.0, 0.0, 0.0, 0.0],
                                [0.0, 1.0, 0.0, 0.0],
                                [0.0, 0.0, 1.0, 0.0],
                            ],
                            alpha,
                        )
                    };
                    assert_eq!(
                        sample(&bound, &actual.render),
                        sample(&reference, &expected.render)
                    );
                }
                assert_eq!(
                    bound.get(actor.runtime_id).unwrap().bone_names,
                    reference.get(actor.runtime_id).unwrap().bone_names
                );
            }
            assert_eq!(bound.frame_fallbacks(), 0);
            assert_eq!(reference.frame_fallbacks(), 0);
            assert!(
                reference_scans > 0,
                "appearance {appearance}, reverse={reverse} must exercise named frame sampling"
            );
        }
    }
}

#[test]
fn catalog_replacement_rebinds_named_channels_in_the_existing_plan_owner() {
    let original = named_catalog(false);
    let replacement = named_catalog(true);
    let layout = VariableLayout::new(&original);
    let old = layout.rig_plan(&original, 0, 0).unwrap();
    let new = layout.rig_plan(&replacement, 0, 0).unwrap();
    let mut old_bones = pose::ChannelBones::bound(&old.names, &old.channels);
    let mut new_bones = pose::ChannelBones::bound(&new.names, &new.channels);
    assert_ne!(old.names, new.names);
    assert_eq!(
        old_bones.resolve(&original.animation_channels()[0]),
        Some(0)
    );
    assert_eq!(
        new_bones.resolve(&replacement.animation_channels()[0]),
        Some(1)
    );
    assert!(!Arc::ptr_eq(&old, &new));
}

#[test]
fn hot_reloaded_pack_replaces_channel_and_attachment_plan_indices() {
    let vanilla = named_catalog(false);
    let actor = TestActor {
        kind: ActorKind::Player {
            uuid: [0; 16],
            username: "fixture".into(),
        },
        ..Default::default()
    };
    let actors = HashMap::from([(actor.runtime_id, actor.clone())]);
    let mut store = ActorAnimationStore::with_assets(vanilla);
    for reverse in [false, true, false] {
        store.set_pack(Some((named_catalog(reverse), vec![0])));
        store.insert(1, 0, &actor);
        store.advance_tick(&actors, None, None, true, false, |_| {
            ActorTickContext::default()
        });
        let rig = store.get(actor.runtime_id).unwrap();
        assert_eq!(
            rig.bone_names[0].as_ref(),
            if reverse { "socket" } else { "rightitem" }
        );
        let before = pose::name_lookups();
        let mut budget = MAX_MOLANG_OPS_PER_RENDER_FRAME;
        let actual = store
            .render_layers(&actor, 0.5, [0.0; 2], [0.0; 3], &mut budget, true)
            .unwrap()
            .render
            .into_owned();
        assert_eq!(pose::name_lookups(), before);
        use_reference(&mut store);
        let mut budget = MAX_MOLANG_OPS_PER_RENDER_FRAME;
        let expected = store
            .render_layers(&actor, 0.5, [0.0; 2], [0.0; 3], &mut budget, true)
            .unwrap();
        assert_eq!(actual, expected.render.as_ref());
        assert_eq!(store.frame_fallbacks(), 0);
    }
}
