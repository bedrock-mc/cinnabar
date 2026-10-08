use super::*;
use crate::actor_store::{ActorApplyResult, ActorStore};
use protocol::{ActorActionEvent, ActorActionKind, ActorEvent, ActorSpawnEvent, ItemActorEvent};

fn compiled() -> assets::CompiledEntityAssets {
    let mut compiled = super::attachable::tests::compiled_fixture();
    compiled.sources[1].path = "entity/display.json".into();
    compiled.symbols[4].kind = EntityAssetKind::Entity;
    compiled.symbols[4].identifier = "minecraft:test".into();
    compiled.symbols.rotate_right(1);
    compiled.rig_bindings[0].entity_symbol = 0;
    compiled.rig_bindings[0].render_controller = 3;
    compiled.rig_bindings[0].pre_animation = None;
    compiled.animation_clips[0].symbol = 2;
    compiled.animation_clips[0].length_seconds = assets::EntityGeometryScalar::new(2.0).unwrap();
    compiled.animation_keyframes[0].expressions = [None; 3];
    compiled.animation_keyframes[0].value =
        [16.0, 0.0, 0.0].map(|value| assets::EntityGeometryScalar::new(value).unwrap());
    compiled.rig_geometries[0].animation_count = 0;
    compiled.rig_animations = Box::new([]);
    compiled
}

fn assets() -> Arc<RuntimeEntityAssets> {
    Arc::new(RuntimeEntityAssets::from_compiled(compiled()).unwrap())
}

fn unposed_store() -> ActorStore {
    let mut store = ActorStore::new_with_entity_assets(1, 0, assets());
    let actor = super::tests::actor_with_metadata(HashMap::new());
    let spawn = ActorEvent::Spawn(ActorSpawnEvent {
        dimension: 0,
        unique_id: actor.unique_id,
        runtime_id: actor.runtime_id,
        kind: actor.kind,
        position: actor.position,
        velocity: actor.velocity,
        pitch: 0.0,
        yaw: 0.0,
        head_yaw: 0.0,
        body_yaw: 0.0,
        held_item: Default::default(),
        metadata: Arc::from([]),
        attributes: Arc::from([]),
        properties: Arc::from([]),
        links: Arc::from([]),
    });
    assert_eq!(store.apply(1, 1, spawn), ActorApplyResult::Inserted);
    store
}

fn store() -> ActorStore {
    let mut store = unposed_store();
    store.advance_interpolation_ticks(1);
    store
}

fn play(store: &mut ActorStore, sequence: u64) {
    assert_eq!(
        store.apply_item_actor(
            1,
            sequence,
            ItemActorEvent::Action(ActorActionEvent {
                actor_runtime_ids: Arc::from([1]),
                kind: ActorActionKind::Custom {
                    animation: "animation.item".into(),
                    controller: "fixture.server".into(),
                    next_state: "".into(),
                    stop_expression: "".into(),
                    stop_expression_version: 0,
                },
                data: 0.0,
                swing_source: None,
            })
        ),
        ActorApplyResult::Updated
    );
}

#[test]
fn server_animation_named_by_packet_changes_the_pose_without_an_entity_alias() {
    let mut store = store();
    assert_eq!(
        store.actor_rigs().next().unwrap().current[0].translation_scale[0],
        0.0
    );
    play(&mut store, 2);
    store.advance_interpolation_ticks(1);
    assert_eq!(
        store.actor_rigs().next().unwrap().current[0].translation_scale[0],
        -16.0
    );
    assert_eq!(store.animation_stats().frozen_actors, 0);
}

fn compile_stop(source: &str, version: i32) -> Option<assets::MolangProgram> {
    assert_eq!(version, 13);
    let (query, ops) = match source {
        "query.all_animations_finished" => (source, vec![assets::MolangOp::LoadQuery(0)]),
        "query.anim_time >= 0.2" => (
            "query.anim_time",
            vec![
                assets::MolangOp::LoadQuery(0),
                assets::MolangOp::Push(assets::EntityGeometryScalar::new(0.2).unwrap()),
                assets::MolangOp::GreaterEqual,
            ],
        ),
        _ => return None,
    };
    Some(assets::MolangProgram {
        symbols: vec![assets::MolangSymbol {
            kind: assets::MolangSymbolKind::Query,
            identifier: query.into(),
        }]
        .into_boxed_slice(),
        expressions: vec![assets::CompiledMolangExpression {
            first_op: 0,
            op_count: ops.len() as u16,
            max_stack: 2,
        }]
        .into_boxed_slice(),
        ops: ops.into_boxed_slice(),
        collections: Box::new([]),
        collection_items: Box::new([]),
    })
}

fn play_with_stop(store: &mut ActorStore, expression: &str, blend: f32) {
    store.set_server_animation_compiler(compile_stop);
    assert_eq!(
        store.apply_item_actor(
            1,
            2,
            ItemActorEvent::Action(ActorActionEvent {
                actor_runtime_ids: Arc::from([1]),
                kind: ActorActionKind::Custom {
                    animation: "animation.item".into(),
                    controller: "fixture.server".into(),
                    next_state: "default".into(),
                    stop_expression: expression.into(),
                    stop_expression_version: 13,
                },
                data: blend,
                swing_source: None,
            })
        ),
        ActorApplyResult::Updated
    );
}

