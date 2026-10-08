use super::*;

#[test]
fn installed_creeper_samples_pack_swelling_and_flash_between_ticks() {
    assert_installed_creeper_samples(false);
}

#[test]
fn installed_powered_creeper_samples_pack_swelling_and_motion_between_ticks() {
    assert_installed_creeper_samples(true);
}

fn assert_installed_creeper_samples(powered: bool) {
    let root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.local/assets/compiled");
    let entries = match std::fs::read_dir(&root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            eprintln!(
                "skipping creeper animation fixture: {} is absent",
                root.display()
            );
            return;
        }
        Err(error) => panic!("read {}: {error}", root.display()),
    };
    let Some(path) = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| {
            path.extension()
                .is_some_and(|extension| extension == "mcbeent")
        })
    else {
        eprintln!(
            "skipping creeper animation fixture: no entity carrier in {}",
            root.display()
        );
        return;
    };
    let assets = Arc::new(RuntimeEntityAssets::decode(&std::fs::read(path).unwrap()).unwrap());
    let mut store =
        crate::actor_store::ActorStore::new_with_entity_assets(1, 0, Arc::clone(&assets));
    store.apply(
        1,
        1,
        protocol::ActorEvent::Spawn(protocol::ActorSpawnEvent {
            dimension: 0,
            unique_id: 1,
            runtime_id: 1,
            kind: ActorKind::Entity {
                identifier: "minecraft:creeper".into(),
            },
            position: [0.0; 3],
            velocity: [0.0; 3],
            pitch: 0.0,
            yaw: 0.0,
            head_yaw: 0.0,
            body_yaw: 0.0,
            held_item: Default::default(),
            metadata: Arc::from([protocol::ActorMetadata {
                key: 0,
                value: ActorMetadataValue::Flags((1 << 10) | if powered { 1 << 9 } else { 0 }),
            }]),
            attributes: Arc::from([]),
            properties: Arc::from([]),
            links: Arc::from([]),
        }),
    );
    store.advance_interpolation_ticks(2);
    let rig = store.actor_rig(1).unwrap();
    let body = rig
        .bone_names
        .iter()
        .position(|name| name.as_ref() == "body")
        .unwrap();
    let exact = store.render_frame(0.0).layers(1).unwrap();
    let drawn = if exact[0].pose.is_empty() {
        &rig.previous[body]
    } else {
        &exact[0].previous_pose[body]
    };
    assert_eq!(
        drawn.axis_scale, rig.current[body].axis_scale,
        "zero fraction must sample the same swelling as the completed tick"
    );
    let tick = rig.completed_tick;
    let early = store.render_frame(0.25).layers(1).unwrap().into_owned();
    let late = store.render_frame(0.75).layers(1).unwrap().into_owned();
    assert_ne!(
        early[0].pose, late[0].pose,
        "pack swell must sample the frame fraction"
    );
    assert_eq!(early[0].overlay[3], 0.0);
    assert_eq!(late[0].overlay, [1.0, 1.0, 1.0, 0.5]);
    assert_eq!(store.actor_rig(1).unwrap().completed_tick, tick);
    assert_eq!(early, store.render_frame(0.25).layers(1).unwrap().as_ref());
    store.apply(
        1,
        2,
        protocol::ActorEvent::Move(protocol::ActorMoveEvent {
            dimension: 0,
            runtime_id: 1,
            position: [Some(1.0), None, None],
            position_origin: protocol::ActorPositionOrigin::Feet,
            pitch: Some(30.0),
            yaw: None,
            head_yaw: None,
            on_ground: Some(true),
            teleported: false,
            player_mode: None,
            source_tick: None,
            interpolation: Default::default(),
        }),
    );
    store.advance_interpolation_ticks(1);
    let rig = store.actor_rig(1).unwrap();
    let head = rig
        .bone_names
        .iter()
        .position(|name| name.as_ref() == "head")
        .unwrap();
    assert_ne!(rig.previous[head].rotation, rig.current[head].rotation);
    let expected = [rig.previous[head].rotation, rig.current[head].rotation];
    for alpha in [0.25, 0.75] {
        let layers = store.render_frame(alpha).layers(1).unwrap();
        if powered {
            assert!(layers.len() > 1);
            assert_eq!(
                [
                    layers[1].previous_pose[head].rotation,
                    layers[1].pose[head].rotation
                ],
                expected,
                "powered layer must preserve motion"
            );
        }
        assert_eq!(
            [
                layers[0].previous_pose[head].rotation,
                layers[0].pose[head].rotation
            ],
            expected,
            "swelling must retain tick interpolation for head motion"
        );
    }
    let mut actor = store.get(1).unwrap().clone();
    let mut animation = ActorAnimationStore::with_assets(assets);
    animation.insert(1, 0, &actor);
    animation.advance_tick(
        &HashMap::from([(1, actor.clone())]),
        None,
        None,
        true,
        true,
        |_| ActorTickContext::default(),
    );
    actor.pitch = -30.0;
    animation.advance_tick(
        &HashMap::from([(1, actor.clone())]),
        None,
        None,
        true,
        true,
        |_| ActorTickContext::default(),
    );
    assert_ne!(
        animation.get(1).unwrap().previous[head].rotation,
        animation.get(1).unwrap().current[head].rotation
    );
    animation.schedule.world_budget = 0;
    animation.advance_tick(
        &HashMap::from([(1, actor.clone())]),
        None,
        None,
        true,
        true,
        |_| ActorTickContext::default(),
    );
    let held = animation.get(1).unwrap();
    assert_eq!(held.previous[head].rotation, held.current[head].rotation);
    for alpha in [0.0, 0.25, 0.75] {
        let mut remaining = MAX_MOLANG_OPS_PER_RENDER_FRAME;
        let layers = animation
            .render_layers(&actor, alpha, [0.0; 2], [0.0; 3], &mut remaining, false)
            .unwrap();
        for layer in layers.render.iter().filter(|layer| !layer.pose.is_empty()) {
            assert_eq!(
                layer.previous_pose[head].rotation, layer.pose[head].rotation,
                "frozen motion must stay held while swelling is sampled"
            );
        }
    }
    let mut steps = 0;
    while store.get(1).unwrap().creeper_swell_changes() {
        store.advance_interpolation_ticks(1);
        steps += 1;
        assert!(steps < 100, "swelling must reach a stable cap");
    }
    let capped = store.actor_rig(1).unwrap();
    for alpha in [0.0, 0.25, 0.75] {
        let layers = store.render_frame(alpha).layers(1).unwrap();
        let previous = if layers[0].previous_pose.is_empty() {
            &capped.previous[body]
        } else {
            &layers[0].previous_pose[body]
        };
        assert_eq!(
            pose::total_scale(previous),
            pose::total_scale(&capped.current[body]),
            "the first steady fuse tick must hold its capped scale"
        );
        if powered {
            assert_eq!(
                pose::total_scale(&layers[1].previous_pose[body]),
                pose::total_scale(&capped.current[body])
            );
        }
    }
    store.apply(
        1,
        3,
        protocol::ActorEvent::Metadata(protocol::ActorMetadataUpdateEvent {
            dimension: 0,
            runtime_id: 1,
            tick: 0,
            metadata: Arc::from([protocol::ActorMetadata {
                key: 0,
                value: ActorMetadataValue::Flags(if powered { 1 << 9 } else { 0 }),
            }]),
            properties: Arc::from([]),
        }),
    );
    store.advance_interpolation_ticks(1);
    steps = 0;
    while store.get(1).unwrap().creeper_swell_changes() {
        store.advance_interpolation_ticks(1);
        steps += 1;
        assert!(steps < 100, "defusing must reach a stable rest pose");
    }
    let defused = store.actor_rig(1).unwrap();
    for alpha in [0.0, 0.25, 0.75] {
        let layers = store.render_frame(alpha).layers(1).unwrap();
        let previous = if layers[0].previous_pose.is_empty() {
            &defused.previous[body]
        } else {
            &layers[0].previous_pose[body]
        };
        assert_eq!(
            pose::total_scale(previous),
            pose::total_scale(&defused.rest[body]),
            "the first steady defused tick must hold its rest scale"
        );
        if powered {
            assert_eq!(
                pose::total_scale(&layers[1].previous_pose[body]),
                pose::total_scale(&defused.rest[body])
            );
        }
    }
    store.advance_interpolation_ticks(1);
    let completed = store.actor_rig(1).unwrap().render;
    for alpha in [0.25, 0.75] {
        let layers = store.render_frame(alpha).layers(1).unwrap();
        if powered {
            assert!(Arc::ptr_eq(&completed[1].pose, &layers[1].pose));
            assert!(Arc::ptr_eq(
                &completed[1].previous_pose,
                &layers[1].previous_pose
            ));
        }
        assert!(
            layers[0].pose.is_empty() && layers[0].previous_pose.is_empty(),
            "unchanged swelling must reuse tick-owned poses"
        );
    }
    store.apply(
        1,
        4,
        protocol::ActorEvent::Metadata(protocol::ActorMetadataUpdateEvent {
            dimension: 0,
            runtime_id: 1,
            tick: 0,
            metadata: Arc::from([protocol::ActorMetadata {
                key: 0,
                value: ActorMetadataValue::Flags((1 << 10) | if powered { 1 << 9 } else { 0 }),
            }]),
            properties: Arc::from([]),
        }),
    );
    store.advance_interpolation_ticks(10);
    store.apply(
        1,
        5,
        protocol::ActorEvent::Status(protocol::ActorStatusEvent {
            runtime_id: 1,
            kind: protocol::ActorStatusKind::Death,
            data: 0,
        }),
    );
    let dying = store.actor_rig(1).unwrap();
    for alpha in [0.0, 0.25, 0.75] {
        let layers = store.render_frame(alpha).layers(1).unwrap();
        let previous = if layers[0].previous_pose.is_empty() {
            &dying.previous[body]
        } else {
            &layers[0].previous_pose[body]
        };
        assert_eq!(
            pose::total_scale(previous),
            pose::total_scale(&dying.rest[body]),
            "death clears swelling before another animation tick"
        );
        if powered {
            assert_eq!(
                pose::total_scale(&layers[1].previous_pose[body]),
                pose::total_scale(&dying.rest[body])
            );
        }
    }
}

