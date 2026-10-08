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
        AuthoredSwellChannel {
            pre_animation: pre_animation,
            property: property,
            variable: variable,
        },
        weighted_query_channel,
        alternate,
        ticks,
        swell_time,
        |_| {},
    )
}

struct AuthoredSwellChannel {
    pre_animation: bool,
    property: assets::EntityAnimationProperty,
    variable: bool,
}

fn pack_swell_fixture_with(
    channel: AuthoredSwellChannel,
    weighted_query_channel: Option<bool>,
    alternate: bool,
    ticks: u32,
    swell_time: bool,
    edit: impl FnOnce(&mut assets::CompiledEntityAssets),
) -> crate::actor_store::ActorStore {
    let AuthoredSwellChannel {
        pre_animation,
        property,
        variable,
    } = channel;
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
    assert_eq!(
        drawn_x,
        -2.5 / crate::actor_store::creeper::SWELL_FULL_TICKS
    );
}

#[test]
fn swell_translation_with_pre_animation_samples_fraction() {
    let store = compiled_swell_fixture(true);
    let layers = store.render_frame(0.5).layers(1).unwrap();
    let drawn_x = (layers[0].previous_pose[0].translation_scale[0]
        + layers[0].pose[0].translation_scale[0])
        * 0.5;
    assert_eq!(
        drawn_x,
        -2.5 / crate::actor_store::creeper::SWELL_FULL_TICKS
    );
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
        AuthoredSwellChannel {
            pre_animation: false,
            property: assets::EntityAnimationProperty::Translation,
            variable: false,
        },
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
            AuthoredSwellChannel {
                pre_animation: true,
                property: assets::EntityAnimationProperty::Translation,
                variable: true,
            },
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
        AuthoredSwellChannel {
            pre_animation: false,
            property: assets::EntityAnimationProperty::Translation,
            variable: false,
        },
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
    let store = swell_controller_fixture(3, 0.12, false);
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

fn swell_controller_fixture(
    ticks: u32,
    threshold: f32,
    deactivate: bool,
) -> crate::actor_store::ActorStore {
    swell_controller_fixture_with(ticks, threshold, deactivate, |_| {})
}

fn swell_controller_fixture_with(
    ticks: u32,
    threshold: f32,
    deactivate: bool,
    edit: impl FnOnce(&mut assets::CompiledEntityAssets),
) -> crate::actor_store::ActorStore {
    pack_swell_fixture_with(
        AuthoredSwellChannel {
            pre_animation: false,
            property: assets::EntityAnimationProperty::Translation,
            variable: false,
        },
        None,
        false,
        ticks,
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
                MolangOp::Push(scalar(if deactivate { 0.0 } else { 1.0 })),
                if deactivate {
                    MolangOp::Multiply
                } else {
                    MolangOp::Add
                },
                MolangOp::LoadQuery(3),
                MolangOp::Push(scalar(threshold)),
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
            edit(compiled);
        },
    )
}

#[test]
fn previous_controller_can_activate_a_swell_channel_at_the_frame_fraction() {
    let store = swell_controller_fixture(2, 0.07, true);
    for alpha in [0.25, 0.5, 0.75] {
        let layers = store.render_frame(alpha).layers(1).unwrap();
        let swell = (1.0 + alpha) / crate::actor_store::creeper::SWELL_FULL_TICKS;
        assert!(
            (layers[0].previous_pose[0].translation_scale[0] + swell * swell).abs() < 1e-6,
            "newly active previous-endpoint channels join the mask: alpha {alpha}, value {}, swell {swell}",
            layers[0].previous_pose[0].translation_scale[0]
        );
        assert_eq!(
            layers[0].pose[0].translation_scale[0], 0.0,
            "transitioned zero-weight current state stays at rest"
        );
    }
}

#[test]
fn swell_keyframe_writes_feed_later_channels_and_render_without_staling_frame_variables() {
    let store = pack_swell_fixture_with(
        AuthoredSwellChannel {
            pre_animation: true,
            property: assets::EntityAnimationProperty::Translation,
            variable: false,
        },
        None,
        false,
        3,
        false,
        |compiled| {
            compiled.molang_symbols = [
                (assets::MolangSymbolKind::Name, "wield"),
                (assets::MolangSymbolKind::Query, "query.frame_alpha"),
                (assets::MolangSymbolKind::Query, "query.swell_amount"),
                (assets::MolangSymbolKind::Variable, "variable.color"),
                (assets::MolangSymbolKind::Variable, "variable.frame"),
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
                MolangOp::StoreVariable(4),
                MolangOp::Push(assets::EntityGeometryScalar::new(0.0).unwrap()),
                MolangOp::LoadQuery(2),
                MolangOp::StoreVariable(3),
                MolangOp::LoadVariable(3),
                MolangOp::LoadVariable(3),
                MolangOp::LoadVariable(4),
            ]
            .into_boxed_slice();
            compiled.molang_expressions = [(0, 3), (3, 3), (6, 1), (7, 1)]
                .into_iter()
                .map(|(first_op, op_count)| assets::CompiledMolangExpression {
                    first_op,
                    op_count,
                    max_stack: 1,
                })
                .collect::<Vec<_>>()
                .into_boxed_slice();
            compiled.animation_keyframes[0].expressions = [Some(1), None, None];
            let mut scale = compiled.animation_channels[0].clone();
            scale.property = assets::EntityAnimationProperty::Scale;
            scale.first_keyframe = 1;
            let mut key = compiled.animation_keyframes[0];
            key.value = [assets::EntityGeometryScalar::new(1.0).unwrap(); 3];
            key.expressions = [Some(2), None, None];
            compiled.animation_keyframes =
                vec![compiled.animation_keyframes[0], key].into_boxed_slice();
            compiled.animation_channels =
                vec![compiled.animation_channels[0].clone(), scale].into_boxed_slice();
            compiled.animation_clips[0].channel_count = 2;
            compiled.render.layers[0].color = Some([2, 3, 2, 3]);
        },
    );
    let tick = store.actor_rig(1).unwrap().completed_tick;
    for alpha in [0.0, 0.25, 0.5, 0.75, 1.0] {
        let amount = (2.0 + alpha) / crate::actor_store::creeper::SWELL_FULL_TICKS;
        let layers = store.render_frame(alpha).layers(1).unwrap();
        assert!(
            (layers[0].color[0] - amount).abs() < 1e-6,
            "keyframe writes reach render colors: {:?}",
            layers[0].color
        );
        assert_eq!(
            layers[0].color[1], alpha,
            "ordinary pre-animation uses the current frame"
        );
        for endpoint in [&layers[0].previous_pose[0], &layers[0].pose[0]] {
            assert!(
                (endpoint.axis_scale[0] * endpoint.translation_scale[3] - amount).abs() < 1e-6,
                "later channels read the sampled assignment"
            );
        }
    }
    assert_eq!(store.actor_rig(1).unwrap().completed_tick, tick);
}

