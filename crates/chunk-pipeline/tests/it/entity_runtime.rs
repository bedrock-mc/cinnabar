use std::sync::Arc;

use assets::{
    CompiledEntityAssets, CompiledMolangExpression, EntityAnimationChannel, EntityAnimationClip,
    EntityAnimationController, EntityAnimationInterpolation, EntityAnimationKeyframe,
    EntityAnimationLoop, EntityAnimationProperty, EntityAssetKind, EntityAssetSource,
    EntityAssetSymbol, EntityControllerAnimation, EntityControllerState,
    EntityControllerTransition, EntityDependency, EntityDependencyKind, EntityDependencyResolution,
    EntityGeometry, EntityGeometryBone, EntityGeometryInheritance, EntityGeometryScalar,
    EntityRigBinding, EntityRigControllerBinding, EntityRigFallback, EntityRigGeometryBinding,
    MolangCollection, MolangCollectionItem, MolangOp, MolangSymbol, MolangSymbolKind,
    RuntimeAssets, RuntimeEntityAssets, encode_entity_blob,
};
use chunk_pipeline::WorldStream;
use client_world::{
    ActorAnimationView, MAX_ACTOR_ACTION_HISTORY, MAX_CONTROLLER_TRANSITIONS_PER_TICK,
    MAX_MOLANG_OPS_PER_ACTOR_TICK, MAX_MOLANG_OPS_PER_RENDER_FRAME, MAX_MOLANG_OPS_PER_WORLD_TICK,
    MAX_RUNTIME_BONES_PER_RIG,
};
use protocol::{
    ActorEvent, ActorKind, ActorLinkEvent, ActorLinkType, ActorMetadata, ActorMetadataUpdateEvent,
    ActorMetadataValue, ActorMoveEvent, ActorPositionOrigin, ActorSpawnEvent, ChangeDimensionEvent,
    WorldBootstrap, WorldEvent,
};

fn scalar(value: f32) -> EntityGeometryScalar {
    EntityGeometryScalar::new(value).unwrap()
}

