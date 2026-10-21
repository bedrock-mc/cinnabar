use std::{collections::HashMap, sync::Arc};

use assets::{EntityAssetKind, RuntimeEntityAssets};
use protocol::{ActorActionEvent, ActorActionKind, ActorEvent, ActorSpawnEvent, ItemActorEvent};

use crate::actor_store::{ActorApplyResult, ActorStore};
use actor_animation::{ActorAnimationStore, ActorTickContext};

fn compiled() -> assets::CompiledEntityAssets {
    let mut compiled = actor_animation::test_support::compiled_fixture();
    compiled.sources[1].path = "entity/display.json".into();
    compiled.symbols[4].kind = EntityAssetKind::Entity;
    compiled.symbols[4].identifier = "minecraft:test".into();
    compiled.symbols.rotate_right(1);
    compiled.rig_bindings[0].entity_symbol = 0;
    compiled.rig_bindings[0].render_controller = Some(3);
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
    Arc::new(RuntimeEntityAssets::from_compiled(compiled().with_unresolved_admissions()).unwrap())
}

fn unposed_store() -> ActorStore {
    let mut store = ActorStore::new_with_entity_assets(1, 0, assets());
    let actor = super::actor_with_metadata(HashMap::new());
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
    Some(assets::MolangProgram::new(
        vec![assets::MolangSymbol {
            kind: assets::MolangSymbolKind::Query,
            identifier: query.into(),
        }]
        .into_boxed_slice(),
        vec![assets::CompiledMolangExpression {
            first_op: 0,
            op_count: ops.len() as u16,
            max_stack: 2,
        }]
        .into_boxed_slice(),
        ops.into_boxed_slice(),
        Box::new([]),
        Box::new([]),
    ))
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
    let assets = Arc::new(
        RuntimeEntityAssets::from_compiled(compiled.with_unresolved_admissions()).unwrap(),
    );
    let actor = super::actor_with_metadata(HashMap::new());
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

#[test]
fn display_actor_uses_custom_rig_and_keeps_a_separate_lifetime_namespace() {
    let mut custom = compiled();
    custom.symbols[0].identifier = "custom:display_actor".into();
    let custom =
        Arc::new(RuntimeEntityAssets::from_compiled(custom.with_unresolved_admissions()).unwrap());
    let mut displays = crate::DisplayActorStore::new(0, Arc::clone(&custom));
    let mut second = crate::DisplayActorStore::new(0, Arc::clone(&custom));
    let position = [1, 64, 2];
    let runtime = displays.resolve(position, "custom:display_actor").unwrap();
    assert_eq!(
        displays.resolve(position, "custom:display_actor"),
        Some(runtime)
    );
    displays.advance(1);
    let first = displays
        .rig(runtime)
        .expect("custom entity must use its normal rig")
        .actor;
    let other = second.resolve(position, "custom:display_actor").unwrap();
    second.advance(1);
    assert_ne!(
        first.session_id,
        second.rig(other).unwrap().actor.session_id
    );
    assert_eq!(
        first.runtime_id, other,
        "identical runtime ids cannot alias across pools"
    );
    assert_eq!(displays.actor(runtime).unwrap().position, [1.0, 64.0, 2.0]);
    assert_eq!(
        displays.resolve(position, "minecraft:test"),
        None,
        "an entity without a rig retains no display lifetime"
    );
    assert!(displays.actor(runtime).is_none());
    let replacement = displays.resolve(position, "custom:display_actor").unwrap();
    assert_ne!(replacement, runtime);
    displays.retain(&Default::default());
    assert!(displays.actor(replacement).is_none());
    assert!(
        second.actor(other).is_some(),
        "retirement is local to one pool"
    );
}

#[test]
fn display_actor_rigless_identifier_cache_evicts_its_oldest_entries() {
    let mut displays = crate::DisplayActorStore::new(0, assets());
    let limit = crate::MAX_DISPLAY_ACTORS;
    for index in 0..limit * 4 {
        let identifier = format!("custom:missing_{index}");
        assert_eq!(displays.resolve([index as i32, 0, 0], &identifier), None);
    }
    assert_eq!(displays.caches_unrigged("custom:missing_0"), None);
    assert_eq!(
        displays.caches_unrigged(&format!("custom:missing_{}", limit * 4 - 1)),
        Some(limit),
        "distinct server identifiers must not grow the cache past its bound"
    );
    assert!(displays.resolve([0; 3], "minecraft:test").is_some());
}

#[test]
fn display_actor_retires_a_replacement_with_a_cached_rigless_identifier() {
    let mut displays = crate::DisplayActorStore::new(0, assets());
    assert_eq!(displays.resolve([0; 3], "custom:missing"), None);
    let position = [1, 64, 2];
    let runtime = displays.resolve(position, "minecraft:test").unwrap();
    assert_eq!(displays.resolve(position, "custom:missing"), None);
    assert!(
        displays.actor(runtime).is_none(),
        "cached rigless replacements must release the previous display's slot"
    );
    assert!(displays.resolve(position, "minecraft:test").is_some());
}

#[test]
fn display_actor_retires_a_reused_lifetime_whose_pack_rig_was_removed() {
    let mut custom = compiled();
    custom.symbols[0].identifier = "custom:display_actor".into();
    let custom =
        Arc::new(RuntimeEntityAssets::from_compiled(custom.with_unresolved_admissions()).unwrap());
    let mut displays = crate::DisplayActorStore::new(0, assets());
    displays.set_pack(Some((Arc::clone(&custom), vec![0])));
    let position = [3, 64, 4];
    let runtime = displays.resolve(position, "custom:display_actor").unwrap();
    displays.set_pack(None);
    assert_eq!(
        displays.resolve(position, "custom:display_actor"),
        None,
        "a display whose rig left with its pack must not keep its slot"
    );
    assert!(displays.actor(runtime).is_none());
    assert!(displays.caches_unrigged("custom:display_actor").is_some());
    displays.set_pack(Some((custom, vec![0])));
    assert!(displays.resolve(position, "custom:display_actor").is_some());
}

#[test]
fn display_actor_rebinds_the_same_lifetime_to_replaced_pack_geometry() {
    let base = assets();
    let mut displays = crate::DisplayActorStore::new(0, Arc::clone(&base));
    let runtime = displays.resolve([0; 3], "minecraft:test").unwrap();
    displays.advance(1);
    let identity = displays.rig(runtime).unwrap().actor;
    displays.set_pack(Some((Arc::clone(&base), vec![0])));
    displays.advance(1);
    let overridden = displays.rig(runtime).unwrap();
    assert_eq!(overridden.actor, identity);
    assert!(overridden.rig.0 >= assets::PACK_RIG_ID_BASE);
    displays.set_pack(None);
    displays.advance(1);
    let restored = displays.rig(runtime).unwrap();
    assert_eq!(restored.actor, identity);
    assert!(restored.rig.0 < assets::PACK_RIG_ID_BASE);
}

/// Java 1.7's swing as a remote viewer runs it: a request restarts only an idle swing, one not
/// yet started or one past its first half; each tick steps it, and frames wrap forward to 1.
#[derive(Default)]
struct JavaSwing {
    counter: Option<i32>,
    progress: [f32; 2],
}

impl JavaSwing {
    fn request(&mut self, length: i32) {
        if self.counter.is_none_or(|c| c < 0 || c >= length / 2) {
            self.counter = Some(-1);
        }
    }

    fn tick(&mut self, length: i32) {
        self.counter = self.counter.map(|c| c + 1).filter(|c| *c < length);
        let current = self.counter.map_or(0.0, |c| c as f32 / length as f32);
        self.progress = [self.progress[1], current];
    }

    fn at(&self, alpha: f32) -> f32 {
        let [previous, current] = self.progress;
        let step = current - previous;
        previous + if step < 0.0 { step + 1.0 } else { step } * alpha
    }
}

const FRAME_FRACTIONS: [f32; 5] = [0.0, 0.25, 0.5, 0.75, 1.0];

fn swing_arm(store: &mut ActorStore, sequence: u64) {
    let swing = ItemActorEvent::Action(ActorActionEvent {
        actor_runtime_ids: Arc::from([1]),
        kind: ActorActionKind::SwingArm,
        data: 0.0,
        swing_source: None,
    });
    assert_eq!(
        store.apply_item_actor(1, sequence, swing),
        ActorApplyResult::Updated
    );
}

fn swing_effect(store: &mut ActorStore, action: protocol::ActorEffectAction, amplifier: i32) {
    store.apply_effect(&protocol::ActorEffectEvent {
        dimension: 0,
        actor_runtime_id: 1,
        action,
        effect_id: protocol::HASTE_EFFECT_ID,
        amplifier,
        particles: true,
        ambient: false,
        duration_ticks: 600,
        tick: 0,
    });
}

/// Steps the remote rig and Java's swing through `requests` and returns every frame's progress
/// after asserting the two agree at each frame fraction.
fn remote_swing_frames(
    store: &mut ActorStore,
    requests: &[u32],
    ticks: u32,
    length: i32,
) -> Vec<f32> {
    let mut java = JavaSwing::default();
    let mut frames = Vec::new();
    let first_sequence = store.latest_sequence + 1;
    for tick in 0..ticks {
        if requests.contains(&tick) {
            swing_arm(store, first_sequence + u64::from(tick));
            java.request(length);
        }
        store.advance_interpolation_ticks(1);
        java.tick(length);
        let motion = store.actor_rigs().next().unwrap().java;
        for alpha in FRAME_FRACTIONS {
            let progress = motion.swing_progress(alpha);
            assert!(
                (progress - java.at(alpha)).abs() < 1e-6,
                "tick {tick} frame {alpha}: {progress}, Java {}",
                java.at(alpha)
            );
            frames.push(progress);
        }
    }
    frames
}

/// Remote swings only move forward: within a tick and across restarts, progress never runs
/// backward except from the finished 1 to the next tick's rest at 0.
fn assert_forward(frames: &[f32]) {
    for pair in frames.windows(2) {
        assert!(
            pair[1] >= pair[0] || (pair[0] > 1.0 - 1e-6 && pair[1] == 0.0),
            "progress ran backward from {} to {}",
            pair[0],
            pair[1]
        );
    }
}

fn assert_tick(frames: &[f32], tick: usize, expected: [f32; 5]) {
    let actual = &frames[tick * FRAME_FRACTIONS.len()..][..FRAME_FRACTIONS.len()];
    for (actual, expected) in actual.iter().zip(expected) {
        assert!(
            (actual - expected).abs() < 1e-6,
            "tick {tick}: {actual:?}, want {expected:?}"
        );
    }
}

#[test]
fn remote_java_swing_ignores_early_reswings_and_wraps_restarts_forward() {
    let mut store = store();
    // A re-swing one tick in is ignored; one three ticks into a later swing restarts it.
    let frames = remote_swing_frames(&mut store, &[0, 2, 10, 14], 24, 6);
    assert_forward(&frames);
    let sixth = 1.0 / 6.0;
    assert_tick(
        &frames,
        2,
        [sixth, 1.25 * sixth, 1.5 * sixth, 1.75 * sixth, 2.0 * sixth],
    );
    // The restart tick sweeps the rest of the old swing, then the new one starts at rest.
    assert_tick(&frames, 14, [0.5, 0.625, 0.75, 0.875, 1.0]);
    assert_tick(
        &frames,
        15,
        [0.0, 0.25 * sixth, 0.5 * sixth, 0.75 * sixth, sixth],
    );
    // The last step wraps forward to 1 rather than sweeping back to 0.
    assert_tick(
        &frames,
        20,
        [5.0 * sixth, 0.875, 0.75 + sixth, 0.875 + 0.5 * sixth, 1.0],
    );
}

#[test]
fn remote_java_swing_follows_spam_clicks_one_restart_per_half_swing() {
    let mut store = store();
    let clicks = (0..40).step_by(2).collect::<Vec<_>>();
    let frames = remote_swing_frames(&mut store, &clicks, 40, 6);
    assert_forward(&frames);
    let restarts = frames
        .chunks(FRAME_FRACTIONS.len())
        .filter(|tick| tick[0] == 0.5 && tick[4] == 1.0)
        .count();
    assert_eq!(restarts, 9, "every other click restarts the swing");
}

#[test]
fn remote_haste_shortens_the_java_swing_and_its_restart_guard_until_removed() {
    let mut store = store();
    swing_effect(&mut store, protocol::ActorEffectAction::Add, 1);
    // Haste II: four ticks, and a re-swing two ticks in is already past half.
    let frames = remote_swing_frames(&mut store, &[0, 3], 8, 4);
    assert_forward(&frames);
    assert_eq!(frames[FRAME_FRACTIONS.len() * 2], 0.25);
    assert_eq!(frames[FRAME_FRACTIONS.len() * 3], 0.5);
    swing_effect(&mut store, protocol::ActorEffectAction::Remove, 0);
    remote_swing_frames(&mut store, &[0, 3], 8, 6);
}

/// Remote Bedrock swings follow the actor's effects: six ticks under Haste I, four under Haste II.
#[test]
fn remote_bedrock_swing_length_follows_haste_levels() {
    for (amplifier, length) in [(0, 6), (1, 4)] {
        let mut store = store();
        swing_effect(&mut store, protocol::ActorEffectAction::Add, amplifier);
        let first = store.latest_sequence + 1;
        swing_arm(&mut store, first);
        let mut progress = Vec::new();
        for _ in 0..length + 1 {
            store.advance_interpolation_ticks(1);
            progress.push(store.actor_rigs().next().unwrap().hand[1].attack_time);
        }
        let expected: Vec<f32> = (0..length)
            .map(|tick| tick as f32 / length as f32)
            .chain([0.0])
            .collect();
        assert_eq!(progress, expected, "Haste amplifier {amplifier}");
    }
}