fn swell_override_fixture(relative_child: bool, ticks: u32) -> crate::actor_store::ActorStore {
    pack_swell_fixture_with(
        AuthoredSwellChannel {
            pre_animation: false,
            property: assets::EntityAnimationProperty::Translation,
            variable: false,
        },
        Some(false),
        false,
        ticks,
        false,
        |compiled| {
            let mut symbols = compiled.symbols.to_vec();
            let mut symbol = symbols[2].clone();
            symbol.identifier = "animation.zz_override".into();
            symbols.insert(3, symbol);
            compiled.symbols = symbols.into_boxed_slice();
            compiled.rig_bindings[0].render_controller = 4;
            compiled.render.candidates[0].source = 4;
            let mut first_clip = compiled.animation_clips[0];
            first_clip.channel_count = 3;
            let mut second_clip = first_clip;
            second_clip.symbol = 3;
            second_clip.first_channel = 3;
            second_clip.channel_count = 1;
            second_clip.override_previous = true;
            compiled.animation_clips = vec![first_clip, second_clip].into_boxed_slice();
            let scalar = |value| assets::EntityGeometryScalar::new(value).unwrap();
            let original = compiled.animation_channels[0].clone();
            compiled.animation_channels = [
                assets::EntityAnimationProperty::Rotation,
                assets::EntityAnimationProperty::Scale,
                assets::EntityAnimationProperty::Translation,
                assets::EntityAnimationProperty::Translation,
            ]
            .into_iter()
            .enumerate()
            .map(|(index, property)| assets::EntityAnimationChannel {
                property,
                first_keyframe: index as u32,
                ..original.clone()
            })
            .collect::<Vec<_>>()
            .into_boxed_slice();
            let key = compiled.animation_keyframes[0];
            compiled.animation_keyframes = [45.0, 2.0, 3.0, 1.0]
                .into_iter()
                .map(|value| assets::EntityAnimationKeyframe {
                    value: [scalar(value); 3],
                    expressions: [None; 3],
                    ..key
                })
                .collect::<Vec<_>>()
                .into_boxed_slice();
            let first_binding = assets::EntityRigAnimationBinding {
                clip: 0,
                weight: None,
                ..compiled.rig_animations[0]
            };
            let second_binding = assets::EntityRigAnimationBinding {
                clip: 1,
                order: 1,
                ..compiled.rig_animations[0]
            };
            compiled.rig_animations = vec![first_binding, second_binding].into_boxed_slice();
            compiled.rig_geometries[0].animation_count = 2;
            if relative_child {
                let root = compiled.geometries[0].bones[0].clone();
                let mut child = root.clone();
                child.name = "child".into();
                child.parent = Some(root.name);
                compiled.geometries[0].bones =
                    vec![compiled.geometries[0].bones[0].clone(), child].into_boxed_slice();
                compiled.animation_channels[2].bone = 1;
                compiled.animation_channels[2].rotation_relative_to_entity = true;
                compiled.animation_channels[3].bone = 1;
            }
        },
    )
}

#[test]
fn swell_weighted_override_resets_every_transform_of_its_bone() {
    let store = swell_override_fixture(false, 1);
    for alpha in [0.25, 0.5, 0.75, 1.0] {
        let amount = alpha / crate::actor_store::creeper::SWELL_FULL_TICKS;
        let layers = store.render_frame(alpha).layers(1).unwrap();
        for endpoint in [&layers[0].previous_pose[0], &layers[0].pose[0]] {
            assert_eq!(
                endpoint.rotation,
                [0.0, 0.0, 0.0, 1.0],
                "override resets cached rotations"
            );
            assert_eq!(
                endpoint.axis_scale[0] * endpoint.translation_scale[3],
                1.0,
                "override resets cached scales"
            );
            assert!(
                (endpoint.translation_scale[0] + amount).abs() < 1e-6,
                "override resets prior translations"
            );
        }
    }
}

#[test]
fn swell_override_restores_the_inherited_rotation_and_scale_basis() {
    let store = swell_override_fixture(true, 1);
    for alpha in [0.25, 0.5, 0.75, 1.0] {
        let layers = store.render_frame(alpha).layers(1).unwrap();
        for pose in [&layers[0].previous_pose, &layers[0].pose] {
            assert_eq!(
                pose[1].rotation, pose[0].rotation,
                "override restores the parent rotation basis"
            );
            assert_eq!(
                pose[1].translation_scale[3], 2.0,
                "override restores inherited scale"
            );
        }
    }
}

#[test]
fn swell_weight_and_clock_assignments_publish_without_advancing_state() {
    for clock in [false, true] {
        let store = pack_swell_fixture_with(
            AuthoredSwellChannel {
                pre_animation: false,
                property: assets::EntityAnimationProperty::Translation,
                variable: false,
            },
            (!clock).then_some(false),
            false,
            3,
            clock,
            |compiled| {
                let mut symbols = compiled.molang_symbols.to_vec();
                symbols.push(assets::MolangSymbol {
                    kind: assets::MolangSymbolKind::Variable,
                    identifier: "variable.color".into(),
                });
                compiled.molang_symbols = symbols.into_boxed_slice();
                compiled.molang_ops = vec![
                    MolangOp::LoadQuery(1),
                    MolangOp::StoreVariable(2),
                    MolangOp::LoadVariable(2),
                    MolangOp::LoadVariable(2),
                ]
                .into_boxed_slice();
                compiled.molang_expressions = [(0, 3), (3, 1)]
                    .into_iter()
                    .map(|(first_op, op_count)| assets::CompiledMolangExpression {
                        first_op,
                        op_count,
                        max_stack: 1,
                    })
                    .collect::<Vec<_>>()
                    .into_boxed_slice();
                compiled.render.layers[0].color = Some([1; 4]);
            },
        );
        let tick = store.actor_rig(1).unwrap().completed_tick;
        for alpha in [0.25, 0.5, 1.0, 0.5] {
            let amount = (2.0 + alpha) / crate::actor_store::creeper::SWELL_FULL_TICKS;
            let layers = store.render_frame(alpha).layers(1).unwrap();
            assert!(
                (layers[0].color[0] - amount).abs() < 1e-6,
                "weight/clock assignments reach render selection"
            );
        }
        assert_eq!(store.actor_rig(1).unwrap().completed_tick, tick);
    }
}

#[test]
fn swell_override_deactivation_restores_previous_animation_channels() {
    let mut store = swell_override_fixture(false, 1);
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
    store.advance_interpolation_ticks(1);
    let layers = store.render_frame(1.0).layers(1).unwrap();
    let expected = pose::quat_from_euler([-45.0, -45.0, 45.0]);
    for endpoint in [&layers[0].previous_pose[0], &layers[0].pose[0]] {
        assert_eq!(
            endpoint.rotation, expected,
            "inactive override restores rotation from the earlier clip"
        );
        assert_eq!(
            endpoint.translation_scale[3], 2.0,
            "inactive override restores scale"
        );
        assert_eq!(
            endpoint.translation_scale[0], -3.0,
            "inactive override restores translation"
        );
    }
}