fn compiled_entity_assets(fallback: EntityRigFallback) -> CompiledEntityAssets {
    let sources = [
        "animation_controllers/bee.json",
        "animations/bee.json",
        "entity/bee.json",
        "models/entity/bee.json",
        "render_controllers/bee.json",
    ]
    .into_iter()
    .enumerate()
    .map(|(index, path)| EntityAssetSource {
        path: path.into(),
        source_bytes: 1,
        source_sha256: [index as u8 + 1; 32],
    })
    .collect::<Vec<_>>()
    .into_boxed_slice();
    let symbols = vec![
        EntityAssetSymbol {
            kind: EntityAssetKind::Entity,
            identifier: "minecraft:bee".into(),
            source_index: 2,
            dependencies: Box::new([]),
        },
        EntityAssetSymbol {
            kind: EntityAssetKind::Geometry,
            identifier: "geometry.base".into(),
            source_index: 3,
            dependencies: Box::new([]),
        },
        EntityAssetSymbol {
            kind: EntityAssetKind::Geometry,
            identifier: "geometry.bee".into(),
            source_index: 3,
            dependencies: vec![EntityDependency {
                kind: EntityDependencyKind::Geometry,
                identifier: "geometry.base".into(),
                resolution: EntityDependencyResolution::Catalog,
            }]
            .into_boxed_slice(),
        },
        EntityAssetSymbol {
            kind: EntityAssetKind::Animation,
            identifier: "animation.bee.move".into(),
            source_index: 1,
            dependencies: Box::new([]),
        },
        EntityAssetSymbol {
            kind: EntityAssetKind::AnimationController,
            identifier: "controller.animation.bee".into(),
            source_index: 0,
            dependencies: Box::new([]),
        },
        EntityAssetSymbol {
            kind: EntityAssetKind::RenderController,
            identifier: "controller.render.bee".into(),
            source_index: 4,
            dependencies: Box::new([]),
        },
    ]
    .into_boxed_slice();
    CompiledEntityAssets {
        source_manifest_sha256: [0x44; 32],
        block_visual_count: 1,
        sources,
        symbols,
        geometries: vec![
            EntityGeometry {
                identifier: "geometry.base".into(),
                inherits: None,
                source_index: 3,
                texture_width: 16,
                texture_height: 16,
                bones: vec![EntityGeometryBone {
                    name: "root".into(),
                    binding: None,
                    texture_meshes: Box::new([]),
                    parent: None,
                    pivot: Some([scalar(1.0), scalar(0.0), scalar(0.0)]),
                    rotation: None,
                    bind_pose_rotation: None,
                    mirror: None,
                    inflate: None,
                    never_render: None,
                    reset: None,
                    cubes: Box::new([]),
                }]
                .into_boxed_slice(),
            },
            EntityGeometry {
                identifier: "geometry.bee".into(),
                inherits: Some(EntityGeometryInheritance {
                    identifier: "geometry.base".into(),
                    resolution: EntityDependencyResolution::Catalog,
                }),
                source_index: 3,
                texture_width: 16,
                texture_height: 16,
                bones: vec![EntityGeometryBone {
                    name: "wing".into(),
                    binding: None,
                    texture_meshes: Box::new([]),
                    parent: Some("root".into()),
                    pivot: Some([scalar(0.0), scalar(2.0), scalar(0.0)]),
                    rotation: None,
                    bind_pose_rotation: None,
                    mirror: None,
                    inflate: None,
                    never_render: None,
                    reset: None,
                    cubes: Box::new([]),
                }]
                .into_boxed_slice(),
            },
        ]
        .into_boxed_slice(),
        animation_clips: vec![EntityAnimationClip {
            symbol: 3,
            length_seconds: scalar(1.0),
            loop_mode: EntityAnimationLoop::Loop,
            first_channel: 0,
            channel_count: 1,
            source: 1,
            override_previous: false,
            anim_time_update: None,
            geometry: None,
        }]
        .into_boxed_slice(),
        animation_channels: vec![EntityAnimationChannel {
            bone: 1,
            property: EntityAnimationProperty::Translation,
            first_keyframe: 0,
            keyframe_count: 2,
            rotation_relative_to_entity: false,
        }]
        .into_boxed_slice(),
        animation_keyframes: vec![
            EntityAnimationKeyframe {
                time_seconds: scalar(0.0),
                value: [scalar(0.0), scalar(0.0), scalar(0.0)],
                interpolation: EntityAnimationInterpolation::Linear,
                expressions: [None; 3],
            },
            EntityAnimationKeyframe {
                time_seconds: scalar(0.1),
                value: [scalar(2.0), scalar(0.0), scalar(0.0)],
                interpolation: EntityAnimationInterpolation::Linear,
                expressions: [None; 3],
            },
        ]
        .into_boxed_slice(),
        molang_symbols: vec![
            MolangSymbol {
                kind: MolangSymbolKind::Name,
                identifier: "move".into(),
            },
            MolangSymbol {
                kind: MolangSymbolKind::Name,
                identifier: "moving".into(),
            },
            MolangSymbol {
                kind: MolangSymbolKind::Query,
                identifier: "query.ground_speed".into(),
            },
        ]
        .into_boxed_slice(),
        molang_expressions: vec![CompiledMolangExpression {
            first_op: 0,
            op_count: 1,
            max_stack: 1,
        }]
        .into_boxed_slice(),
        molang_ops: vec![MolangOp::LoadQuery(2)].into_boxed_slice(),
        molang_collections: Box::new([]),
        molang_collection_items: Box::new([]),
        controllers: vec![EntityAnimationController {
            symbol: 4,
            first_state: 0,
            state_count: 2,
            initial_state: 0,
        }]
        .into_boxed_slice(),
        controller_states: vec![
            EntityControllerState {
                name: 0,
                first_animation: 0,
                animation_count: 0,
                first_transition: 0,
                transition_count: 1,
                on_entry: None,
                on_exit: None,
                ..Default::default()
            },
            EntityControllerState {
                name: 1,
                first_animation: 0,
                animation_count: 1,
                first_transition: 1,
                transition_count: 0,
                on_entry: None,
                on_exit: None,
                ..Default::default()
            },
        ]
        .into_boxed_slice(),
        controller_animations: vec![EntityControllerAnimation {
            target: assets::EntityControllerAnimationTarget::Clip(0),
            weight: None,
        }]
        .into_boxed_slice(),
        controller_transitions: vec![EntityControllerTransition {
            target_state: 1,
            condition: 0,
        }]
        .into_boxed_slice(),
        rig_bindings: vec![EntityRigBinding {
            entity_symbol: 0,
            render_controller: 5,
            first_geometry: 0,
            geometry_count: 1,
            fallback,
            initialize: None,
            pre_animation: None,
            scale: assets::EntityGeometryScalar::new(1.0).unwrap(),
            scale_expressions: None,
        }]
        .into_boxed_slice(),
        rig_geometries: vec![EntityRigGeometryBinding {
            geometry: 1,
            condition: None,
            first_animation: 0,
            animation_count: 0,
            first_controller: 0,
            controller_count: 1,
        }]
        .into_boxed_slice(),
        rig_animations: Box::new([]),
        rig_controllers: vec![EntityRigControllerBinding {
            name: 0,
            controller: 0,
            weight: None,
            order: 0,
        }]
        .into_boxed_slice(),
        item_visuals: Box::new([]),
        item_visual_aliases: Box::new([]),
        render: Default::default(),
    }
}

