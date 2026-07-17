use sha2::{Digest, Sha256};

use super::{entity, item, suite::carrier_v4_fixture};
use entity::{MolangOp, RuntimeEntityAssets};

fn assert_mutation_rejected(mutate: impl FnOnce(&mut entity::CompiledEntityAssets)) {
    let mut compiled = carrier_v4_fixture();
    mutate(&mut compiled);
    assert!(compiled.validate().is_err());
}

#[test]
fn carrier_v4_rejects_every_extended_cross_index_relationship() {
    assert_mutation_rejected(|c| c.animation_clips[0].symbol = 0);
    assert_mutation_rejected(|c| c.animation_clips[0].source = 0);
    assert_mutation_rejected(|c| c.animation_clips[0].first_channel = u32::MAX);
    assert_mutation_rejected(|c| c.animation_channels[0].first_keyframe = u32::MAX);
    assert_mutation_rejected(|c| c.molang_ops[0] = MolangOp::LoadQuery(u32::MAX));
    assert_mutation_rejected(|c| c.molang_ops[0] = MolangOp::SelectCollection(u32::MAX));
    assert_mutation_rejected(|c| c.molang_collections[0].first_item = u32::MAX);
    assert_mutation_rejected(|c| c.controllers[0].symbol = 0);
    assert_mutation_rejected(|c| c.controllers[0].first_state = u32::MAX);
    assert_mutation_rejected(|c| c.controller_states[0].name = u32::MAX);
    assert_mutation_rejected(|c| c.controller_states[0].first_animation = u32::MAX);
    assert_mutation_rejected(|c| c.controller_states[0].first_transition = u32::MAX);
    assert_mutation_rejected(|c| c.controller_states[0].on_entry = Some(u32::MAX));
    assert_mutation_rejected(|c| c.controller_states[0].on_exit = Some(u32::MAX));
    assert_mutation_rejected(|c| c.controller_animations[0].clip = u32::MAX);
    assert_mutation_rejected(|c| c.controller_animations[0].weight = Some(u32::MAX));
    assert_mutation_rejected(|c| c.controller_transitions[0].target_state = u16::MAX);
    assert_mutation_rejected(|c| c.controller_transitions[0].condition = u32::MAX);
    assert_mutation_rejected(|c| c.rig_bindings[0].entity_symbol = 1);
    assert_mutation_rejected(|c| c.rig_bindings[0].geometry = u32::MAX);
    assert_mutation_rejected(|c| c.rig_bindings[0].render_controller = 0);
    assert_mutation_rejected(|c| c.rig_bindings[0].first_animation = u32::MAX);
    assert_mutation_rejected(|c| c.rig_bindings[0].first_controller = u32::MAX);
    assert_mutation_rejected(|c| c.rig_animations[0].name = u32::MAX);
    assert_mutation_rejected(|c| c.rig_animations[0].clip = u32::MAX);
    assert_mutation_rejected(|c| c.rig_controllers[0].name = u32::MAX);
    assert_mutation_rejected(|c| c.rig_controllers[0].controller = u32::MAX);
    assert_mutation_rejected(|c| c.item_visuals[0].texture_source = u32::MAX);
    assert_mutation_rejected(|c| {
        c.item_visuals[0].block_visual = Some(item::BlockVisualId(c.block_visual_count));
    });
    assert_mutation_rejected(|c| c.item_visual_aliases[0].visual = item::ItemVisualId(u32::MAX));
}

#[test]
fn carrier_v4_validates_channel_bones_against_the_selected_rig_geometry() {
    assert_mutation_rejected(|c| c.geometries[0].bones = Box::new([]));
}

#[test]
fn carrier_v4_rejects_every_orphan_flattened_tail() {
    assert_mutation_rejected(|c| {
        let mut values = c.animation_channels.to_vec();
        values.push(values[0]);
        c.animation_channels = values.into_boxed_slice();
    });
    assert_mutation_rejected(|c| {
        let mut values = c.animation_keyframes.to_vec();
        values.push(values[0]);
        c.animation_keyframes = values.into_boxed_slice();
    });
    assert_mutation_rejected(|c| {
        let mut values = c.molang_ops.to_vec();
        values.push(values[0]);
        c.molang_ops = values.into_boxed_slice();
    });
    assert_mutation_rejected(|c| {
        let mut values = c.molang_collection_items.to_vec();
        values.push(values[0]);
        c.molang_collection_items = values.into_boxed_slice();
    });
    assert_mutation_rejected(|c| {
        let mut values = c.controller_states.to_vec();
        values.push(values[0]);
        c.controller_states = values.into_boxed_slice();
    });
    assert_mutation_rejected(|c| {
        let mut values = c.controller_animations.to_vec();
        values.push(values[0]);
        c.controller_animations = values.into_boxed_slice();
    });
    assert_mutation_rejected(|c| {
        let mut values = c.controller_transitions.to_vec();
        values.push(values[0]);
        c.controller_transitions = values.into_boxed_slice();
    });
    assert_mutation_rejected(|c| {
        let mut values = c.rig_animations.to_vec();
        values.push(values[0]);
        c.rig_animations = values.into_boxed_slice();
    });
    assert_mutation_rejected(|c| {
        let mut values = c.rig_controllers.to_vec();
        values.push(values[0]);
        c.rig_controllers = values.into_boxed_slice();
    });
}