#[test]
fn swell_assignments_drive_geometry_texture_and_visibility_selection() {
    let store = pack_swell_fixture_with(
        AuthoredSwellChannel {
            pre_animation: false,
            property: assets::EntityAnimationProperty::Translation,
            variable: false,
        },
        None,
        true,
        3,
        false,
        |compiled| {
            let scalar = |value| assets::EntityGeometryScalar::new(value).unwrap();
            let mut symbols = compiled.molang_symbols.to_vec();
            symbols.push(assets::MolangSymbol {
                kind: assets::MolangSymbolKind::Variable,
                identifier: "variable.selection".into(),
            });
            compiled.molang_symbols = symbols.into_boxed_slice();
            compiled.molang_ops = vec![
                MolangOp::LoadQuery(1),
                MolangOp::StoreVariable(2),
                MolangOp::LoadVariable(2),
                MolangOp::LoadVariable(2),
                MolangOp::LoadVariable(2),
                MolangOp::Push(scalar(0.06)),
                MolangOp::Greater,
            ]
            .into_boxed_slice();
            compiled.molang_expressions = [(0, 3, 1), (3, 1, 1), (4, 3, 2)]
                .into_iter()
                .map(
                    |(first_op, op_count, max_stack)| assets::CompiledMolangExpression {
                        first_op,
                        op_count,
                        max_stack,
                    },
                )
                .collect::<Vec<_>>()
                .into_boxed_slice();
            compiled.render.layers[0].color = Some([1; 4]);
            compiled.render.layers[1].condition = Some(2);
            compiled.render.geometries[0].condition = Some(2);
            for candidate in &mut compiled.render.candidates {
                candidate.condition = Some(2);
            }
            compiled.render.visibility = vec![assets::EntityRenderVisibility {
                pattern: "*".into(),
                condition: 2,
            }]
            .into_boxed_slice();
            compiled.render.layers[0].visibility_count = 1;
            compiled.render.layers[1].first_visibility = 1;
        },
    );
    let layers = store.render_frame(0.5).layers(1).unwrap();
    assert_eq!(
        layers.len(),
        2,
        "sampled assignment enables both render layers and texture choices"
    );
    assert_eq!(
        layers[1].geometry,
        Some(1),
        "sampled assignment selects the alternate geometry"
    );
    assert!(
        layers[0].hidden_bones.is_empty(),
        "sampled assignment controls bone visibility"
    );
    assert!(
        (layers[0].color[0] - 2.5 / crate::actor_store::creeper::SWELL_FULL_TICKS).abs() < 1e-6
    );
}

#[test]
fn swell_render_controller_writes_feed_selected_geometry_channels() {
    let store = pack_swell_fixture_with(
        AuthoredSwellChannel {
            pre_animation: false,
            property: assets::EntityAnimationProperty::Translation,
            variable: false,
        },
        None,
        true,
        3,
        false,
        |compiled| {
            let mut symbols = compiled.molang_symbols.to_vec();
            symbols.push(assets::MolangSymbol {
                kind: assets::MolangSymbolKind::Variable,
                identifier: "variable.layer".into(),
            });
            compiled.molang_symbols = symbols.into_boxed_slice();
            compiled.molang_ops = vec![
                MolangOp::LoadQuery(1),
                MolangOp::LoadQuery(1),
                MolangOp::StoreVariable(2),
                MolangOp::LoadVariable(2),
                MolangOp::LoadVariable(2),
            ]
            .into_boxed_slice();
            compiled.molang_expressions = [(0, 1), (1, 3), (4, 1)]
                .into_iter()
                .map(|(first_op, op_count)| assets::CompiledMolangExpression {
                    first_op,
                    op_count,
                    max_stack: 1,
                })
                .collect::<Vec<_>>()
                .into_boxed_slice();
            compiled.render.layers[0].color = Some([1; 4]);
            compiled.animation_keyframes[1].expressions = [Some(2), None, None];
        },
    );
    let tick = store.actor_rig(1).unwrap().completed_tick;
    for alpha in [0.25, 0.5, 1.0, 0.5] {
        let amount = (2.0 + alpha) / crate::actor_store::creeper::SWELL_FULL_TICKS;
        let layers = store.render_frame(alpha).layers(1).unwrap();
        for endpoint in [&layers[1].previous_pose[1], &layers[1].pose[1]] {
            assert!(
                (endpoint.translation_scale[0] + amount).abs() < 1e-6,
                "render writes reach selected geometry channels"
            );
        }
    }
    assert_eq!(store.actor_rig(1).unwrap().completed_tick, tick);
}

#[test]
fn swell_render_writes_keep_each_geometry_motion_endpoint() {
    let store = pack_swell_fixture_with(
        AuthoredSwellChannel {
            pre_animation: false,
            property: assets::EntityAnimationProperty::Translation,
            variable: false,
        },
        None,
        true,
        7,
        false,
        |compiled| {
            compiled.molang_symbols = [
                (assets::MolangSymbolKind::Name, "wield"),
                (assets::MolangSymbolKind::Query, "query.life_time"),
                (assets::MolangSymbolKind::Query, "query.swell_amount"),
                (assets::MolangSymbolKind::Variable, "variable.layer"),
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
                MolangOp::LoadQuery(2),
                MolangOp::LoadQuery(1),
                MolangOp::Add,
                MolangOp::StoreVariable(3),
                MolangOp::LoadVariable(3),
                MolangOp::LoadVariable(3),
            ]
            .into_boxed_slice();
            compiled.molang_expressions = [(0, 1, 1), (1, 5, 2), (6, 1, 1)]
                .into_iter()
                .map(
                    |(first_op, op_count, max_stack)| assets::CompiledMolangExpression {
                        first_op,
                        op_count,
                        max_stack,
                    },
                )
                .collect::<Vec<_>>()
                .into_boxed_slice();
            compiled.render.layers[0].color = Some([1; 4]);
            compiled.animation_keyframes[1].expressions = [Some(2), None, None];
        },
    );
    for alpha in [0.0, 0.25, 0.75, 1.0] {
        let amount = (6.0 + alpha) / crate::actor_store::creeper::SWELL_FULL_TICKS;
        let layers = store.render_frame(alpha).layers(1).unwrap();
        for (endpoint, tick) in [
            (&layers[1].previous_pose[1], 6.0),
            (&layers[1].pose[1], 7.0),
        ] {
            let expected = -(amount + tick * ACTOR_TICK_DURATION.as_secs_f32());
            assert!(
                (endpoint.translation_scale[0] - expected).abs() < 1e-6,
                "render assignment keeps the geometry's ordinary endpoint: actual {} expected {}",
                endpoint.translation_scale[0],
                expected
            );
        }
    }
}