fn decode_entity_assets(compiled: &CompiledEntityAssets) -> Arc<RuntimeEntityAssets> {
    let blob = encode_entity_blob(compiled).unwrap();
    Arc::new(RuntimeEntityAssets::decode(&blob).unwrap())
}

fn entity_assets(fallback: EntityRigFallback) -> Arc<RuntimeEntityAssets> {
    decode_entity_assets(&compiled_entity_assets(fallback))
}

fn bootstrap() -> WorldBootstrap {
    WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0, 64.0, 0.0],
        world_spawn_position: [0, 64, 0],
        air_network_id: 0,
        block_network_ids_are_hashes: false,
    }
}

fn spawn(runtime_id: u64, unique_id: i64, velocity: [f32; 3]) -> WorldEvent {
    WorldEvent::Actor(ActorEvent::Spawn(ActorSpawnEvent {
        dimension: 0,
        unique_id,
        runtime_id,
        kind: ActorKind::Entity {
            identifier: "minecraft:bee".into(),
        },
        position: [0.0, 64.0, 0.0],
        velocity,
        pitch: 0.0,
        yaw: 0.0,
        head_yaw: 0.0,
        body_yaw: 0.0,
        held_item: Default::default(),
        metadata: Arc::from([]),
        attributes: Arc::from([]),
        properties: Arc::from([]),
        links: Arc::from([]),
    }))
}

fn stream(fallback: EntityRigFallback) -> WorldStream {
    stream_with_entity_assets(entity_assets(fallback))
}

fn stream_with_entity_assets(entity_assets: Arc<RuntimeEntityAssets>) -> WorldStream {
    WorldStream::new_with_asset_sets(
        bootstrap(),
        Arc::new(RuntimeAssets::diagnostic()),
        entity_assets,
        [0.0, 64.0, 0.0],
        None,
    )
}

#[test]
fn runtime_budgets_are_the_reviewed_exact_ceilings() {
    assert_eq!(MAX_RUNTIME_BONES_PER_RIG, 96);
    assert_eq!(MAX_CONTROLLER_TRANSITIONS_PER_TICK, 8);
    assert_eq!(MAX_MOLANG_OPS_PER_ACTOR_TICK, 4_096);
    assert_eq!(MAX_MOLANG_OPS_PER_WORLD_TICK, 262_144);
    assert_eq!(
        MAX_MOLANG_OPS_PER_RENDER_FRAME,
        MAX_MOLANG_OPS_PER_WORLD_TICK
    );
    assert_eq!(MAX_ACTOR_ACTION_HISTORY, 32);
}

#[test]
fn resolves_inherited_rig_and_publishes_adjacent_completed_tick_palettes() {
    let mut stream = stream(EntityRigFallback::Skip);
    stream.submit(1, spawn(42, -7, [1.0, 0.0, 0.0])).unwrap();

    let initial = stream.authority().actor_rig(42).unwrap();
    assert_eq!(initial.actor.runtime_id, 42);
    assert_eq!(initial.actor.spawn_revision, 1);
    assert_eq!(initial.previous, initial.current);
    assert_eq!(initial.rest, initial.current);
    let rest = initial.rest.to_vec();
    assert_eq!(initial.current.len(), 2);
    assert_eq!(initial.completed_tick, 0);
    assert_eq!(initial.current[1].translation_scale[0..3], [0.0, 2.0, 0.0]);

    // The first tick enters the animated state, whose clip starts at time zero.
    stream.advance_actor_interpolation_ticks(2);
    let tick = stream.authority().actor_rig(42).unwrap();
    assert_eq!(tick.completed_tick, 2);
    assert_eq!(tick.previous[1].translation_scale[0..3], [0.0, 2.0, 0.0]);
    // The rig frame mirrors authored X, so the +X clip offset lands at -X.
    assert_eq!(tick.current[1].translation_scale[0..3], [-1.0, 2.0, 0.0]);
    assert_eq!(tick.current[1].translation_scale[3], 1.0);
    assert_eq!(tick.rest, rest);
    assert_ne!(tick.rest, tick.current);
}

