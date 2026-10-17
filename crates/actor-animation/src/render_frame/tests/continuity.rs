use super::*;
use assets::{
    CompiledMolangExpression, EntityAnimationProperty, EntityAssetKind, EntityAssetSymbol,
    MolangOp, MolangSymbol, MolangSymbolKind,
};

const LOCAL: u64 = 1;
const FRAMES_PER_TICK: usize = 5;
const CAMERA_POSITION: [f32; 3] = [0.0, 1.6, -4.0];

fn player(runtime_id: u64) -> TestActor {
    TestActor {
        unique_id: runtime_id as i64,
        runtime_id,
        kind: ActorKind::Player {
            uuid: [runtime_id as u8; 16],
            username: format!("p{runtime_id}").into(),
        },
        on_ground: Some(true),
        ..Default::default()
    }
}

/// Body bones as `[rotation, translation]` after the shader's endpoint mix at `alpha`.
fn drawn_pose(previous: &[BoneTransform], current: &[BoneTransform], alpha: f32) -> Vec<[f32; 7]> {
    previous
        .iter()
        .zip(current)
        .map(|(previous, current)| {
            let dot: f32 = previous
                .rotation
                .iter()
                .zip(current.rotation)
                .map(|(a, b)| a * b)
                .sum();
            let sign = if dot < 0.0 { -1.0 } else { 1.0 };
            let mix = |from: f32, to: f32| from + (to - from) * alpha;
            let mut out = [0.0; 7];
            for (slot, (from, to)) in out[..4]
                .iter_mut()
                .zip(previous.rotation.iter().zip(current.rotation))
            {
                *slot = mix(*from, to * sign);
            }
            for (slot, (from, to)) in out[4..].iter_mut().zip(
                previous
                    .translation_scale
                    .iter()
                    .zip(current.translation_scale),
            ) {
                *slot = mix(*from, to);
            }
            out
        })
        .collect()
}

/// Walks `players` (the first local, in third person) for 20 sampled ticks and returns every
/// bone whose drawn pose jumps at a tick edge far beyond its in-tick motion.
fn tick_steps(assets: &Arc<RuntimeEntityAssets>, players: u64, look: f32) -> Vec<(u64, usize)> {
    let mut actors: HashMap<u64, TestActor> = (1..=players).map(|id| (id, player(id))).collect();
    let mut store = ActorAnimationStore::with_assets(Arc::clone(assets));
    for id in 1..=players {
        store.insert(1, 0, &actors[&id]);
    }
    let camera = |frame: usize| {
        let frame = frame as f32 * look;
        [5.0 + frame * 0.7, 30.0 + frame * 1.3]
    };
    let mut series: HashMap<u64, Vec<Vec<[f32; 7]>>> = HashMap::new();
    for tick in 0..24 {
        for (id, actor) in &mut actors {
            actor.previous_position = actor.position;
            actor.position[0] += 0.08 + *id as f32 * 0.02;
            if id % 3 == 0 {
                actor.yaw += 9.0;
                actor.head_yaw += 9.0;
            }
        }
        store.advance_tick(&actors, None, Some(LOCAL), true, tick == 0, |actor| {
            ActorTickContext {
                is_local: actor.runtime_id == LOCAL,
                is_local_player: actor.runtime_id == LOCAL,
                camera_rotation: camera(tick * FRAMES_PER_TICK),
                camera_position: CAMERA_POSITION,
                ..Default::default()
            }
        });
        if tick < 4 {
            continue;
        }
        for step in 0..FRAMES_PER_TICK {
            let alpha = step as f32 / FRAMES_PER_TICK as f32;
            let frame = tick * FRAMES_PER_TICK + step;
            let mut sampler =
                ActorRenderFrame::new(&store, &actors, camera(frame), CAMERA_POSITION, alpha);
            for id in 1..=players {
                let rig = store.get(id).unwrap();
                let layers = sampler.layers_with_skin(id).unwrap();
                // Presentation draws frame-sampled bodies and otherwise the committed tick pair.
                let body = match &layers.render {
                    Cow::Owned(layers) => layers
                        .iter()
                        .find(|layer| layer.geometry.is_none() && !layer.pose.is_empty()),
                    Cow::Borrowed(_) => None,
                };
                series.entry(id).or_default().push(match body {
                    Some(body) => drawn_pose(&body.previous_pose, &body.pose, alpha),
                    None => drawn_pose(rig.previous, rig.current, alpha),
                });
            }
        }
    }
    assert_eq!(
        store.frame_fallbacks(),
        0,
        "ordinary players never fall back"
    );
    let mut steps = Vec::new();
    for (id, frames) in &series {
        for bone in 0..frames[0].len() {
            let deltas: Vec<f32> = frames
                .windows(2)
                .map(|pair| {
                    pair[0][bone]
                        .iter()
                        .zip(pair[1][bone])
                        .map(|(a, b)| (a - b).abs())
                        .sum()
                })
                .collect();
            let stepped = (FRAMES_PER_TICK - 1..deltas.len() - 1)
                .step_by(FRAMES_PER_TICK)
                .any(|edge| deltas[edge] > deltas[edge - 1].max(deltas[edge + 1]) * 3.0 + 1.0e-4);
            if stepped {
                steps.push((*id, bone));
            }
        }
    }
    steps.sort_unstable();
    steps
}

/// Nine walking or turning players, local and remote, never step once per tick, with or
/// without look input moving the camera between frames.
#[test]
fn installed_player_poses_change_every_frame_without_tick_steps() {
    let Some(assets) = crate::test_support::installed_entity_assets("player animation") else {
        return;
    };
    for look in [0.0, 1.0] {
        let steps = tick_steps(&assets, 9, look);
        assert!(
            steps.is_empty(),
            "look {look}: tick steps (player, bone) {steps:?}"
        );
    }
}

