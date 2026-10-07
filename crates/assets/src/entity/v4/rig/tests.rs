use super::super::{EntityAnimationController, EntityControllerAnimation, EntityControllerState};
use super::*;

/// Builds a shared diamond so path expansion revisits the same leaf clip.
fn shared_controller_graph() -> CompiledEntityAssets {
    let mut compiled = CompiledEntityAssets {
        source_manifest_sha256: [1; 32],
        block_visual_count: 0,
        sources: Box::new([]),
        symbols: Box::new([]),
        geometries: Box::new([]),
        animation_clips: Box::new([]),
        animation_channels: Box::new([]),
        animation_keyframes: Box::new([]),
        molang_symbols: Box::new([]),
        molang_expressions: Box::new([]),
        molang_ops: Box::new([]),
        molang_collections: Box::new([]),
        molang_collection_items: Box::new([]),
        controllers: Box::new([]),
        controller_states: Box::new([]),
        controller_animations: Box::new([]),
        controller_transitions: Box::new([]),
        rig_bindings: Box::new([]),
        rig_geometries: Box::new([]),
        rig_animations: Box::new([]),
        rig_controllers: Box::new([]),
        item_visuals: Box::new([]),
        item_visual_aliases: Box::new([]),
        render: Default::default(),
    };
    compiled.controllers = (0..4)
        .map(|index| EntityAnimationController {
            symbol: 0,
            first_state: index,
            state_count: 1,
            initial_state: 0,
        })
        .collect();
    compiled.controller_states = (0..4)
        .map(|index| EntityControllerState {
            name: 0,
            first_animation: [0, 2, 3, 4][index],
            animation_count: [2, 1, 1, 1][index],
            first_transition: 0,
            transition_count: 0,
            on_entry: None,
            on_exit: None,
            ..Default::default()
        })
        .collect();
    compiled.controller_animations = [
        EntityControllerAnimationTarget::Controller(1),
        EntityControllerAnimationTarget::Controller(2),
        EntityControllerAnimationTarget::Controller(3),
        EntityControllerAnimationTarget::Controller(3),
        EntityControllerAnimationTarget::Clip(0),
    ]
    .map(|target| EntityControllerAnimation {
        target,
        weight: None,
    })
    .into();
    compiled
}

#[test]
fn controller_graph_visits_shared_leaf_clips_once() {
    let compiled = shared_controller_graph();
    let mut clips = Vec::new();
    let summaries = controller_summaries(&compiled, &mut |clip| {
        clips.push(clip);
        8
    })
    .unwrap();
    assert_eq!(summaries[0].required_bones, 8);
    assert_eq!(clips, [0]);
}

#[test]
fn controller_graph_rejects_cycles_and_overdeep_cached_paths() {
    let mut compiled = shared_controller_graph();
    assert!(validate_controller_nesting(&compiled).is_ok());
    compiled.controller_animations[4].target = EntityControllerAnimationTarget::Controller(0);
    assert!(validate_controller_nesting(&compiled).is_err());
    compiled.controller_animations[4].target = EntityControllerAnimationTarget::Clip(0);
    compiled.controller_animations[1].target = EntityControllerAnimationTarget::Controller(0);
    assert!(validate_controller_nesting(&compiled).is_err());
}

#[test]
fn controller_graph_checks_the_height_of_cached_shared_subgraphs() {
    let mut compiled = shared_controller_graph();
    let mut controllers = compiled.controllers.to_vec();
    let mut leaf = controllers[3];
    leaf.first_state = 4;
    controllers.push(leaf);
    compiled.controllers = controllers.into();
    let mut states = compiled.controller_states.to_vec();
    let mut leaf = states[3];
    leaf.first_animation = 5;
    states.push(leaf);
    compiled.controller_states = states.into();
    let mut animations = compiled.controller_animations.to_vec();
    animations[3].target = EntityControllerAnimationTarget::Controller(1);
    animations[4].target = EntityControllerAnimationTarget::Controller(4);
    animations.push(EntityControllerAnimation {
        target: EntityControllerAnimationTarget::Clip(0),
        weight: None,
    });
    compiled.controller_animations = animations.into();
    assert!(validate_controller_nesting(&compiled).is_err());
}

#[test]
fn dense_controller_graph_summarizes_each_leaf_once() {
    let mut compiled = shared_controller_graph();
    let width = 100;
    let count = width * MAX_ENTITY_CONTROLLER_NESTING;
    let mut controllers = Vec::new();
    let mut states = Vec::new();
    let mut animations = Vec::new();
    for index in 0..count {
        let mut controller = compiled.controllers[0];
        controller.first_state = index as u32;
        controllers.push(controller);
        let mut state = compiled.controller_states[1];
        state.first_animation = animations.len() as u32;
        let level = index / width;
        if level + 1 == MAX_ENTITY_CONTROLLER_NESTING {
            state.animation_count = 1;
            animations.push(EntityControllerAnimation {
                target: EntityControllerAnimationTarget::Clip(0),
                weight: None,
            });
        } else {
            state.animation_count = width as u16;
            for next in (level + 1) * width..(level + 2) * width {
                animations.push(EntityControllerAnimation {
                    target: EntityControllerAnimationTarget::Controller(next as u32),
                    weight: None,
                });
            }
        }
        states.push(state);
    }
    compiled.controllers = controllers.into();
    compiled.controller_states = states.into();
    compiled.controller_animations = animations.into();
    let mut visits = 0;
    controller_summaries(&compiled, &mut |_| {
        visits += 1;
        0
    })
    .unwrap();
    assert_eq!(visits, width);
}