#[test]
fn cached_rest_uses_resolved_rotated_inheritance_not_the_latest_animation() {
    let mut compiled = compiled_entity_assets(EntityRigFallback::Skip);
    compiled.geometries[0].bones[0].rotation = Some([scalar(0.0), scalar(0.0), scalar(90.0)]);
    let mut stream = stream_with_entity_assets(decode_entity_assets(&compiled));
    stream.submit(1, spawn(42, -7, [1.0, 0.0, 0.0])).unwrap();
    let rest = stream.authority().actor_rig(42).unwrap().rest.to_vec();
    stream.advance_actor_interpolation_ticks(2);
    let rig = stream.authority().actor_rig(42).unwrap();
    assert_eq!(rig.rest, rest);
    assert_ne!(rig.current, rest);
    assert_ne!(rest[0].rotation, [0.0, 0.0, 0.0, 1.0]);
}

#[test]
fn geometry_only_fallback_retains_static_pose_and_attribution() {
    let mut stream = stream(EntityRigFallback::GeometryOnly);
    stream.submit(1, spawn(42, -7, [1.0, 0.0, 0.0])).unwrap();
    stream.advance_actor_interpolation_ticks(2);
    let rig = stream.authority().actor_rig(42).unwrap();
    assert_eq!(rig.fallback, EntityRigFallback::GeometryOnly);
    assert_eq!(rig.previous, rig.current);
    assert_eq!(rig.current[1].translation_scale[0..3], [0.0, 2.0, 0.0]);
}

#[test]
fn teleport_incompatible_metadata_and_replacement_reset_both_palettes() {
    let mut stream = stream(EntityRigFallback::Skip);
    stream.submit(1, spawn(42, -7, [1.0, 0.0, 0.0])).unwrap();
    stream.advance_actor_interpolation_ticks(1);
    let generation = stream.authority().actor_rig(42).unwrap().reset_generation;
    let rest = stream.authority().actor_rig(42).unwrap().rest.to_vec();

    stream
        .submit(
            2,
            WorldEvent::Actor(ActorEvent::Move(ActorMoveEvent {
                dimension: 0,
                runtime_id: 42,
                position: [Some(100.0), None, None],
                position_origin: ActorPositionOrigin::Feet,
                pitch: None,
                yaw: None,
                head_yaw: None,
                on_ground: Some(true),
                teleported: true,
                player_mode: None,
                source_tick: Some(2),
                interpolation: Default::default(),
            })),
        )
        .unwrap();
    stream.advance_actor_interpolation_ticks(1);
    let teleported = stream.authority().actor_rig(42).unwrap();
    assert!(teleported.reset_generation > generation);
    assert_eq!(teleported.previous, teleported.current);
    assert_eq!(teleported.rest, rest);
    let generation = teleported.reset_generation;

    for (sequence, value) in [
        (3, ActorMetadataValue::Int(1)),
        (4, ActorMetadataValue::String("changed-type".into())),
    ] {
        stream
            .submit(
                sequence,
                WorldEvent::Actor(ActorEvent::Metadata(ActorMetadataUpdateEvent {
                    dimension: 0,
                    runtime_id: 42,
                    metadata: Arc::from([ActorMetadata { key: 7, value }]),
                    properties: Arc::from([]),
                    tick: sequence,
                })),
            )
            .unwrap();
    }
    stream.advance_actor_interpolation_ticks(1);
    let metadata_reset = stream.authority().actor_rig(42).unwrap();
    assert!(metadata_reset.reset_generation > generation);
    assert_eq!(metadata_reset.previous, metadata_reset.current);
    assert_eq!(metadata_reset.rest, rest);
    let old_lifetime = metadata_reset.actor;
    let metadata_reset_generation = metadata_reset.reset_generation;

    stream.submit(5, spawn(42, -8, [0.0; 3])).unwrap();
    let replacement = stream.authority().actor_rig(42).unwrap();
    assert_ne!(replacement.actor, old_lifetime);
    assert!(replacement.reset_generation > metadata_reset_generation);
    assert_eq!(replacement.previous, replacement.current);
    assert_eq!(replacement.rest, rest);
}

