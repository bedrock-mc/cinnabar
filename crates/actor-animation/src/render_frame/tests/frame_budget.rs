use super::*;

const PLAYERS: u64 = 9;
const LOCAL: u64 = 1;

/// Nine skinned, sword-holding players walk, turn and swing while the camera moves every
/// frame; returns the most frame ops any one player sample used.
fn heaviest_player_sample(assets: &Arc<RuntimeEntityAssets>) -> (usize, ActorAnimationStore) {
    let skin = crate::test_support::skin_source(crate::test_support::SKIN_MODEL);
    let mut actors: HashMap<u64, TestActor> = (1..=PLAYERS)
        .map(|id| {
            let actor = TestActor {
                unique_id: id as i64,
                runtime_id: id,
                kind: ActorKind::Player {
                    uuid: [id as u8; 16],
                    username: format!("p{id}").into(),
                },
                on_ground: Some(true),
                ..Default::default()
            };
            (id, actor)
        })
        .collect();
    let mut store = ActorAnimationStore::with_assets(Arc::clone(assets));
    for id in 1..=PLAYERS {
        store.insert(1, 0, &actors[&id]);
    }
    let mut heaviest = 0;
    for tick in 0..30_u32 {
        for (id, actor) in &mut actors {
            actor.previous_position = actor.position;
            actor.position[0] += 0.2;
            actor.yaw += 7.0 * *id as f32;
            actor.head_yaw = actor.yaw + 20.0;
        }
        if tick % 2 == 0 {
            for id in 2..=PLAYERS {
                store.start_swing(id, 6, 6);
            }
        }
        let progress = (tick % 6) as f32 / 6.0;
        store.sync_local_swing(
            LOCAL,
            LocalSwingProgress {
                bedrock: [progress, ((tick + 1) % 6) as f32 / 6.0],
                java: [progress, progress],
                frame_alpha: None,
            },
        );
        let camera = [tick as f32, tick as f32 * 3.0];
        store.advance_tick(&actors, None, Some(LOCAL), true, tick == 0, |actor| {
            ActorTickContext {
                is_local: actor.runtime_id == LOCAL,
                is_local_player: actor.runtime_id == LOCAL,
                main_hand: Some("minecraft:diamond_sword".into()),
                skin_geometry: Some(Arc::clone(&skin)),
                camera_rotation: camera,
                camera_position: [0.0, 1.6, -4.0],
                ..Default::default()
            }
        });
        for step in 0..4 {
            let frame_camera = [camera[0] + step as f32, camera[1] + 2.0 * step as f32];
            let mut sampler = ActorRenderFrame::new(
                &store,
                &actors,
                frame_camera,
                [0.0, 1.6, -4.0],
                step as f32 / 4.0,
            );
            for id in 1..=PLAYERS {
                let before = sampler.remaining_ops();
                sampler.layers_with_skin(id);
                heaviest = heaviest.max(before - sampler.remaining_ops());
            }
        }
    }
    (heaviest, store)
}

/// Ordinary players never exhaust their frame budget and fall back to the tick pair.
#[test]
fn installed_players_fit_their_frame_budget() {
    let Some(assets) = crate::test_support::installed_entity_assets("player frame budget") else {
        return;
    };
    let (heaviest, store) = heaviest_player_sample(&assets);
    eprintln!("heaviest player frame sample: {heaviest} ops");
    assert_eq!(
        store.frame_fallbacks(),
        0,
        "heaviest sample used {heaviest} ops"
    );
}

/// A rig whose tick evaluation fits the actor budget also fits every frame sample, which
/// replays pre-animation for both endpoints besides its own pass.
#[test]
fn rigs_within_the_tick_budget_never_fall_back_between_ticks() {
    use assets::{CompiledMolangExpression, EntityAnimationProperty, MolangOp, MolangSymbol};
    let scalar = crate::test_support::scalar;
    let mut compiled = crate::test_support::counting_random_compiled("minecraft:player");
    compiled.rig_bindings[0].initialize = None;
    compiled.render.layers[0].color = None;
    compiled.molang_symbols = [
        (assets::MolangSymbolKind::Name, "wield"),
        (assets::MolangSymbolKind::Query, "query.body_y_rotation"),
    ]
    .map(|(kind, identifier)| MolangSymbol {
        kind,
        identifier: identifier.into(),
    })
    .into();
    // Two bounded loops make pre-animation cost about 1,400 ops.
    compiled.molang_ops = vec![
        MolangOp::Push(scalar(1000.0)),
        MolangOp::LoopStart(3),
        MolangOp::LoopNext(2),
        MolangOp::Push(scalar(400.0)),
        MolangOp::LoopStart(6),
        MolangOp::LoopNext(5),
        MolangOp::Push(scalar(0.0)),
        MolangOp::LoadQuery(1),
        MolangOp::Push(scalar(0.0)),
        MolangOp::Push(scalar(0.0)),
    ]
    .into();
    compiled.molang_expressions = [(0, 7, 1), (7, 1, 1), (8, 1, 1), (9, 1, 1)]
        .map(|(first_op, op_count, max_stack)| CompiledMolangExpression {
            first_op,
            op_count,
            max_stack,
        })
        .into();
    compiled.rig_bindings[0].pre_animation = Some(0);
    compiled.animation_channels[0].property = EntityAnimationProperty::Rotation;
    compiled.animation_keyframes[0].expressions = [None, Some(1), None];
    let assets = Arc::new(
        RuntimeEntityAssets::from_compiled(compiled.with_unresolved_admissions()).unwrap(),
    );
    let mut actor = TestActor {
        kind: ActorKind::Player {
            uuid: [1; 16],
            username: "p".into(),
        },
        ..Default::default()
    };
    let mut store = ActorAnimationStore::with_assets(assets);
    store.insert(1, 0, &actor);
    for _ in 0..3 {
        actor.yaw += 10.0;
        actor.head_yaw = actor.yaw;
        let actors = HashMap::from([(1, actor.clone())]);
        let before = store.stats().evaluated_molang_ops;
        store.advance_tick(&actors, None, None, true, true, |_| {
            ActorTickContext::default()
        });
        let used = store.stats().evaluated_molang_ops - before;
        assert!(
            (1_400..MAX_MOLANG_OPS_PER_ACTOR_TICK as u64).contains(&used),
            "tick used {used} ops"
        );
    }
    let actors = HashMap::from([(1, actor)]);
    let mut frame = ActorRenderFrame::new(&store, &actors, [0.0; 2], [0.0; 3], 0.5);
    let layers = frame.layers(1).unwrap();
    assert_eq!(store.frame_fallbacks(), 0);
    assert!(
        matches!(layers, Cow::Owned(_)),
        "the frame samples the turn"
    );
}