fn compiled_swell_fixture(pre_animation: bool) -> crate::actor_store::ActorStore {
    authored_swell_fixture(
        pre_animation,
        assets::EntityAnimationProperty::Translation,
        false,
    )
}

fn authored_swell_fixture(
    pre_animation: bool,
    property: assets::EntityAnimationProperty,
    variable: bool,
) -> crate::actor_store::ActorStore {
    pack_swell_fixture(pre_animation, property, variable, None, false, 3, false)
}

fn pack_swell_fixture(
    pre_animation: bool,
    property: assets::EntityAnimationProperty,
    variable: bool,
    weighted_query_channel: Option<bool>,
    alternate: bool,
    ticks: u32,
    swell_time: bool,
) -> crate::actor_store::ActorStore {
    pack_swell_fixture_with(
        pre_animation,
        property,
        variable,
        weighted_query_channel,
        alternate,
        ticks,
        swell_time,
        |_| {},
    )
}

fn pack_swell_fixture_with(
    pre_animation: bool,
    property: assets::EntityAnimationProperty,
    variable: bool,
    weighted_query_channel: Option<bool>,
    alternate: bool,
    ticks: u32,
    swell_time: bool,
    edit: impl FnOnce(&mut assets::CompiledEntityAssets),
) -> crate::actor_store::ActorStore {
    let mut compiled = super::attachable::tests::compiled_fixture();
    compiled.sources[1].path = "entity/creeper.json".into();
    compiled.symbols[4].kind = assets::EntityAssetKind::Entity;
    compiled.symbols[4].identifier = "minecraft:creeper".into();
    compiled.symbols.rotate_right(1);
    compiled.rig_bindings[0].entity_symbol = 0;
    compiled.rig_bindings[0].render_controller = 3;
    compiled.rig_bindings[0].pre_animation = pre_animation.then_some(0);
    compiled.animation_clips[0].symbol = 2;
    compiled.molang_symbols = vec![
        assets::MolangSymbol {
            kind: assets::MolangSymbolKind::Name,
            identifier: "wield".into(),
        },
        assets::MolangSymbol {
            kind: assets::MolangSymbolKind::Query,
            identifier: "query.swell_amount".into(),
        },
    ]
    .into_boxed_slice();
    compiled.molang_ops = vec![MolangOp::LoadQuery(1)].into_boxed_slice();
    compiled.molang_expressions = vec![assets::CompiledMolangExpression {
        first_op: 0,
        op_count: 1,
        max_stack: 1,
    }]
    .into_boxed_slice();
    compiled.animation_keyframes[0].expressions = [Some(0), None, None];
    compiled.animation_channels[0].property = property;
    if variable {
        compiled.molang_symbols = [
            (assets::MolangSymbolKind::Name, "wield"),
            (assets::MolangSymbolKind::Query, "query.life_time"),
            (assets::MolangSymbolKind::Query, "query.swell_amount"),
            (assets::MolangSymbolKind::Variable, "variable.swell"),
            (assets::MolangSymbolKind::Variable, "variable.zz_motion"),
        ]
        .into_iter()
        .map(|(kind, identifier)| assets::MolangSymbol {
            kind,
            identifier: identifier.into(),
        })
        .collect::<Vec<_>>()
        .into_boxed_slice();
        compiled.molang_ops = vec![
            MolangOp::LoadQuery(2),
            MolangOp::StoreVariable(3),
            MolangOp::LoadQuery(1),
            MolangOp::StoreVariable(4),
            MolangOp::Push(assets::EntityGeometryScalar::new(0.0).unwrap()),
            MolangOp::LoadVariable(3),
            MolangOp::LoadVariable(4),
        ]
        .into_boxed_slice();
        compiled.molang_expressions = [(0, 5), (5, 1), (6, 1)]
            .into_iter()
            .map(|(first_op, op_count)| assets::CompiledMolangExpression {
                first_op,
                op_count,
                max_stack: 1,
            })
            .collect::<Vec<_>>()
            .into_boxed_slice();
        compiled.rig_bindings[0].pre_animation = Some(0);
        compiled.animation_keyframes[0].expressions = [
            Some(1),
            (property == assets::EntityAnimationProperty::Translation).then_some(2),
            None,
        ];
    }

    if let Some(query_channel) = weighted_query_channel {
        compiled.rig_animations[0].weight = Some(0);
        if !query_channel {
            compiled.animation_keyframes[0].expressions = [None; 3];
            compiled.animation_keyframes[0].value[0] =
                assets::EntityGeometryScalar::new(1.0).unwrap();
        }
    }
    if swell_time {
        let scalar = |value| assets::EntityGeometryScalar::new(value).unwrap();
        compiled.animation_clips[0].anim_time_update = Some(0);
        compiled.animation_clips[0].length_seconds = scalar(1.0);
        let mut key = compiled.animation_keyframes[0];
        key.value = [scalar(0.0); 3];
        key.expressions = [None; 3];
        compiled.animation_keyframes = vec![
            key,
            assets::EntityAnimationKeyframe {
                time_seconds: scalar(1.0),
                value: [scalar(1.0), scalar(0.0), scalar(0.0)],
                ..key
            },
        ]
        .into_boxed_slice();
        compiled.animation_channels[0].keyframe_count = 2;
    }
    if alternate {
        let mut geometry = compiled.geometries[0].clone();
        geometry.identifier = "geometry.title".into();
        let root = geometry.bones[0].clone();
        let mut child = root.clone();
        child.name = "child".into();
        child.parent = Some(root.name.clone());
        geometry.bones = vec![child, root].into_boxed_slice();
        let mut symbols = compiled.symbols.into_vec();
        symbols.insert(
            2,
            assets::EntityAssetSymbol {
                kind: assets::EntityAssetKind::Geometry,
                identifier: geometry.identifier.clone(),
                source_index: geometry.source_index,
                dependencies: Box::new([]),
            },
        );
        compiled.symbols = symbols.into_boxed_slice();
        let mut geometries = compiled.geometries.into_vec();
        geometries.push(geometry);
        compiled.geometries = geometries.into_boxed_slice();
        compiled.rig_bindings[0].render_controller = 4;
        compiled.animation_clips[0].symbol = 3;
        let clip = compiled.animation_clips[0];
        compiled.animation_clips = vec![
            clip,
            assets::EntityAnimationClip {
                geometry: Some(1),
                first_channel: 1,
                ..clip
            },
        ]
        .into_boxed_slice();
        let mut channel = compiled.animation_channels[0].clone();
        channel.bone = 1;
        channel.first_keyframe = 1;
        compiled.animation_channels =
            vec![compiled.animation_channels[0].clone(), channel].into_boxed_slice();
        compiled.animation_keyframes = compiled.animation_keyframes.repeat(2).into_boxed_slice();
        let layer = compiled.render.layers[0];
        compiled.render.layers = vec![
            layer,
            assets::EntityRenderLayer {
                first_slot: 1,
                geometry_count: 1,
                ..layer
            },
        ]
        .into_boxed_slice();
        let slot = compiled.render.slots[0];
        compiled.render.slots = vec![
            slot,
            assets::EntityRenderSlot {
                first_candidate: 1,
                ..slot
            },
        ]
        .into_boxed_slice();
        compiled.render.candidates = compiled.render.candidates.repeat(2).into_boxed_slice();
        compiled.render.geometries = vec![assets::EntityRenderGeometry {
            geometry: 1,
            condition: None,
        }]
        .into_boxed_slice();
    }

    edit(&mut compiled);
    let assets = Arc::new(RuntimeEntityAssets::from_compiled(compiled).unwrap());
    let mut store = crate::actor_store::ActorStore::new_with_entity_assets(1, 0, assets);
    store.apply(
        1,
        1,
        protocol::ActorEvent::Spawn(protocol::ActorSpawnEvent {
            dimension: 0,
            unique_id: 1,
            runtime_id: 1,
            kind: ActorKind::Entity {
                identifier: "minecraft:creeper".into(),
            },
            position: [0.; 3],
            velocity: [0.; 3],
            pitch: 0.,
            yaw: 0.,
            head_yaw: 0.,
            body_yaw: 0.,
            held_item: Default::default(),
            metadata: Arc::from([protocol::ActorMetadata {
                key: 0,
                value: ActorMetadataValue::Flags(1 << 10),
            }]),
            attributes: Arc::from([]),
            properties: Arc::from([]),
            links: Arc::from([]),
        }),
    );
    store.advance_interpolation_ticks(ticks);
    store
}