#[test]
fn missing_required_rig_produces_no_stale_snapshot() {
    let mut stream = stream(EntityRigFallback::Skip);
    let mut missing = match spawn(77, -9, [0.0; 3]) {
        WorldEvent::Actor(ActorEvent::Spawn(spawn)) => spawn,
        _ => unreachable!(),
    };
    missing.kind = ActorKind::Entity {
        identifier: "minecraft:missing".into(),
    };
    stream
        .submit(1, WorldEvent::Actor(ActorEvent::Spawn(missing)))
        .unwrap();
    assert!(stream.authority().actor_rig(77).is_none());
    assert!(stream.authority().actor_rigs().next().is_none());
    assert_eq!(
        stream.authority().actor_animation_stats().unrigged_spawns,
        1
    );
}

/// An actor outside the animation view holds its pose; back in view it resumes from its new
/// pose without blending from the held one.
#[test]
fn rigs_outside_the_animation_view_hold_their_pose_and_resume_unblended() {
    let mut stream = stream(EntityRigFallback::Skip);
    stream.submit(1, spawn(42, -7, [1.0, 0.0, 0.0])).unwrap();
    stream.advance_actor_interpolation_ticks(2);
    let held = stream.authority().actor_rig(42).unwrap().current.to_vec();
    stream.set_actor_animation_view(Some(ActorAnimationView {
        planes: [[1.0, 0.0, 0.0, -100.0]; 6],
        camera: [0.0, 64.0, 0.0],
        player_distance: 192.0,
        entity_radius: 72.0,
    }));
    stream.advance_actor_interpolation_ticks(5);
    let rig = stream.authority().actor_rig(42).unwrap();
    assert_eq!(
        (rig.previous, rig.current),
        (held.as_slice(), held.as_slice())
    );

    stream.set_actor_animation_view(None);
    stream.advance_actor_interpolation_ticks(1);
    let rig = stream.authority().actor_rig(42).unwrap();
    assert_ne!(rig.current, held.as_slice());
    assert_eq!(rig.previous, rig.current);
}

#[test]
fn animation_time_is_lifetime_relative_and_looped() {
    let mut stream = stream(EntityRigFallback::Skip);
    stream.advance_actor_interpolation_ticks(10);
    stream.submit(1, spawn(42, -7, [1.0, 0.0, 0.0])).unwrap();

    stream.advance_actor_interpolation_ticks(2);
    assert_eq!(
        stream.authority().actor_rig(42).unwrap().current[1].translation_scale[0],
        -1.0
    );

    stream.advance_actor_interpolation_ticks(19);
    assert_eq!(
        stream.authority().actor_rig(42).unwrap().current[1].translation_scale[0],
        -2.0,
        "native keeps the loop endpoint at exact equality"
    );
    stream.advance_actor_interpolation_ticks(1);
    assert!(
        (stream.authority().actor_rig(42).unwrap().current[1].translation_scale[0] + 1.0).abs()
            < 1e-5
    );
}

#[test]
fn authoritative_riding_link_reaches_the_next_runtime_tick_query() {
    let mut compiled = compiled_entity_assets(EntityRigFallback::Skip);
    compiled.molang_symbols[2].identifier = "query.is_riding".into();
    let mut stream = stream_with_entity_assets(decode_entity_assets(&compiled));
    stream.submit(1, spawn(42, -7, [0.0; 3])).unwrap();
    stream
        .submit(
            2,
            WorldEvent::ActorLink(ActorLinkEvent {
                dimension: 0,
                ridden_unique_id: -8,
                rider_unique_id: -7,
                link_type: ActorLinkType::Rider,
                immediate: false,
                rider_initiated: false,
            }),
        )
        .unwrap();
    stream.advance_actor_interpolation_ticks(2);
    assert_eq!(
        stream.authority().actor_rig(42).unwrap().current[1].translation_scale[0],
        -1.0
    );
}

#[test]
fn dimension_change_drops_rig_palettes_without_stale_publication() {
    let mut stream = stream(EntityRigFallback::Skip);
    stream.submit(1, spawn(42, -7, [1.0, 0.0, 0.0])).unwrap();
    assert!(stream.authority().actor_rig(42).is_some());

    stream
        .submit(
            2,
            WorldEvent::ChangeDimension(ChangeDimensionEvent {
                dimension: 1,
                position: [0.0, 80.0, 0.0],
                ..Default::default()
            }),
        )
        .unwrap();

    assert!(stream.authority().actor_rig(42).is_none());
    assert!(stream.authority().actor_rigs().next().is_none());
}