#[test]
fn swell_endpoint_retains_controller_entry_and_exit_assignments_after_pre_animation() {
    for weighted in [false, true] {
        let mut store = swell_controller_fixture_with(3, 0.12, false, |compiled| {
            let scalar = |value| assets::EntityGeometryScalar::new(value).unwrap();
            let mut symbols = compiled.molang_symbols.to_vec();
            symbols.push(assets::MolangSymbol {
                kind: assets::MolangSymbolKind::Variable,
                identifier: "variable.factor".into(),
            });
            compiled.molang_symbols = symbols.into_boxed_slice();
            let mut ops = compiled.molang_ops.to_vec();
            ops.extend([
                MolangOp::Push(scalar(1.0)),
                MolangOp::StoreVariable(5),
                MolangOp::Push(scalar(0.0)),
                MolangOp::Push(scalar(2.0)),
                MolangOp::StoreVariable(5),
                MolangOp::Push(scalar(0.0)),
                MolangOp::Push(scalar(3.0)),
                MolangOp::StoreVariable(5),
                MolangOp::Push(scalar(0.0)),
                MolangOp::LoadQuery(4),
                MolangOp::LoadVariable(5),
                MolangOp::Multiply,
            ]);
            compiled.molang_ops = ops.into_boxed_slice();
            let mut expressions = compiled.molang_expressions.to_vec();
            expressions.extend([(7, 3, 1), (10, 3, 1), (13, 3, 1), (16, 3, 2)].map(
                |(first_op, op_count, max_stack)| assets::CompiledMolangExpression {
                    first_op,
                    op_count,
                    max_stack,
                },
            ));
            compiled.molang_expressions = expressions.into_boxed_slice();
            compiled.rig_bindings[0].pre_animation = Some(3);
            compiled.controller_states[0].on_exit = Some(4);
            compiled.controller_states[1].on_entry = Some(5);
            compiled.animation_keyframes[0].expressions = [Some(6), None, None];
            for animation in &mut compiled.controller_animations {
                animation.weight = weighted.then_some(0);
            }
        });
        for (before, after) in [(1.0, 3.0), (3.0, 1.0)] {
            let tick = store.actor_rig(1).unwrap().completed_tick;
            let amount = (tick as f32 - 1.0 + 0.5) / crate::actor_store::creeper::SWELL_FULL_TICKS;
            let factor = if weighted { amount * amount } else { amount };
            let layers = store.render_frame(0.5).layers(1).unwrap();
            for (pose, multiplier) in [
                (&layers[0].previous_pose[0], before),
                (&layers[0].pose[0], after),
            ] {
                assert!(
                    (pose.translation_scale[0] + factor * multiplier).abs() < 1e-6,
                    "entry/exit writes follow pre-animation at their endpoint"
                );
            }
            store.advance_interpolation_ticks(1);
        }
    }
}

#[test]
fn swell_endpoint_retains_ordinary_actor_hurt_query_history() {
    let mut store = pack_swell_fixture_with(
        AuthoredSwellChannel {
            pre_animation: false,
            property: assets::EntityAnimationProperty::Translation,
            variable: false,
        },
        None,
        false,
        0,
        false,
        |compiled| {
            compiled.molang_symbols = [
                (assets::MolangSymbolKind::Name, "wield"),
                (assets::MolangSymbolKind::Query, "query.hurt_time"),
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
                MolangOp::LoadQuery(2),
                MolangOp::LoadQuery(1),
                MolangOp::Add,
            ]
            .into_boxed_slice();
            compiled.molang_expressions[0].op_count = 3;
            compiled.molang_expressions[0].max_stack = 2;
        },
    );
    store.apply(
        1,
        2,
        protocol::ActorEvent::Status(protocol::ActorStatusEvent {
            runtime_id: 1,
            kind: protocol::ActorStatusKind::Hurt,
            data: 0,
        }),
    );
    store.advance_interpolation_ticks(2);
    let hurt = f32::from(store.get(1).unwrap().status.hurt_time);
    let layers = store.render_frame(0.5).layers(1).unwrap();
    let amount = 1.5 / crate::actor_store::creeper::SWELL_FULL_TICKS;
    for (pose, value) in [
        (&layers[0].previous_pose[0], hurt + 1.0),
        (&layers[0].pose[0], hurt),
    ] {
        assert!(
            (pose.translation_scale[0] + amount + value).abs() < 1e-6,
            "ordinary hurt query stays at its pose endpoint"
        );
    }
}

#[test]
fn swell_dependencies_flow_through_this_into_later_bones() {
    let store = pack_swell_fixture_with(
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
                identifier: "variable.copy".into(),
            });
            compiled.molang_symbols = symbols.into_boxed_slice();
            let mut second = compiled.geometries[0].bones[0].clone();
            second.name = "second".into();
            compiled.geometries[0].bones =
                vec![compiled.geometries[0].bones[0].clone(), second].into_boxed_slice();
            compiled.molang_ops = vec![
                MolangOp::LoadQuery(1),
                MolangOp::LoadThis,
                MolangOp::StoreVariable(2),
                MolangOp::Push(scalar(0.0)),
                MolangOp::LoadVariable(2),
            ]
            .into_boxed_slice();
            compiled.molang_expressions = [(0, 1), (1, 3), (4, 1)]
                .into_iter()
                .map(|(first_op, op_count)| assets::CompiledMolangExpression {
                    first_op,
                    op_count,
                    max_stack: 1,
                })
                .collect::<Vec<_>>()
                .into_boxed_slice();
            let original = compiled.animation_channels[0].clone();
            compiled.animation_channels = (0..3)
                .map(|index| assets::EntityAnimationChannel {
                    bone: u32::from(index == 2),
                    first_keyframe: index,
                    ..original.clone()
                })
                .collect::<Vec<_>>()
                .into_boxed_slice();
            let key = compiled.animation_keyframes[0];
            compiled.animation_keyframes = (0..3)
                .map(|index| assets::EntityAnimationKeyframe {
                    expressions: [Some(index), None, None],
                    ..key
                })
                .collect::<Vec<_>>()
                .into_boxed_slice();
            compiled.animation_clips[0].channel_count = 3;
        },
    );
    let layers = store.render_frame(0.5).layers(1).unwrap();
    for pose in [&layers[0].previous_pose[1], &layers[0].pose[1]] {
        assert!(
            (pose.translation_scale[0] + 2.5 / crate::actor_store::creeper::SWELL_FULL_TICKS).abs()
                < 1e-6,
            "this-derived assignments reach later bones"
        );
    }
}