#[test]
fn direct_swell_channel_samples_fraction() {
    let store = compiled_swell_fixture(false);
    let rig = store.actor_rig(1).unwrap();
    let layers = store.render_frame(0.5).layers(1).unwrap();
    let (previous, current) = if layers[0].pose.is_empty() {
        (&rig.previous[0], &rig.current[0])
    } else {
        (&layers[0].previous_pose[0], &layers[0].pose[0])
    };
    let drawn_x = (previous.translation_scale[0] + current.translation_scale[0]) * 0.5;
    assert_eq!(drawn_x, -2.5 / 28.);
}

#[test]
fn swell_translation_with_pre_animation_samples_fraction() {
    let store = compiled_swell_fixture(true);
    let layers = store.render_frame(0.5).layers(1).unwrap();
    let drawn_x = (layers[0].previous_pose[0].translation_scale[0]
        + layers[0].pose[0].translation_scale[0])
        * 0.5;
    assert_eq!(drawn_x, -2.5 / 28.);
}

#[test]
fn authored_swell_trs_channels_follow_query_and_pre_animation_variables() {
    use assets::EntityAnimationProperty::*;
    for variable in [false, true] {
        for property in [Translation, Rotation, Scale] {
            let store = authored_swell_fixture(variable, property, variable);
            for alpha in [0.0, 0.25, 0.75, 1.0] {
                let value = (2.0 + alpha) / crate::actor_store::creeper::SWELL_FULL_TICKS;
                let layers = store.render_frame(alpha).layers(1).unwrap();
                assert!(!layers[0].pose.is_empty());
                for pose in [&layers[0].previous_pose[0], &layers[0].pose[0]] {
                    match property {
                        Translation => assert_eq!(pose.translation_scale[0], -value),
                        Rotation => {
                            assert_eq!(pose.rotation, pose::quat_from_euler([-value, 0.0, 0.0]))
                        }
                        Scale => assert!((pose::total_scale(pose)[0] - value).abs() < 1e-6),
                    }
                }
            }
        }
    }
}