#[test]
fn conditioned_geometry_candidates_precede_the_unconditional_fallback() {
    let mut compiled = compiled_entity_assets(EntityRigFallback::Skip);
    let first_op = compiled.molang_ops.len() as u32;
    let mut ops = compiled.molang_ops.into_vec();
    ops.extend([
        MolangOp::LoadQuery(2),
        MolangOp::Push(scalar(0.0)),
        MolangOp::Greater,
    ]);
    compiled.molang_ops = ops.into_boxed_slice();
    let condition = compiled.molang_expressions.len() as u32;
    let mut expressions = compiled.molang_expressions.into_vec();
    expressions.push(CompiledMolangExpression {
        first_op,
        op_count: 3,
        max_stack: 2,
    });
    compiled.molang_expressions = expressions.into_boxed_slice();
    let mut rig_controllers = compiled.rig_controllers.into_vec();
    rig_controllers.push(rig_controllers[0]);
    compiled.rig_controllers = rig_controllers.into_boxed_slice();
    let fallback = compiled.rig_geometries[0];
    compiled.rig_geometries = vec![
        fallback,
        EntityRigGeometryBinding {
            condition: Some(condition),
            first_controller: 1,
            ..fallback
        },
    ]
    .into_boxed_slice();
    compiled.rig_bindings[0].geometry_count = 2;

    let mut stream = stream_with_entity_assets(decode_entity_assets(&compiled));
    stream.submit(1, spawn(42, -7, [1.0, 0.0, 0.0])).unwrap();
    assert_eq!(stream.authority().actor_rig(42).unwrap().rig.0, 1);
}

#[test]
fn reversed_dynamic_clamp_takes_the_lower_bound_instead_of_freezing() {
    let mut compiled = compiled_entity_assets(EntityRigFallback::Skip);
    let first_op = compiled.molang_ops.len() as u32;
    let mut ops = compiled.molang_ops.into_vec();
    ops.extend([
        MolangOp::Push(scalar(1.0)),
        MolangOp::Push(scalar(2.0)),
        MolangOp::Push(scalar(1.0)),
        MolangOp::Call(assets::MolangFunction::Clamp),
    ]);
    compiled.molang_ops = ops.into_boxed_slice();
    let weight = compiled.molang_expressions.len() as u32;
    let mut expressions = compiled.molang_expressions.into_vec();
    expressions.push(CompiledMolangExpression {
        first_op,
        op_count: 4,
        max_stack: 3,
    });
    compiled.molang_expressions = expressions.into_boxed_slice();
    compiled.controller_animations[0].weight = Some(weight);

    let mut stream = stream_with_entity_assets(decode_entity_assets(&compiled));
    stream.submit(1, spawn(42, -7, [1.0, 0.0, 0.0])).unwrap();
    stream.advance_actor_interpolation_ticks(2);
    assert_eq!(stream.authority().actor_rig(42).unwrap().completed_tick, 2);
    assert_eq!(stream.authority().actor_animation_stats().frozen_actors, 0);
    // One tick into the entered state, weight 2 doubles the clip's +1 authored X offset,
    // which the rig frame mirrors.
    assert_eq!(
        stream.authority().actor_rig(42).unwrap().current[1].translation_scale[0],
        -2.0
    );
}

#[test]
fn movement_updates_velocity_queries_and_teleport_restarts_clip_time() {
    let mut stream = stream(EntityRigFallback::Skip);
    stream.submit(1, spawn(42, -7, [0.0; 3])).unwrap();
    stream.advance_actor_interpolation_ticks(1);
    assert_eq!(
        stream.authority().actor_rig(42).unwrap().current[1].translation_scale[0],
        0.0
    );

    stream
        .submit(
            2,
            WorldEvent::Actor(ActorEvent::Move(ActorMoveEvent {
                dimension: 0,
                runtime_id: 42,
                position: [Some(1.0), None, None],
                position_origin: ActorPositionOrigin::Feet,
                pitch: None,
                yaw: None,
                head_yaw: None,
                on_ground: Some(true),
                teleported: false,
                player_mode: None,
                source_tick: Some(2),
                interpolation: Default::default(),
            })),
        )
        .unwrap();
    // The move enters the animated state; its clip then runs from zero.
    stream.advance_actor_interpolation_ticks(3);
    assert_eq!(
        stream.authority().actor_rig(42).unwrap().current[1].translation_scale[0],
        -2.0
    );

    stream
        .submit(
            3,
            WorldEvent::Actor(ActorEvent::Move(ActorMoveEvent {
                dimension: 0,
                runtime_id: 42,
                position: [Some(100.0), None, None],
                position_origin: ActorPositionOrigin::Feet,
                pitch: None,
                yaw: None,
                head_yaw: None,
                on_ground: Some(true),
                teleported: true,
                player_mode: None,
                source_tick: Some(3),
                interpolation: Default::default(),
            })),
        )
        .unwrap();
    stream.advance_actor_interpolation_ticks(1);
    let reset = stream.authority().actor_rig(42).unwrap();
    assert_eq!(reset.previous, reset.current);
    assert_eq!(reset.current[1].translation_scale[0], 0.0);
}