fn translation(store: &ActorStore) -> f32 {
    store.actor_rigs().next().unwrap().current[0].translation_scale[0]
}

#[test]
fn server_animation_stops_on_its_expression_and_blends_out_on_the_outgoing_state_clock() {
    let mut store = store();
    play_with_stop(&mut store, "query.anim_time >= 0.2", 0.2);
    store.advance_interpolation_ticks(4);
    assert_eq!(translation(&store), -16.0);
    store.advance_interpolation_ticks(2);
    assert!((translation(&store) + 8.0).abs() < 1e-5);
    store.advance_interpolation_ticks(2);
    assert_eq!(translation(&store), 0.0);
    assert_eq!(store.animation_stats().frozen_actors, 0);
}

#[test]
fn server_animation_finished_query_tracks_the_played_clip_and_not_the_idle_controller() {
    let mut store = store();
    play_with_stop(&mut store, "query.all_animations_finished", 0.0);
    store.advance_interpolation_ticks(39);
    assert_eq!(translation(&store), -16.0);
    store.advance_interpolation_ticks(1);
    assert_eq!(translation(&store), 0.0);
}

#[test]
fn server_custom_animation_applies_to_the_local_player_while_remote_swings_stay_excluded() {
    let mut store = store();
    store.exclude_remote_state_for(1);
    play(&mut store, 2);
    store.advance_interpolation_ticks(1);
    assert_eq!(translation(&store), -16.0);
    assert_eq!(
        store.apply_item_actor(
            1,
            3,
            ItemActorEvent::Action(ActorActionEvent {
                actor_runtime_ids: Arc::from([1]),
                kind: ActorActionKind::SwingArm,
                data: 0.0,
                swing_source: None,
            })
        ),
        ActorApplyResult::MissingActor
    );
}

#[test]
fn generic_packet_channels_bind_by_bone_name_and_skip_names_absent_from_the_model() {
    let mut compiled = compiled();
    let mut dummy = compiled.geometries[0].bones[0].clone();
    dummy.name = "dummy".into();
    compiled.geometries[0].bones =
        vec![dummy, compiled.geometries[0].bones[0].clone()].into_boxed_slice();
    compiled.animation_clips[0].geometry = None;
    compiled.animation_channels[0].bone_name = Some("rightitem".into());
    let mut missing = compiled.animation_channels[0].clone();
    missing.bone_name = Some("missing".into());
    missing.first_keyframe = 1;
    compiled.animation_keyframes = vec![
        compiled.animation_keyframes[0],
        compiled.animation_keyframes[0],
    ]
    .into_boxed_slice();
    compiled.animation_channels =
        vec![compiled.animation_channels[0].clone(), missing].into_boxed_slice();
    compiled.animation_clips[0].channel_count = 2;
    let assets = Arc::new(RuntimeEntityAssets::from_compiled(compiled).unwrap());
    let actor = super::tests::actor_with_metadata(HashMap::new());
    let mut store = ActorAnimationStore::with_assets(Arc::clone(&assets));
    store.insert(1, 0, &actor);
    let request = store
        .prepare_server_animation(&ActorActionEvent {
            actor_runtime_ids: Arc::from([1]),
            kind: ActorActionKind::Custom {
                animation: "animation.item".into(),
                controller: "fixture.server".into(),
                next_state: "".into(),
                stop_expression: "".into(),
                stop_expression_version: 0,
            },
            data: 0.0,
            swing_source: None,
        })
        .unwrap();
    store.start_server_animation(1, request);
    store.advance_tick(&HashMap::from([(1, actor)]), None, None, true, true, |_| {
        ActorTickContext::default()
    });
    let rig = store.get(1).unwrap();
    assert_eq!(rig.current[0].translation_scale[0], 0.0);
    assert_eq!(rig.current[1].translation_scale[0], -16.0);
    assert_eq!(store.stats().frozen_actors, 0);
}

#[test]
fn server_animation_immediately_after_spawn_survives_the_first_pose_tick() {
    let mut store = unposed_store();
    assert_eq!(store.actor_rigs().next().unwrap().completed_tick, 0);
    play(&mut store, 2);
    store.advance_interpolation_ticks(1);
    assert_eq!(translation(&store), -16.0);
    assert_eq!(store.animation_stats().frozen_actors, 0);
}

#[test]
fn invalid_nonempty_server_stop_is_counted_once_and_skips_playback() {
    let mut store = store();
    play_with_stop(&mut store, "unsupported expression", 0.0);
    store.advance_interpolation_ticks(1);
    assert_eq!(translation(&store), 0.0);
    assert_eq!(store.animation_stats().invalid_server_stop_expressions, 1);
    assert_eq!(store.animation_stats().frozen_actors, 0);
}