#[test]
fn swell_shared_clock_updates_execute_once_per_endpoint() {
    let store = pack_swell_fixture_with(
        AuthoredSwellChannel {
            pre_animation: false,
            property: assets::EntityAnimationProperty::Translation,
            variable: false,
        },
        None,
        false,
        3,
        true,
        |compiled| {
            compiled.molang_symbols = [
                (assets::MolangSymbolKind::Name, "other"),
                (assets::MolangSymbolKind::Name, "wield"),
                (assets::MolangSymbolKind::Query, "query.swell_amount"),
                (assets::MolangSymbolKind::Variable, "variable.counter"),
            ]
            .into_iter()
            .map(|(kind, identifier)| assets::MolangSymbol {
                kind,
                identifier: identifier.into(),
            })
            .collect::<Vec<_>>()
            .into_boxed_slice();
            compiled.molang_ops = vec![
                MolangOp::LoadVariable(3),
                MolangOp::LoadQuery(2),
                MolangOp::Add,
                MolangOp::StoreVariable(3),
                MolangOp::LoadVariable(3),
                MolangOp::LoadVariable(3),
            ]
            .into_boxed_slice();
            compiled.molang_expressions = [(0, 5, 2), (5, 1, 1)]
                .into_iter()
                .map(
                    |(first_op, op_count, max_stack)| assets::CompiledMolangExpression {
                        first_op,
                        op_count,
                        max_stack,
                    },
                )
                .collect::<Vec<_>>()
                .into_boxed_slice();
            let binding = compiled.rig_animations[0];
            compiled.rig_animations = vec![
                assets::EntityRigAnimationBinding { name: 0, ..binding },
                assets::EntityRigAnimationBinding {
                    name: 1,
                    order: 1,
                    ..binding
                },
            ]
            .into_boxed_slice();
            compiled.rig_geometries[0].animation_count = 2;
            compiled.render.layers[0].color = Some([1; 4]);
        },
    );
    let completed = store.actor_rig(1).unwrap().render[0].color[0];
    for alpha in [0.25, 0.5, 1.0, 0.5] {
        let layers = store.render_frame(alpha).layers(1).unwrap();
        let expected = completed + alpha / crate::actor_store::creeper::SWELL_FULL_TICKS;
        assert!(
            (layers[0].color[0] - expected).abs() < 1e-6,
            "duplicate clock references publish one update: {} expected {} completed {} alpha {}",
            layers[0].color[0],
            expected,
            completed,
            alpha
        );
        assert!(
            (layers[0].pose[0].translation_scale[0] + 2.0 * expected).abs() < 1e-6,
            "duplicate references share the same sampled time"
        );
    }
    assert_eq!(store.actor_rig(1).unwrap().render[0].color[0], completed);
}

#[test]
fn swell_keyframe_random_draw_precedes_the_render_random_draw() {
    let store = pack_swell_fixture_with(
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
                identifier: "variable.random".into(),
            });
            compiled.molang_symbols = symbols.into_boxed_slice();
            compiled.molang_ops = vec![
                MolangOp::Push(scalar(0.0)),
                MolangOp::Push(scalar(1.0)),
                MolangOp::Call(assets::MolangFunction::Random),
                MolangOp::StoreVariable(2),
                MolangOp::LoadQuery(1),
                MolangOp::LoadVariable(2),
                MolangOp::Multiply,
                MolangOp::Push(scalar(0.0)),
                MolangOp::Push(scalar(1.0)),
                MolangOp::Call(assets::MolangFunction::Random),
            ]
            .into_boxed_slice();
            compiled.molang_expressions = [(0, 7, 2), (7, 3, 2)]
                .into_iter()
                .map(
                    |(first_op, op_count, max_stack)| assets::CompiledMolangExpression {
                        first_op,
                        op_count,
                        max_stack,
                    },
                )
                .collect::<Vec<_>>()
                .into_boxed_slice();
            compiled.render.layers[0].color = Some([1; 4]);
        },
    );
    let completed = store.actor_rig(1).unwrap().render[0].color;
    for alpha in [0.25, 0.5, 1.0, 0.5] {
        assert_eq!(
            store.render_frame(alpha).layers(1).unwrap()[0].color,
            completed,
            "body and render share one scratch random sequence"
        );
    }
    assert_eq!(store.actor_rig(1).unwrap().render[0].color, completed);
}

fn assert_swell_stage_assignments(scale: bool) {
    let store = pack_swell_fixture_with(
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
            compiled.molang_symbols = symbols.into_boxed_slice();
            compiled.molang_ops = vec![
                MolangOp::LoadQuery(1),
                MolangOp::LoadVariable(2),
                MolangOp::Multiply,
                MolangOp::Push(scalar(2.0)),
                MolangOp::StoreVariable(2),
                MolangOp::Push(scalar(if scale { 1.0 } else { 0.0 })),
                MolangOp::Push(scalar(1.0)),
                MolangOp::StoreVariable(2),
                MolangOp::Push(scalar(0.0)),
            ]
            .into_boxed_slice();
            compiled.molang_expressions = [(0, 3, 2), (3, 3, 1), (6, 3, 1)]
                .map(
                    |(first_op, op_count, max_stack)| assets::CompiledMolangExpression {
                        first_op,
                        op_count,
                        max_stack,
                    },
                )
                .into();
            compiled.rig_bindings[0].pre_animation = Some(2);
            if scale {
                compiled.rig_bindings[0].scale_expressions = Some([1; 4]);
            } else {
                compiled.animation_clips[0].anim_time_update = Some(1);
            }
        },
    );
    for alpha in [0.25, 0.5, 1.0] {
        let layers = store.render_frame(alpha).layers(1).unwrap();
        let expected = -2.0 * (2.0 + alpha) / crate::actor_store::creeper::SWELL_FULL_TICKS;
        for pose in [&layers[0].previous_pose[0], &layers[0].pose[0]] {
            assert!(
                (pose.translation_scale[0] - expected).abs() < 1e-6,
                "assignments preceding bones retain their authored stage"
            );
        }
    }
}

#[test]
fn swell_channels_retain_assignments_from_ordinary_clocks() {
    assert_swell_stage_assignments(false);
}

#[test]
fn swell_channels_retain_assignments_from_rig_scale() {
    assert_swell_stage_assignments(true);
}