/// A walk variable that another script also writes beside an unrelated camera read keeps the
/// walk clip tick-interpolated while the camera moves.
#[test]
fn camera_reads_beside_a_walk_variable_keep_walking_smooth() {
    assert_smooth_walk(walk_catalog(false));
}

/// A walk clip that genuinely reads the camera still interpolates its motion between ticks.
#[test]
fn camera_dependent_walk_clips_keep_walking_smooth() {
    assert_smooth_walk(walk_catalog(true));
}

fn assert_smooth_walk(assets: Arc<RuntimeEntityAssets>) {
    for players in [1, 9] {
        for look in [0.0, 1.0] {
            let steps = tick_steps(&assets, players, look);
            assert!(
                steps.is_empty(),
                "{players} players, look {look}: tick steps (player, bone) {steps:?}"
            );
        }
    }
}

/// A walk clip driven by `variable.tcos0`, which another entity's script writes beside a camera
/// read; `camera_walk` also makes the walk clip itself read a camera input.
fn walk_catalog(camera_walk: bool) -> Arc<RuntimeEntityAssets> {
    let mut compiled = crate::test_support::counting_random_compiled("minecraft:player");
    compiled.rig_bindings[0].initialize = None;
    compiled.render.layers[0].color = None;
    let mut symbols = compiled.symbols.into_vec();
    symbols.insert(
        3,
        EntityAssetSymbol {
            kind: EntityAssetKind::Animation,
            identifier: "animation.spear".into(),
            ..symbols[2].clone()
        },
    );
    compiled.symbols = symbols.into();
    *compiled.rig_bindings[0].render_controller.as_mut().unwrap() += 1;
    compiled.molang_symbols = [
        (MolangSymbolKind::Name, "wield"),
        (MolangSymbolKind::Query, "query.modified_distance_moved"),
        (MolangSymbolKind::Variable, "variable.spear_copy"),
        (MolangSymbolKind::Variable, "variable.tcos0"),
        (
            MolangSymbolKind::Variable,
            "variable.tp_melee_spear_use_arm_rotation_x",
        ),
    ]
    .map(|(kind, identifier)| MolangSymbol {
        kind,
        identifier: identifier.into(),
    })
    .into();
    let scalar = crate::test_support::scalar;
    let walk = [
        MolangOp::LoadQuery(1),
        MolangOp::Push(scalar(40.0)),
        MolangOp::Multiply,
        MolangOp::StoreVariable(3),
    ];
    compiled.molang_ops = walk
        .into_iter()
        .chain([
            MolangOp::Push(scalar(0.0)),
            MolangOp::LoadVariable(3),
            MolangOp::LoadVariable(4),
            MolangOp::Push(scalar(0.0)),
        ])
        // Another entity's script writes the same walk variable beside a camera read.
        .chain(walk)
        .chain([
            MolangOp::LoadVariable(4),
            MolangOp::StoreVariable(2),
            MolangOp::Push(scalar(0.0)),
        ])
        .collect();
    compiled.molang_expressions = [(0, 5, 2), (5, 1, 1), (6, 1, 1), (7, 1, 1), (8, 7, 2)]
        .map(|(first_op, op_count, max_stack)| CompiledMolangExpression {
            first_op,
            op_count,
            max_stack,
        })
        .into();
    compiled.rig_bindings[0].pre_animation = Some(0);
    compiled.animation_channels[0].property = EntityAnimationProperty::Rotation;
    compiled.animation_keyframes[0].expressions = [Some(1), camera_walk.then_some(2), None];
    let mut channels = compiled.animation_channels.into_vec();
    let mut spear = channels[0].clone();
    spear.first_keyframe = 1;
    channels.push(spear);
    compiled.animation_channels = channels.into();
    let mut keys = compiled.animation_keyframes.into_vec();
    let mut spear = keys[0];
    spear.expressions = [Some(2), None, None];
    keys.push(spear);
    compiled.animation_keyframes = keys.into();
    let mut clips = compiled.animation_clips.into_vec();
    let mut spear = clips[0];
    spear.symbol = 3;
    spear.first_channel = 1;
    clips.push(spear);
    compiled.animation_clips = clips.into();
    let mut bindings = compiled.rig_animations.into_vec();
    // The camera clip makes the rig camera-capable but never contributes.
    let mut spear = bindings[0];
    spear.clip = 1;
    spear.order = 1;
    spear.weight = Some(3);
    bindings.push(spear);
    compiled.rig_animations = bindings.into();
    compiled.rig_geometries[0].animation_count += 1;
    Arc::new(RuntimeEntityAssets::from_compiled(compiled.with_unresolved_admissions()).unwrap())
}

/// A frame that runs out of expression budget counts its fallback to the tick pair.
#[test]
fn exhausted_frames_count_their_fallbacks() {
    let actors = HashMap::from([(1, player(1))]);
    let mut store = ActorAnimationStore::with_assets(walk_catalog(true));
    store.insert(1, 0, &actors[&1]);
    for _ in 0..3 {
        store.advance_tick(&actors, None, None, true, true, |_| {
            ActorTickContext::default()
        });
    }
    let mut frame = ActorRenderFrame::new(&store, &actors, [0.0; 2], [0.0; 3], 0.5);
    frame.set_remaining_ops(0);
    assert!(frame.layers(1).is_some());
    assert_eq!(store.frame_fallbacks(), 1);
}