#[test]
fn swell_script_preserves_an_independent_motion_variable_channel() {
    let store = authored_swell_fixture(true, assets::EntityAnimationProperty::Translation, true);
    let rig = store.actor_rig(1).unwrap();
    assert_ne!(
        rig.previous[0].translation_scale[1],
        rig.current[0].translation_scale[1]
    );
    for alpha in [0.0, 0.25, 0.75, 1.0] {
        let layers = store.render_frame(alpha).layers(1).unwrap();
        assert_eq!(
            layers[0].previous_pose[0].translation_scale[1],
            rig.previous[0].translation_scale[1]
        );
        assert_eq!(
            layers[0].pose[0].translation_scale[1],
            rig.current[0].translation_scale[1]
        );
    }
}

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
fn swell_layers_follow_geometry_specific_bone_order() {
    let store = pack_swell_fixture(
        false,
        assets::EntityAnimationProperty::Translation,
        false,
        None,
        true,
        3,
        false,
    );
    for alpha in [0.25, 0.5, 0.75] {
        let layers = store.render_frame(alpha).layers(1).unwrap();
        assert_eq!(layers.len(), 2);
        let expected = -(2.0 + alpha) / crate::actor_store::creeper::SWELL_FULL_TICKS;
        for pose in [&layers[1].previous_pose[1], &layers[1].pose[1]] {
            assert!((pose.translation_scale[0] - expected).abs() < 1e-6);
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
fn swell_driven_clip_time_samples_fraction_without_advancing_tick_clock() {
    let store = pack_swell_fixture(
        false,
        assets::EntityAnimationProperty::Translation,
        false,
        None,
        false,
        3,
        true,
    );
    let tick = store.actor_rig(1).unwrap().completed_tick;
    for alpha in [0.25, 0.5, 0.75] {
        let layers = store.render_frame(alpha).layers(1).unwrap();
        let expected = -(2.0 + alpha) / crate::actor_store::creeper::SWELL_FULL_TICKS;
        for pose in [&layers[0].previous_pose[0], &layers[0].pose[0]] {
            assert!((pose.translation_scale[0] - expected).abs() < 1e-6);
        }
    }
    assert_eq!(store.actor_rig(1).unwrap().completed_tick, tick);
}

#[test]
fn cumulative_swell_clock_reuses_the_pre_update_baseline() {
    let store = pack_swell_fixture_with(
        false,
        assets::EntityAnimationProperty::Translation,
        false,
        None,
        false,
        7,
        true,
        |compiled| {
            compiled.molang_symbols = [
                (assets::MolangSymbolKind::Name, "wield"),
                (assets::MolangSymbolKind::Query, "query.anim_time"),
                (assets::MolangSymbolKind::Query, "query.swell_amount"),
            ]
            .into_iter()
            .map(|(kind, identifier)| assets::MolangSymbol {
                kind,
                identifier: identifier.into(),
            })
            .collect::<Vec<_>>()
            .into_boxed_slice();
            compiled.molang_ops = vec![
                MolangOp::LoadQuery(1),
                MolangOp::LoadQuery(2),
                MolangOp::Add,
            ]
            .into_boxed_slice();
            compiled.molang_expressions[0].op_count = 3;
            compiled.molang_expressions[0].max_stack = 2;
        },
    );
    let tick = store.actor_rig(1).unwrap().completed_tick;
    let completed = store.actor_rig(1).unwrap().current[0].translation_scale[0];
    for alpha in [0.0, 0.25, 0.75, 1.0] {
        let layers = store.render_frame(alpha).layers(1).unwrap();
        let expected = completed - alpha / crate::actor_store::creeper::SWELL_FULL_TICKS;
        assert!(
            (layers[0].pose[0].translation_scale[0] - expected).abs() < 1e-6,
            "scratch clock must recompute the same cumulative update"
        );
    }
    assert_eq!(store.actor_rig(1).unwrap().completed_tick, tick);
}

#[test]
fn swell_and_motion_share_one_axis_without_losing_motion_history() {
    for operation in [MolangOp::Add, MolangOp::Multiply] {
        let store = pack_swell_fixture_with(
            true,
            assets::EntityAnimationProperty::Translation,
            true,
            None,
            false,
            7,
            false,
            |compiled| {
                let mut ops = compiled.molang_ops.to_vec();
                ops.extend([
                    MolangOp::LoadVariable(3),
                    MolangOp::LoadVariable(4),
                    operation,
                ]);
                compiled.molang_ops = ops.into_boxed_slice();
                let mut expressions = compiled.molang_expressions.to_vec();
                expressions.push(assets::CompiledMolangExpression {
                    first_op: 7,
                    op_count: 3,
                    max_stack: 2,
                });
                compiled.molang_expressions = expressions.into_boxed_slice();
                compiled.animation_keyframes[0].expressions = [Some(3), None, None];
            },
        );
        for alpha in [0.0, 0.25, 0.75, 1.0] {
            let swell = (6.0 + alpha) / crate::actor_store::creeper::SWELL_FULL_TICKS;
            let layers = store.render_frame(alpha).layers(1).unwrap();
            for (pose, life_tick) in [
                (&layers[0].previous_pose[0], 6.0),
                (&layers[0].pose[0], 7.0),
            ] {
                let motion = life_tick * ACTOR_TICK_DURATION.as_secs_f32();
                let expected = -match operation {
                    MolangOp::Add => swell + motion,
                    _ => swell * motion,
                };
                assert!(
                    (pose.translation_scale[0] - expected).abs() < 1e-6,
                    "independent motion retains its endpoint while swell samples the fraction"
                );
            }
        }
    }
}

#[test]
fn swell_on_an_alternate_only_bone_samples_the_frame_fraction() {
    let store = pack_swell_fixture_with(
        false,
        assets::EntityAnimationProperty::Translation,
        false,
        None,
        true,
        8,
        false,
        |compiled| {
            compiled.animation_clips[0].channel_count = 0;
            compiled.animation_clips[1].first_channel = 0;
            let mut channel = compiled.animation_channels[1].clone();
            channel.bone = 0;
            channel.first_keyframe = 0;
            compiled.animation_channels = vec![channel].into_boxed_slice();
            compiled.animation_keyframes = vec![compiled.animation_keyframes[1]].into_boxed_slice();
        },
    );
    for alpha in [0.0, 0.25, 0.75, 1.0] {
        let layers = store.render_frame(alpha).layers(1).unwrap();
        let expected = -(7.0 + alpha) / crate::actor_store::creeper::SWELL_FULL_TICKS;
        for pose in [&layers[1].previous_pose[0], &layers[1].pose[0]] {
            assert!(
                (pose.translation_scale[0] - expected).abs() < 1e-6,
                "alternate-only channel uses the fractional swell query"
            );
        }
    }
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
    let store = pack_swell_fixture_with(
        false,
        assets::EntityAnimationProperty::Translation,
        false,
        None,
        false,
        3,
        false,
        |compiled| {
            use assets::*;
            let scalar = |v| EntityGeometryScalar::new(v).unwrap();
            let mut symbols = compiled.symbols.to_vec();
            symbols.insert(
                3,
                EntityAssetSymbol {
                    kind: EntityAssetKind::AnimationController,
                    identifier: "controller.animation.test".into(),
                    source_index: 0,
                    dependencies: Box::new([]),
                },
            );
            let mut sources = compiled.sources.to_vec();
            sources.insert(
                0,
                EntityAssetSource {
                    path: "animation_controllers/test.json".into(),
                    source_bytes: 1,
                    source_sha256: [1; 32],
                },
            );
            for s in &mut symbols {
                s.source_index += 1;
            }
            symbols[3].source_index = 0;
            compiled.symbols = symbols.into();
            compiled.sources = sources.into();
            for g in &mut compiled.geometries {
                g.source_index += 1;
            }
            compiled.animation_clips[0].source += 1;
            compiled.rig_bindings[0].render_controller = 4;
            for c in &mut compiled.render.candidates {
                c.source = 5;
            }
            compiled.molang_symbols = vec![
                MolangSymbol {
                    kind: MolangSymbolKind::Name,
                    identifier: "default".into(),
                },
                MolangSymbol {
                    kind: MolangSymbolKind::Name,
                    identifier: "second".into(),
                },
                MolangSymbol {
                    kind: MolangSymbolKind::Name,
                    identifier: "wield".into(),
                },
                MolangSymbol {
                    kind: MolangSymbolKind::Query,
                    identifier: "query.life_time".into(),
                },
                MolangSymbol {
                    kind: MolangSymbolKind::Query,
                    identifier: "query.swell_amount".into(),
                },
            ]
            .into();
            compiled.molang_ops = vec![
                MolangOp::LoadQuery(4),
                MolangOp::LoadQuery(4),
                MolangOp::Push(scalar(1.0)),
                MolangOp::Add,
                MolangOp::LoadQuery(3),
                MolangOp::Push(scalar(0.12)),
                MolangOp::Greater,
            ]
            .into();
            compiled.molang_expressions = vec![
                CompiledMolangExpression {
                    first_op: 0,
                    op_count: 1,
                    max_stack: 1,
                },
                CompiledMolangExpression {
                    first_op: 1,
                    op_count: 3,
                    max_stack: 2,
                },
                CompiledMolangExpression {
                    first_op: 4,
                    op_count: 3,
                    max_stack: 2,
                },
            ]
            .into();
            compiled.controllers = vec![EntityAnimationController {
                symbol: 3,
                first_state: 0,
                state_count: 2,
                initial_state: 0,
            }]
            .into();
            compiled.controller_states = vec![
                EntityControllerState {
                    name: 0,
                    first_animation: 0,
                    animation_count: 1,
                    first_transition: 0,
                    transition_count: 1,
                    ..Default::default()
                },
                EntityControllerState {
                    name: 1,
                    first_animation: 1,
                    animation_count: 1,
                    first_transition: 1,
                    ..Default::default()
                },
            ]
            .into();
            compiled.controller_animations = vec![
                EntityControllerAnimation {
                    target: EntityControllerAnimationTarget::Clip(0),
                    weight: Some(0),
                },
                EntityControllerAnimation {
                    target: EntityControllerAnimationTarget::Clip(0),
                    weight: Some(1),
                },
            ]
            .into();
            compiled.controller_transitions = vec![EntityControllerTransition {
                target_state: 1,
                condition: 2,
            }]
            .into();
            compiled.rig_geometries[0].animation_count = 0;
            compiled.rig_animations = Box::new([]);
            compiled.rig_geometries[0].controller_count = 1;
            compiled.rig_controllers = vec![EntityRigControllerBinding {
                name: 0,
                controller: 0,
                weight: None,
                order: 0,
            }]
            .into();
        },
    );
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