#[test]
fn swell_controller_effects_stay_at_their_authored_reference_when_an_earlier_gate_activates() {
    let store = swell_controller_fixture_with(3, 0.12, false, |compiled| {
        let scalar = |value| assets::EntityGeometryScalar::new(value).unwrap();
        let mut symbols = compiled.molang_symbols.to_vec();
        symbols.push(assets::MolangSymbol {
            kind: assets::MolangSymbolKind::Variable,
            identifier: "variable.factor".into(),
        });
        compiled.molang_symbols = symbols.into();
        let mut ops = compiled.molang_ops.to_vec();
        ops.extend([
            MolangOp::Push(scalar(0.0)),
            MolangOp::Push(scalar(1.0)),
            MolangOp::StoreVariable(5),
            MolangOp::Push(scalar(0.0)),
            MolangOp::Push(scalar(3.0)),
            MolangOp::StoreVariable(5),
            MolangOp::Push(scalar(0.0)),
            MolangOp::LoadQuery(4),
            MolangOp::Push(scalar(0.08)),
            MolangOp::Greater,
            MolangOp::LoadVariable(5),
        ]);
        compiled.molang_ops = ops.into();
        let mut expressions = compiled.molang_expressions.to_vec();
        expressions.extend(
            [(7, 1, 1), (8, 3, 1), (11, 3, 1), (14, 3, 2), (17, 1, 1)].map(
                |(first_op, op_count, max_stack)| assets::CompiledMolangExpression {
                    first_op,
                    op_count,
                    max_stack,
                },
            ),
        );
        compiled.molang_expressions = expressions.into();
        compiled.rig_bindings[0].pre_animation = Some(4);
        compiled.controller_states[1].on_entry = Some(5);
        for animation in &mut compiled.controller_animations {
            animation.weight = Some(3);
        }
        compiled.rig_controllers = vec![
            assets::EntityRigControllerBinding {
                name: 0,
                controller: 0,
                weight: Some(6),
                order: 0,
            },
            assets::EntityRigControllerBinding {
                name: 1,
                controller: 0,
                weight: None,
                order: 2,
            },
        ]
        .into();
        compiled.rig_animations = vec![assets::EntityRigAnimationBinding {
            name: 2,
            clip: 0,
            weight: Some(7),
            order: 1,
        }]
        .into();
        compiled.rig_geometries[0].controller_count = 2;
        compiled.rig_geometries[0].animation_count = 1;
    });
    for alpha in [0.5, 0.75, 1.0] {
        let layers = store.render_frame(alpha).layers(1).unwrap();
        let expected = -(2.0 + alpha) / crate::actor_store::creeper::SWELL_FULL_TICKS;
        for pose in [&layers[0].previous_pose[0], &layers[0].pose[0]] {
            assert!(
                (pose.translation_scale[0] - expected).abs() < 1e-6,
                "a newly selected reference cannot consume a later reference's completed event"
            );
        }
    }
}

#[test]
fn swell_render_random_preserves_frame_pre_animation_draws() {
    let store = pack_swell_fixture_with(
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
            compiled.molang_symbols = [
                (assets::MolangSymbolKind::Name, "wield"),
                (assets::MolangSymbolKind::Query, "query.frame_alpha"),
                (assets::MolangSymbolKind::Query, "query.swell_amount"),
            ]
            .map(|(kind, identifier)| assets::MolangSymbol {
                kind,
                identifier: identifier.into(),
            })
            .into();
            compiled.molang_ops = vec![
                MolangOp::LoadQuery(1),
                MolangOp::JumpIfFalse(6),
                MolangOp::Push(scalar(0.0)),
                MolangOp::Push(scalar(1.0)),
                MolangOp::Call(assets::MolangFunction::Random),
                MolangOp::Pop,
                MolangOp::Push(scalar(0.0)),
                MolangOp::LoadQuery(2),
                MolangOp::Push(scalar(0.0)),
                MolangOp::Push(scalar(1.0)),
                MolangOp::Call(assets::MolangFunction::Random),
            ]
            .into();
            compiled.molang_expressions = [(0, 7, 2), (7, 1, 1), (8, 3, 2)]
                .map(
                    |(first_op, op_count, max_stack)| assets::CompiledMolangExpression {
                        first_op,
                        op_count,
                        max_stack,
                    },
                )
                .into();
            compiled.rig_bindings[0].pre_animation = Some(0);
            compiled.animation_keyframes[0].expressions = [Some(1), None, None];
            compiled.render.layers[0].color = Some([2; 4]);
        },
    );
    let completed = store.actor_rig(1).unwrap().render[0].color;
    for alpha in [0.25, 0.5, 0.75] {
        let layers = store.render_frame(alpha).layers(1).unwrap();
        assert_eq!(
            &layers[0].color[..3],
            &completed[1..],
            "the pre-animation frame draw precedes body and render effects"
        );
    }
    assert_eq!(store.actor_rig(1).unwrap().render[0].color, completed);
}

#[test]
fn swell_activated_paused_clock_resumes_from_its_completed_time() {
    let store = pack_swell_fixture_with(
        AuthoredSwellChannel {
            pre_animation: false,
            property: assets::EntityAnimationProperty::Translation,
            variable: false,
        },
        None,
        false,
        3,
        true,
        |compiled| {
            let scalar = |value| assets::EntityGeometryScalar::new(value).unwrap();
            compiled.molang_symbols = [
                (assets::MolangSymbolKind::Name, "wield"),
                (assets::MolangSymbolKind::Query, "query.anim_time"),
                (assets::MolangSymbolKind::Query, "query.swell_amount"),
            ]
            .map(|(kind, identifier)| assets::MolangSymbol {
                kind,
                identifier: identifier.into(),
            })
            .into();
            compiled.molang_ops = vec![
                MolangOp::LoadQuery(1),
                MolangOp::LoadQuery(2),
                MolangOp::Add,
                MolangOp::LoadQuery(2),
                MolangOp::Push(scalar(0.05)),
                MolangOp::LessEqual,
                MolangOp::LoadQuery(2),
                MolangOp::Push(scalar(0.08)),
                MolangOp::Greater,
                MolangOp::Add,
            ]
            .into();
            compiled.molang_expressions = [(0, 3, 2), (3, 7, 3)]
                .map(
                    |(first_op, op_count, max_stack)| assets::CompiledMolangExpression {
                        first_op,
                        op_count,
                        max_stack,
                    },
                )
                .into();
            compiled.rig_animations[0].weight = Some(1);
        },
    );
    let completed_time = -store.actor_rig(1).unwrap().previous[0].translation_scale[0];
    assert!(completed_time > 0.0);
    for alpha in [0.5, 0.75, 1.0] {
        let layers = store.render_frame(alpha).layers(1).unwrap();
        let expected =
            -(completed_time + (2.0 + alpha) / crate::actor_store::creeper::SWELL_FULL_TICKS);
        assert!(
            (layers[0].pose[0].translation_scale[0] - expected).abs() < 1e-6,
            "a paused clock starts after its last committed increment"
        );
    }
    assert_eq!(
        -store.actor_rig(1).unwrap().previous[0].translation_scale[0],
        completed_time
    );
}

