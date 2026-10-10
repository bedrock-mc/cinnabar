//! Controller weights observe the elapsed time of the state whose animations they select.
use super::*;
use assets::{
    CompiledMolangExpression, EntityAnimationController, EntityAssetKind, EntityAssetSource,
    EntityAssetSymbol, EntityControllerAnimation, EntityControllerState, EntityGeometryScalar,
    MolangOp, MolangSymbol, MolangSymbolKind,
};

/// Evaluates a timed animation weight, optionally while blending two different state epochs.
fn playback(blending: bool) -> (Vec<WeightedClip>, f32) {
    let mut compiled = crate::actor_animation::attachable::tests::compiled_fixture();
    let mut sources = compiled.sources.into_vec();
    sources.insert(
        0,
        EntityAssetSource {
            path: "animation_controllers/timed.json".into(),
            source_bytes: 1,
            source_sha256: [1; 32],
        },
    );
    compiled.sources = sources.into();
    let mut symbols = compiled.symbols.into_vec();
    for symbol in &mut symbols {
        symbol.source_index += 1;
    }
    symbols.insert(
        2,
        EntityAssetSymbol {
            kind: EntityAssetKind::AnimationController,
            identifier: "controller.animation.timed".into(),
            source_index: 0,
            dependencies: Box::new([]),
        },
    );
    compiled.symbols = symbols.into();
    for geometry in &mut compiled.geometries {
        geometry.source_index += 1;
    }
    for clip in &mut compiled.animation_clips {
        clip.source += 1;
    }
    compiled.rig_bindings[0].entity_symbol += 1;
    compiled.rig_bindings[0].render_controller += 1;
    for candidate in &mut compiled.render.candidates {
        candidate.source += 1;
    }
    compiled.molang_symbols = [
        (MolangSymbolKind::Name, "initial"),
        (MolangSymbolKind::Name, "next"),
        (MolangSymbolKind::Query, "query.state_time"),
    ]
    .map(|(kind, identifier)| MolangSymbol {
        kind,
        identifier: identifier.into(),
    })
    .into();
    compiled.molang_ops = [MolangOp::LoadQuery(2)].into();
    compiled.molang_expressions = [CompiledMolangExpression {
        first_op: 0,
        op_count: 1,
        max_stack: 1,
    }]
    .into();
    for keyframe in &mut compiled.animation_keyframes {
        keyframe.expressions = [Some(0), None, None];
    }
    compiled.animation_clips[0].anim_time_update = Some(0);
    compiled.controllers = [EntityAnimationController {
        symbol: 2,
        first_state: 0,
        state_count: 2,
        initial_state: 0,
    }]
    .into();
    compiled.controller_states = [0, 1]
        .map(|state| EntityControllerState {
            name: state,
            first_animation: state,
            animation_count: 1,
            blend_transition: EntityGeometryScalar::new(1.0).unwrap(),
            ..Default::default()
        })
        .into();
    compiled.controller_animations = [EntityControllerAnimation {
        target: EntityControllerAnimationTarget::Clip(0),
        weight: Some(0),
    }; 2]
        .into();
    let assets = RuntimeEntityAssets::from_compiled(compiled).unwrap();
    let layout = evaluation::VariableLayout::new(&assets);
    let actor = crate::actor_animation::tests::actor_with_metadata(HashMap::new());
    let input = ActorTickInput::default();
    let context = ActorTickContext::default();
    let evaluator = Evaluator {
        assets: &assets,
        layout: &layout,
        program: None,
        actor: &actor,
        input: &input,
        context: &context,
        anim_tick: 13,
        anim_time: None,
        swell_amount: None,
        presentation_alpha: None,
        query_history: None,
        life_tick: 13,
        finished: (false, false),
        state_time: 0.0,
        bones: &[],
        bone_names: &[],
    };
    let mut variables = layout.fresh(1);
    let mut controllers = [ControllerState {
        controller: 0,
        state: u16::from(blending),
        active: false,
        entered_tick: 8,
        blend_from: blending.then_some((0, 2, 0.0)),
    }];
    let mut world_left = MAX_MOLANG_OPS_PER_WORLD_TICK;
    let mut budget = EvalBudget {
        actor_left: MAX_MOLANG_OPS_PER_ACTOR_TICK,
        world_left: &mut world_left,
        work_left: MAX_RUNTIME_POSE_WORK_PER_ACTOR_TICK,
        transitions_left: MAX_CONTROLLER_TRANSITIONS_PER_TICK,
        used: 0,
        stack: Vec::new(),
        static_draw: None,
    };
    let mut clips = Vec::new();
    ControllerWalk {
        evaluator: &evaluator,
        variables: &mut variables,
        controllers: &mut controllers,
        clip_clocks: &Default::default(),
        clips: &mut clips,
        budget: &mut budget,
        journal: &mut ControllerJournal::default(),
        replay: false,
        record: false,
        reference: 0,
        path: [0; assets::MAX_ENTITY_CONTROLLER_NESTING],
    }
    .evaluate(0, 1.0, 0)
    .unwrap();
    super::super::super::clock::prepare(
        &evaluator,
        &mut variables,
        None,
        &controllers,
        &mut clips,
        &mut budget,
    )
    .unwrap();
    let pose = super::super::super::pose::sample_clips(
        &evaluator,
        &mut variables,
        &[RuntimeBone::default()],
        &["rightitem".into()],
        &clips,
        &mut budget,
    )
    .unwrap();
    (clips, pose[0].translation[0])
}

#[test]
fn controller_animation_weight_reads_its_state_time() {
    let (clips, _) = playback(false);
    assert_eq!(clips.len(), 1);
    assert!((clips[0].weight - 0.25).abs() < 1.0e-6);
}

#[test]
fn blended_animation_weights_read_their_own_state_times() {
    let (clips, _) = playback(true);
    assert_eq!(clips.len(), 2);
    assert!((clips[0].weight - 0.4125).abs() < 1.0e-6);
    assert!((clips[1].weight - 0.0625).abs() < 1.0e-6);
}

#[test]
fn controller_clocks_and_channels_observe_their_state_time() {
    let (clips, translation) = playback(false);
    assert_eq!(clips.len(), 1);
    assert!((clips[0].time - 0.25).abs() < 1.0e-6);
    assert!((translation - 0.0625).abs() < 1.0e-6);
}

#[test]
fn blended_clocks_and_channels_observe_separate_state_epochs() {
    let (clips, translation) = playback(true);
    assert_eq!(clips.len(), 2);
    assert!((clips[0].time - 0.55).abs() < 1.0e-6);
    assert!((clips[1].time - 0.25).abs() < 1.0e-6);
    assert!((translation - 0.2425).abs() < 1.0e-6);
}