#[test]
fn carrier_v4_rejects_each_new_total_ceiling_plus_one() {
    assert_mutation_rejected(|c| {
        c.molang_ops = vec![c.molang_ops[0]; entity::MAX_MOLANG_OPS + 1].into_boxed_slice();
    });
    assert_mutation_rejected(|c| {
        c.molang_collections =
            vec![c.molang_collections[0]; entity::MAX_MOLANG_COLLECTIONS + 1].into_boxed_slice();
    });
    assert_mutation_rejected(|c| {
        c.molang_collection_items =
            vec![c.molang_collection_items[0]; entity::MAX_MOLANG_COLLECTION_ITEMS_TOTAL + 1]
                .into_boxed_slice();
    });
    assert_mutation_rejected(|c| {
        c.controller_animations =
            vec![c.controller_animations[0]; entity::MAX_ENTITY_CONTROLLER_ANIMATIONS + 1]
                .into_boxed_slice();
    });
    assert_mutation_rejected(|c| {
        c.rig_animations =
            vec![c.rig_animations[0]; entity::MAX_ENTITY_RIG_ANIMATIONS + 1].into_boxed_slice();
    });
    assert_mutation_rejected(|c| {
        c.rig_controllers =
            vec![c.rig_controllers[0]; entity::MAX_ENTITY_RIG_CONTROLLERS + 1].into_boxed_slice();
    });
}

#[test]
fn carrier_v4_summary_reports_every_retained_extended_section() {
    let blob = entity::encode_entity_blob(&carrier_v4_fixture()).unwrap();
    let summary = RuntimeEntityAssets::decode(&blob).unwrap().summary();
    assert_eq!(summary.molang_symbols, 1);
    assert_eq!(summary.molang_ops, 1);
    assert_eq!(summary.molang_collections, 1);
    assert_eq!(summary.molang_collection_items, 1);
    assert_eq!(summary.controller_animations, 1);
    assert_eq!(summary.rig_animations, 1);
    assert_eq!(summary.rig_controllers, 1);
    assert_eq!(summary.block_visuals, 8);
}

#[test]
fn carrier_v4_preflights_every_retained_array_before_typed_allocation() {
    let blob = entity::encode_entity_blob(&carrier_v4_fixture()).unwrap();
    let mut payload: serde_json::Value =
        serde_json::from_slice(&blob[80..blob.len() - 32]).unwrap();
    payload["animation_channels"] = serde_json::Value::Array(vec![
        serde_json::Value::Null;
        entity::MAX_ENTITY_ANIMATION_CHANNELS
            + 1
    ]);
    let payload = serde_json::to_vec(&payload).unwrap();
    let mut excessive = blob[..80].to_vec();
    excessive[56..64].copy_from_slice(&(payload.len() as u64).to_le_bytes());
    excessive.extend_from_slice(&payload);
    let digest = Sha256::digest(&excessive);
    excessive.extend_from_slice(&digest);

    let error = RuntimeEntityAssets::decode(&excessive).unwrap_err();
    assert!(error.to_string().contains("count preflight"));
}

#[test]
fn carrier_v4_preflights_nested_dependency_arrays_before_typed_allocation() {
    let blob = entity::encode_entity_blob(&carrier_v4_fixture()).unwrap();
    let mut payload: serde_json::Value =
        serde_json::from_slice(&blob[80..blob.len() - 32]).unwrap();
    payload["symbols"][0]["dependencies"] = serde_json::Value::Array(vec![
        serde_json::Value::Null;
        entity::MAX_ENTITY_DEPENDENCIES
            + 1
    ]);
    let payload = serde_json::to_vec(&payload).unwrap();
    let mut excessive = blob[..80].to_vec();
    excessive[56..64].copy_from_slice(&(payload.len() as u64).to_le_bytes());
    excessive.extend_from_slice(&payload);
    let digest = Sha256::digest(&excessive);
    excessive.extend_from_slice(&digest);

    let error = RuntimeEntityAssets::decode(&excessive).unwrap_err();
    assert!(error.to_string().contains("count preflight"));
}