#[test]
fn swell_controlled_assignment_taints_later_unconditional_bone_channels() {
    let store = pack_swell_fixture_with(
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
            compiled.molang_symbols = [
                (assets::MolangSymbolKind::Name, "other"),
                (assets::MolangSymbolKind::Name, "wield"),
                (assets::MolangSymbolKind::Query, "query.swell_amount"),
                (assets::MolangSymbolKind::Variable, "variable.factor"),
            ]
            .map(|(kind, identifier)| assets::MolangSymbol {
                kind,
                identifier: identifier.into(),
            })
            .into();
            compiled.molang_ops = vec![
                MolangOp::Push(scalar(2.0)),
                MolangOp::StoreVariable(3),
                MolangOp::Push(scalar(0.0)),
                MolangOp::LoadVariable(3),
                MolangOp::LoadQuery(2),
                MolangOp::Push(scalar(1.5 / crate::actor_store::creeper::SWELL_FULL_TICKS)),
                MolangOp::Greater,
                MolangOp::Push(scalar(0.0)),
                MolangOp::StoreVariable(3),
                MolangOp::Push(scalar(0.0)),
            ]
            .into();
            compiled.molang_expressions = [(0, 3, 1), (3, 1, 1), (4, 3, 2), (7, 3, 1)]
                .map(
                    |(first_op, op_count, max_stack)| assets::CompiledMolangExpression {
                        first_op,
                        op_count,
                        max_stack,
                    },
                )
                .into();
            compiled.rig_bindings[0].pre_animation = Some(3);
            let mut symbols = compiled.symbols.to_vec();
            symbols.insert(
                3,
                assets::EntityAssetSymbol {
                    kind: assets::EntityAssetKind::Animation,
                    identifier: "animation.second".into(),
                    source_index: symbols[2].source_index,
                    dependencies: Box::new([]),
                },
            );
            compiled.symbols = symbols.into();
            compiled.rig_bindings[0].render_controller += 1;
            let mut child = compiled.geometries[0].bones[0].clone();
            child.name = "second".into();
            child.parent = None;
            compiled.geometries[0].bones =
                vec![compiled.geometries[0].bones[0].clone(), child].into();
            let clip = compiled.animation_clips[0];
            compiled.animation_clips = vec![
                clip,
                assets::EntityAnimationClip {
                    symbol: 3,
                    first_channel: 1,
                    ..clip
                },
            ]
            .into();
            let channel = compiled.animation_channels[0].clone();
            compiled.animation_channels = vec![
                channel.clone(),
                assets::EntityAnimationChannel {
                    bone: 1,
                    first_keyframe: 1,
                    ..channel
                },
            ]
            .into();
            let key = compiled.animation_keyframes[0];
            compiled.animation_keyframes = vec![
                key,
                assets::EntityAnimationKeyframe {
                    expressions: [Some(1), None, None],
                    ..key
                },
            ]
            .into();
            compiled.rig_animations = vec![
                assets::EntityRigAnimationBinding {
                    name: 0,
                    clip: 0,
                    weight: Some(2),
                    order: 0,
                },
                assets::EntityRigAnimationBinding {
                    name: 1,
                    clip: 1,
                    weight: None,
                    order: 1,
                },
            ]
            .into();
            compiled.rig_geometries[0].animation_count = 2;
        },
    );
    let layers = store.render_frame(0.5).layers(1).unwrap();
    for pose in [&layers[0].previous_pose[1], &layers[0].pose[1]] {
        assert!(
            (pose.translation_scale[0] + 2.0).abs() < 1e-6,
            "swell-controlled writes reach later independently weighted clips"
        );
    }
}

#[test]
fn swell_pre_animation_samples_presentation_alpha_while_retaining_ordinary_queries() {
    let store = pack_swell_fixture_with(
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
            compiled.molang_symbols = [
                (assets::MolangSymbolKind::Name, "wield"),
                (assets::MolangSymbolKind::Query, "query.frame_alpha"),
                (assets::MolangSymbolKind::Query, "query.swell_amount"),
                (assets::MolangSymbolKind::Variable, "variable.frame"),
            ]
            .map(|(kind, identifier)| assets::MolangSymbol {
                kind,
                identifier: identifier.into(),
            })
            .into();
            compiled.molang_ops = vec![
                MolangOp::LoadQuery(1),
                MolangOp::StoreVariable(3),
                MolangOp::Push(scalar(0.0)),
                MolangOp::LoadQuery(2),
                MolangOp::LoadVariable(3),
                MolangOp::Multiply,
            ]
            .into();
            compiled.molang_expressions = [(0, 3, 1), (3, 3, 2)]
                .map(
                    |(first_op, op_count, max_stack)| assets::CompiledMolangExpression {
                        first_op,
                        op_count,
                        max_stack,
                    },
                )
                .into();
            compiled.rig_bindings[0].pre_animation = Some(0);
            compiled.animation_keyframes[0].expressions = [Some(1), None, None];
        },
    );
    for alpha in [0.25, 0.5, 0.75] {
        let layers = store.render_frame(alpha).layers(1).unwrap();
        let expected = -alpha * (2.0 + alpha) / crate::actor_store::creeper::SWELL_FULL_TICKS;
        for pose in [&layers[0].previous_pose[0], &layers[0].pose[0]] {
            assert!(
                (pose.translation_scale[0] - expected).abs() < 1e-6,
                "presentation alpha is fresh for each ordinary motion endpoint"
            );
        }
    }
}

#[test]
fn swell_controlled_random_draws_taint_later_random_channels() {
    let store = pack_swell_fixture_with(
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
            compiled.molang_symbols = [
                (assets::MolangSymbolKind::Name, "wield"),
                (assets::MolangSymbolKind::Query, "query.swell_amount"),
                (assets::MolangSymbolKind::Variable, "variable.random"),
            ]
            .map(|(kind, identifier)| assets::MolangSymbol {
                kind,
                identifier: identifier.into(),
            })
            .into();
            compiled.molang_ops = vec![
                MolangOp::LoadQuery(1),
                MolangOp::Push(scalar(0.08)),
                MolangOp::Greater,
                MolangOp::JumpIfFalse(8),
                MolangOp::Push(scalar(0.0)),
                MolangOp::Push(scalar(1.0)),
                MolangOp::Call(assets::MolangFunction::Random),
                MolangOp::Pop,
                MolangOp::Push(scalar(0.0)),
                MolangOp::Push(scalar(1.0)),
                MolangOp::Call(assets::MolangFunction::Random),
                MolangOp::StoreVariable(2),
                MolangOp::LoadVariable(2),
                MolangOp::LoadVariable(2),
            ]
            .into();
            compiled.molang_expressions = [(0, 13, 2), (13, 1, 1)]
                .map(
                    |(first_op, op_count, max_stack)| assets::CompiledMolangExpression {
                        first_op,
                        op_count,
                        max_stack,
                    },
                )
                .into();
            compiled.rig_bindings[0].pre_animation = Some(0);
            compiled.animation_keyframes[0].expressions = [Some(1), None, None];
            compiled.render.layers[0].color = Some([1; 4]);
        },
    );
    let completed = store.actor_rig(1).unwrap().current[0].translation_scale[0];
    for alpha in [0.5, 0.75, 1.0, 0.5] {
        let layers = store.render_frame(alpha).layers(1).unwrap();
        assert!(
            (layers[0].pose[0].translation_scale[0] + layers[0].color[0]).abs() < 1e-6,
            "body and render consume the same conditional random sequence"
        );
    }
    assert_eq!(
        store.actor_rig(1).unwrap().current[0].translation_scale[0],
        completed
    );
}

