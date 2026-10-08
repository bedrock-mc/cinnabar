use super::*;

pub(super) fn compiled_swell_fixture(pre_animation: bool) -> crate::actor_store::ActorStore {
    authored_swell_fixture(
        pre_animation,
        assets::EntityAnimationProperty::Translation,
        false,
    )
}

pub(super) fn authored_swell_fixture(
    pre_animation: bool,
    property: assets::EntityAnimationProperty,
    variable: bool,
) -> crate::actor_store::ActorStore {
    pack_swell_fixture(pre_animation, property, variable, None, false, 3, false)
}

pub(super) fn pack_swell_fixture(
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

pub(super) struct AuthoredSwellChannel {
    pub(super) pre_animation: bool,
    pub(super) property: assets::EntityAnimationProperty,
    pub(super) variable: bool,
}

pub(super) fn pack_swell_fixture_with(
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

pub(super) fn swell_controller_fixture(
    ticks: u32,
    threshold: f32,
    deactivate: bool,
) -> crate::actor_store::ActorStore {
    swell_controller_fixture_with(ticks, threshold, deactivate, |_| {})
}

pub(super) fn swell_controller_fixture_with(
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

pub(super) fn swell_override_fixture(
    relative_child: bool,
    ticks: u32,
) -> crate::actor_store::ActorStore {
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