#[test]
fn duplicate_keyframe_post_values_and_collection_indices_are_bounded() {
    let mut compiled = compiled_entity_assets(EntityRigFallback::Skip);
    compiled.animation_channels[0].keyframe_count = 3;
    compiled.animation_keyframes = vec![
        EntityAnimationKeyframe {
            time_seconds: scalar(0.0),
            value: [scalar(0.0), scalar(0.0), scalar(0.0)],
            interpolation: EntityAnimationInterpolation::Linear,
            expressions: [None; 3],
        },
        EntityAnimationKeyframe {
            time_seconds: scalar(0.0),
            value: [scalar(2.0), scalar(0.0), scalar(0.0)],
            interpolation: EntityAnimationInterpolation::Linear,
            expressions: [None; 3],
        },
        EntityAnimationKeyframe {
            time_seconds: scalar(0.1),
            value: [scalar(4.0), scalar(0.0), scalar(0.0)],
            interpolation: EntityAnimationInterpolation::Linear,
            expressions: [None; 3],
        },
    ]
    .into_boxed_slice();
    let first_op = compiled.molang_ops.len() as u32;
    let mut ops = compiled.molang_ops.into_vec();
    ops.extend([MolangOp::Push(scalar(99.0)), MolangOp::SelectCollection(0)]);
    compiled.molang_ops = ops.into_boxed_slice();
    let weight = compiled.molang_expressions.len() as u32;
    let mut expressions = compiled.molang_expressions.into_vec();
    expressions.push(CompiledMolangExpression {
        first_op,
        op_count: 2,
        max_stack: 1,
    });
    compiled.molang_expressions = expressions.into_boxed_slice();
    compiled.controller_animations[0].weight = Some(weight);
    compiled.molang_collections = vec![MolangCollection {
        first_item: 0,
        item_count: 2,
    }]
    .into_boxed_slice();
    compiled.molang_collection_items = vec![
        MolangCollectionItem { value: scalar(1.0) },
        MolangCollectionItem { value: scalar(2.0) },
    ]
    .into_boxed_slice();

    let mut stream = stream_with_entity_assets(decode_entity_assets(&compiled));
    stream.submit(1, spawn(42, -7, [1.0, 0.0, 0.0])).unwrap();
    stream.advance_actor_interpolation_ticks(2);
    // At 0.05 the duplicate time's post value (2) interpolates to 3; index 99 wraps
    // to the second collection weight, which doubles that delta.
    assert_eq!(
        stream.authority().actor_rig(42).unwrap().current[1].translation_scale[0],
        -6.0
    );
}

fn turn(sequence: u64, yaw: f32) -> WorldEvent {
    WorldEvent::Actor(ActorEvent::Move(ActorMoveEvent {
        dimension: 0,
        runtime_id: 42,
        position: [None; 3],
        position_origin: ActorPositionOrigin::Feet,
        pitch: None,
        yaw: Some(yaw),
        head_yaw: Some(yaw),
        on_ground: Some(true),
        teleported: false,
        player_mode: None,
        source_tick: Some(sequence),
        interpolation: Default::default(),
    }))
}

fn with_weight_program(compiled: &mut CompiledEntityAssets, program: Vec<MolangOp>, stack: u8) {
    let first_op = compiled.molang_ops.len() as u32;
    let op_count = program.len() as u16;
    let mut ops = std::mem::take(&mut compiled.molang_ops).into_vec();
    ops.extend(program);
    compiled.molang_ops = ops.into_boxed_slice();
    let weight = compiled.molang_expressions.len() as u32;
    let mut expressions = std::mem::take(&mut compiled.molang_expressions).into_vec();
    expressions.push(CompiledMolangExpression {
        first_op,
        op_count,
        max_stack: stack,
    });
    compiled.molang_expressions = expressions.into_boxed_slice();
    compiled.controller_animations[0].weight = Some(weight);
}