#[test]
fn swell_selected_geometry_synthesizes_both_ordinary_motion_endpoints() {
    let store = pack_swell_fixture_with(
        AuthoredSwellChannel {
            pre_animation: false,
            property: assets::EntityAnimationProperty::Translation,
            variable: false,
        },
        None,
        true,
        3,
        false,
        |compiled| {
            let scalar = |value| assets::EntityGeometryScalar::new(value).unwrap();
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
            compiled.molang_ops = vec![
                MolangOp::LoadQuery(2),
                MolangOp::LoadQuery(1),
                MolangOp::LoadQuery(2),
                MolangOp::Push(scalar(0.08)),
                MolangOp::Greater,
            ]
            .into();
            compiled.molang_expressions = [(0, 1, 1), (1, 1, 1), (2, 3, 2)]
                .map(
                    |(first_op, op_count, max_stack)| assets::CompiledMolangExpression {
                        first_op,
                        op_count,
                        max_stack,
                    },
                )
                .into();
            for key in &mut compiled.animation_keyframes {
                key.expressions = [Some(0), Some(1), None];
            }
            compiled.render.layers[1].condition = Some(2);
        },
    );
    assert_eq!(store.actor_rig(1).unwrap().render.len(), 1);
    for alpha in [0.5, 0.75, 1.0, 0.5] {
        let layers = store.render_frame(alpha).layers(1).unwrap();
        assert_eq!(layers.len(), 2);
        for (pose, tick) in [
            (&layers[1].previous_pose[1], 2.0),
            (&layers[1].pose[1], 3.0),
        ] {
            assert!(
                (pose.translation_scale[1] - tick * ACTOR_TICK_DURATION.as_secs_f32()).abs() < 1e-6,
                "new geometry retains both ordinary motion endpoints: actual {} tick {}",
                pose.translation_scale[1],
                tick,
            );
        }
    }
}
#[test]
fn swell_controller_entry_effects_survive_reference_deactivation() {
    assert_controller_entry_effects_survive_deactivation(false);
}

#[test]
fn swell_controller_entry_effects_survive_nested_reference_deactivation() {
    assert_controller_entry_effects_survive_deactivation(true);
}

fn assert_controller_entry_effects_survive_deactivation(nested: bool) {
    let store = swell_controller_fixture_with(3, 0.12, false, |compiled| {
        let scalar = |value| assets::EntityGeometryScalar::new(value).unwrap();
        let mut symbols = compiled.molang_symbols.to_vec();
        symbols.push(assets::MolangSymbol {
            kind: assets::MolangSymbolKind::Variable,
            identifier: "variable.factor".into(),
        });
        compiled.molang_symbols = symbols.into();
        let mut ops = compiled.molang_ops.to_vec();
        ops.extend([
            MolangOp::Push(scalar(0.0)),
            MolangOp::Push(scalar(1.0)),
            MolangOp::StoreVariable(5),
            MolangOp::Push(scalar(0.0)),
            MolangOp::Push(scalar(3.0)),
            MolangOp::StoreVariable(5),
            MolangOp::Push(scalar(0.0)),
            MolangOp::LoadQuery(4),
            MolangOp::Push(scalar(0.08)),
            MolangOp::Less,
            MolangOp::LoadVariable(5),
        ]);
        compiled.molang_ops = ops.into();
        let mut expressions = compiled.molang_expressions.to_vec();
        expressions.extend(
            [(7, 1, 1), (8, 3, 1), (11, 3, 1), (14, 3, 2), (17, 1, 1)].map(
                |(first_op, op_count, max_stack)| assets::CompiledMolangExpression {
                    first_op,
                    op_count,
                    max_stack,
                },
            ),
        );
        compiled.molang_expressions = expressions.into();
        compiled.rig_bindings[0].pre_animation = Some(4);
        compiled.controller_states[1].on_entry = Some(5);
        for animation in &mut compiled.controller_animations {
            animation.weight = Some(3);
        }
        compiled.rig_controllers = vec![
            assets::EntityRigControllerBinding {
                name: 0,
                controller: 0,
                weight: Some(6),
                order: 0,
            },
            assets::EntityRigControllerBinding {
                name: 1,
                controller: 0,
                weight: None,
                order: 2,
            },
        ]
        .into();
        compiled.rig_animations = vec![assets::EntityRigAnimationBinding {
            name: 2,
            clip: 0,
            weight: Some(7),
            order: 1,
        }]
        .into();
        compiled.rig_geometries[0].controller_count = 2;
        compiled.rig_geometries[0].animation_count = 1;
        if nested {
            let mut symbols = compiled.symbols.to_vec();
            symbols.insert(
                4,
                assets::EntityAssetSymbol {
                    kind: assets::EntityAssetKind::AnimationController,
                    identifier: "controller.animation.wrapper".into(),
                    source_index: 0,
                    dependencies: Box::new([]),
                },
            );
            compiled.symbols = symbols.into();
            compiled.rig_bindings[0].render_controller += 1;
            let mut controllers = compiled.controllers.to_vec();
            controllers.push(assets::EntityAnimationController {
                symbol: 4,
                first_state: 2,
                state_count: 1,
                initial_state: 0,
            });
            compiled.controllers = controllers.into();
            let mut states = compiled.controller_states.to_vec();
            states.push(assets::EntityControllerState {
                name: 0,
                first_animation: 2,
                animation_count: 1,
                first_transition: 1,
                ..Default::default()
            });
            compiled.controller_states = states.into();
            let mut animations = compiled.controller_animations.to_vec();
            animations.push(assets::EntityControllerAnimation {
                target: assets::EntityControllerAnimationTarget::Controller(0),
                weight: Some(6),
            });
            compiled.controller_animations = animations.into();
            compiled.rig_controllers[0].controller = 1;
            compiled.rig_controllers[0].weight = None;
        }
    });
    for alpha in [0.5, 0.75, 1.0] {
        let layers = store.render_frame(alpha).layers(1).unwrap();
        let expected = -(2.0 + alpha) / crate::actor_store::creeper::SWELL_FULL_TICKS;
        for (pose, factor) in [
            (&layers[0].previous_pose[0], 1.0),
            (&layers[0].pose[0], 3.0),
        ] {
            assert!(
                (pose.translation_scale[0] - factor * expected).abs() < 1e-6,
                "completed entry effects remain part of the ordinary endpoint when its reference stops drawing"
            );
        }
    }
}
