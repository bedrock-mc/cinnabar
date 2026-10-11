use super::super::{tests::actor_with_metadata, tick};
use super::*;

/// Reads the integer metadata source from a typed query contract.
fn key(query: Q) -> u32 {
    query
        .integer_metadata_key()
        .expect("registered integer metadata query")
}

fn fish(variant: ActorMetadataValue, mark_variant: ActorMetadataValue) -> ActorSnapshot {
    let mut actor = actor_with_metadata(HashMap::from([
        (key(Q::Variant), variant),
        (key(Q::MarkVariant), mark_variant),
    ]));
    actor.kind = ActorKind::Entity {
        identifier: "minecraft:tropicalfish".into(),
    };
    actor
}

fn engine() -> EngineSlots {
    EngineSlots {
        tropical_fish_base: Some(0),
        tropical_fish_pattern: Some(1),
        ..EngineSlots::default()
    }
}

fn update(actor: &ActorSnapshot, variables: &mut MolangVariables) {
    tick::apply_engine_variables(
        &engine(),
        variables,
        actor,
        &ActorTickContext::default(),
        &ActorTickInput::default(),
        &MotionState::default(),
    );
}

#[test]
fn both_tropical_fish_families_select_all_six_patterns() {
    for (variant, patterns) in [(0, [0, 1, 2, 3, 4, 5]), (1, [6, 7, 8, 9, 10, 11])] {
        for (mark, pattern) in patterns.into_iter().enumerate() {
            let actor = fish(
                ActorMetadataValue::Int(variant),
                ActorMetadataValue::Int(mark as i32),
            );
            let mut variables = MolangVariables::slots(2);
            update(&actor, &mut variables);
            assert_eq!(variables.number_at(0), Some(variant as f32));
            assert_eq!(variables.number_at(1), Some(pattern as f32));
        }
    }
}

#[test]
fn tropical_fish_metadata_changes_refresh_retained_variables() {
    let mut actor = fish(ActorMetadataValue::Int(0), ActorMetadataValue::Int(5));
    let mut variables = MolangVariables::slots(2);
    update(&actor, &mut variables);
    assert_eq!(variables.number_at(0), Some(0.0));
    assert_eq!(variables.number_at(1), Some(5.0));

    actor
        .metadata
        .insert(key(Q::Variant), ActorMetadataValue::Int(1));
    actor
        .metadata
        .insert(key(Q::MarkVariant), ActorMetadataValue::Int(2));
    update(&actor, &mut variables);
    assert_eq!(variables.number_at(0), Some(1.0));
    assert_eq!(variables.number_at(1), Some(8.0));

    actor.metadata.clear();
    update(&actor, &mut variables);
    assert_eq!(variables.number_at(0), Some(0.0));
    assert_eq!(variables.number_at(1), Some(0.0));
}

#[test]
fn invalid_tropical_fish_marks_reset_to_the_first_pattern_of_the_family() {
    for variant in [0, 1] {
        for mark in [i32::MIN, -1, 6, i32::MAX] {
            let actor = fish(
                ActorMetadataValue::Int(variant),
                ActorMetadataValue::Int(mark),
            );
            assert_eq!(
                tropical_fish_variables(&actor),
                Some([variant as f32, if variant == 0 { 0.0 } else { 6.0 }]),
            );
        }
    }
}

#[test]
fn tropical_fish_base_uses_the_native_low_byte_truth_test() {
    for (variant, expected_base) in [
        (-1, 1.0),
        (255, 1.0),
        (256, 0.0),
        (-256, 0.0),
        (i32::MIN, 0.0),
        (i32::MAX, 1.0),
    ] {
        let actor = fish(ActorMetadataValue::Int(variant), ActorMetadataValue::Int(3));
        assert_eq!(
            tropical_fish_variables(&actor),
            Some([expected_base, if expected_base == 0.0 { 3.0 } else { 9.0 }]),
        );
    }
}

#[test]
fn tropical_fish_variables_require_native_int_metadata_tags() {
    for value in [
        ActorMetadataValue::Byte(1),
        ActorMetadataValue::Short(1),
        ActorMetadataValue::Long(1),
        ActorMetadataValue::Float(1.0),
        ActorMetadataValue::Float(f32::NAN),
        ActorMetadataValue::Float(f32::INFINITY),
        ActorMetadataValue::Float(f32::NEG_INFINITY),
        ActorMetadataValue::String("1".into()),
    ] {
        let actor = fish(value.clone(), ActorMetadataValue::Int(5));
        assert_eq!(tropical_fish_variables(&actor), Some([0.0, 5.0]));
        let actor = fish(ActorMetadataValue::Int(1), value);
        assert_eq!(tropical_fish_variables(&actor), Some([1.0, 6.0]));
    }
}

#[test]
fn tropical_fish_engine_variables_preserve_other_actor_pack_values() {
    let mut actor = fish(ActorMetadataValue::Int(1), ActorMetadataValue::Int(4));
    let mut variables = MolangVariables::slots(2);
    variables.set(Some(0), 3.0);
    variables.set(Some(1), 4.0);
    for identifier in ["minecraft:cod", "example:tropicalfish", "tropicalfish"] {
        actor.kind = ActorKind::Entity {
            identifier: identifier.into(),
        };
        update(&actor, &mut variables);
        assert_eq!(variables.number_at(0), Some(3.0));
        assert_eq!(variables.number_at(1), Some(4.0));
    }
    actor.kind = ActorKind::Player {
        uuid: [0; 16],
        username: "test".into(),
    };
    update(&actor, &mut variables);
    assert_eq!(variables.number_at(0), Some(3.0));
    assert_eq!(variables.number_at(1), Some(4.0));
}
