use super::*;
use crate::attachable::AttachableQueryContext;
use assets::{MOLANG_QUERY_DESCRIPTORS, MolangQuerySupport};

#[test]
fn every_admitted_query_has_a_dispatch_or_an_explicit_idle_disposition() {
    let actor = crate::tests::actor_with_metadata(HashMap::new());
    let input = ActorTickInput::default();
    let context = ActorTickContext::default();
    let inputs = QueryInputs {
        actor: &actor,
        input: &input,
        context: &context,
        anim_tick: 0,
        anim_time: None,
        swell_amount: None,
        life_tick: 0,
        finished: (false, false),
        state_time: 0.0,
        bones: &[],
        bone_names: &[],
    };
    for descriptor in MOLANG_QUERY_DESCRIPTORS {
        let binding = Q::from_name(descriptor.name).unwrap();
        for arguments in [
            vec![],
            vec![MolangValue::Number(0.0)],
            vec![
                MolangValue::String("fixture:property".into()),
                MolangValue::Number(0.0),
            ],
        ] {
            let value = query(&inputs, binding, &arguments);
            if descriptor.support == MolangQuerySupport::Unimplemented {
                assert_eq!(value.number(), 0.0, "{}", descriptor.name);
            }
        }
    }
}

#[test]
fn typed_actor_item_use_queries_preserve_timing_and_arguments() {
    check_typed_item_use_queries(false);
}

#[test]
fn typed_attachable_item_use_queries_preserve_timing_and_arguments() {
    check_typed_item_use_queries(true);
}

/// Checks item timing and all supported argument forms in the selected query context.
fn check_typed_item_use_queries(attachable: bool) {
    let mut actor = crate::tests::actor_with_metadata(HashMap::new());
    actor
        .metadata
        .insert(0, ActorMetadataValue::Flags(1 << FLAG_USING_ITEM));
    let input = ActorTickInput {
        item_use_ticks: 6,
        ..Default::default()
    };
    let mut context = ActorTickContext {
        main_hand_max_use_ticks: 25,
        off_hand_max_use_ticks: 40,
        local_item_use_ticks: Some(5),
        ..Default::default()
    };
    if attachable {
        context.attachable = Some(AttachableQueryContext {
            worn: false,
            worn_slot: 0,
            first_person: true,
            off_hand: false,
            is_paperdoll: false,
            frame_alpha: 0.0,
            animation_frame: 0,
            delta_seconds: None,
            use_elapsed_ticks: Some(5),
            max_use_ticks: 25,
            owner_life_tick: 0,
        });
    }
    let inputs = QueryInputs {
        actor: &actor,
        input: &input,
        context: &context,
        anim_tick: 0,
        anim_time: None,
        swell_amount: None,
        life_tick: 0,
        finished: (false, false),
        state_time: 0.0,
        bones: &[],
        bone_names: &[],
    };
    assert_eq!(
        query(&inputs, Q::MainHandItemUseDuration, &[]).number(),
        20.0
    );
    assert_eq!(
        query(&inputs, Q::MainHandItemMaxDuration, &[]).number(),
        25.0
    );
    let slot = MolangValue::String("main_hand".into());
    let range = MolangValue::Number(2.0);
    for arguments in [vec![range.clone()], vec![slot.clone(), range]] {
        assert!(
            (query(&inputs, Q::ItemRemainingUseDuration, &arguments).number() - 1.6).abs() < 1e-6
        );
    }
    assert_eq!(
        query(&inputs, Q::ItemRemainingUseDuration, &[]).number(),
        if attachable { 20.0 } else { 1.0 }
    );
    assert_eq!(
        query(&inputs, Q::ItemRemainingUseDuration, &[slot]).number(),
        if attachable { 20.0 } else { 400.0 }
    );
    assert_eq!(
        query(
            &inputs,
            Q::ItemRemainingUseDuration,
            &[
                MolangValue::String("off_hand".into()),
                MolangValue::Number(2.0),
            ],
        )
        .number(),
        1.0
    );
    assert_eq!(
        query(
            &inputs,
            Q::ItemRemainingUseDuration,
            &[MolangValue::String("invalid".into())],
        )
        .number(),
        0.0
    );
}