#[test]
fn static_and_failing_rigs_still_turn_their_bodies_toward_the_reported_yaw() {
    let mut failing = compiled_entity_assets(EntityRigFallback::Skip);
    // A NaN weight makes every pose non-finite, so evaluation fails each tick.
    with_weight_program(
        &mut failing,
        vec![
            MolangOp::Push(scalar(-1.0)),
            MolangOp::Call(assets::MolangFunction::Sqrt),
        ],
        1,
    );
    for assets in [
        entity_assets(EntityRigFallback::GeometryOnly),
        decode_entity_assets(&failing),
    ] {
        let mut stream = stream_with_entity_assets(assets);
        stream.submit(1, spawn(42, -7, [0.0; 3])).unwrap();
        stream.submit(2, turn(2, 90.0)).unwrap();
        stream.advance_actor_interpolation_ticks(30);
        let rig = stream.authority().actor_rig(42).unwrap();
        assert!(
            (rig.body_yaw - 90.0).abs() < 1.0e-3,
            "body yaw {}",
            rig.body_yaw
        );
    }
}

#[test]
fn world_budget_starvation_rotates_so_the_same_actors_do_not_always_freeze() {
    let mut compiled = compiled_entity_assets(EntityRigFallback::Skip);
    // One capped loop spends about a thousand operations per actor per tick.
    with_weight_program(
        &mut compiled,
        vec![
            MolangOp::Push(scalar(5_000.0)),
            MolangOp::LoopStart(3),
            MolangOp::LoopNext(2),
            MolangOp::Push(scalar(1.0)),
        ],
        1,
    );
    let mut stream = stream_with_entity_assets(decode_entity_assets(&compiled));
    let actors = 300_u64;
    for runtime_id in 0..actors {
        stream
            .submit(
                runtime_id + 1,
                spawn(100 + runtime_id, -100 - runtime_id as i64, [1.0, 0.0, 0.0]),
            )
            .unwrap();
    }
    stream.advance_actor_interpolation_ticks(1);
    let starved_first = (0..actors)
        .filter(|index| {
            stream
                .authority()
                .actor_rig(100 + index)
                .unwrap()
                .completed_tick
                == 0
        })
        .collect::<Vec<_>>();
    assert!(
        !starved_first.is_empty(),
        "the fixture exhausts the world budget"
    );
    stream.advance_actor_interpolation_ticks(1);
    for index in starved_first {
        assert_eq!(
            stream
                .authority()
                .actor_rig(100 + index)
                .unwrap()
                .completed_tick,
            2,
            "actor {index} starved twice"
        );
    }
}

#[path = "entity_runtime/clip_clock.rs"]
mod clip_clock;
#[path = "entity_runtime/frame_advance.rs"]
mod frame_advance;

#[path = "entity_runtime/math_calls.rs"]
mod math_calls;

#[test]
fn review_custom_action_uses_the_server_pack_rig_catalog() {
    let mut compiled = compiled_entity_assets(EntityRigFallback::Skip);
    compiled.rig_geometries[0].animation_count = 1;
    compiled.rig_animations = vec![assets::EntityRigAnimationBinding {
        name: 0,
        clip: 0,
        weight: None,
        order: 0,
    }]
    .into_boxed_slice();
    let pack = decode_entity_assets(&compiled);
    let mut stream = stream(EntityRigFallback::Skip);
    stream.set_pack_entities(Some((pack, vec![0])));
    stream.submit(1, spawn(42, -7, [0.0; 3])).unwrap();
    assert!(stream.authority().actor_rig(42).unwrap().rig.0 >= assets::PACK_RIG_ID_BASE);
    stream
        .submit(
            2,
            WorldEvent::ItemActor(protocol::ItemActorEvent::Action(
                protocol::ActorActionEvent {
                    actor_runtime_ids: Arc::from([42]),
                    kind: protocol::ActorActionKind::Custom {
                        animation: "animation.bee.move".into(),
                        controller: "controller.animation.bee".into(),
                    },
                    data: 0.0,
                    swing_source: None,
                },
            )),
        )
        .unwrap();
    assert_eq!(
        stream.authority().actor_action(42).unwrap().fallback,
        client_world::RemoteActionFallback::None
    );
}

#[path = "entity_runtime/pose_defaults.rs"]
mod pose_defaults;
