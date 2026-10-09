use super::*;

/// Evaluates a query with the supplied actor and animation context.
fn read(
    actor: &ActorSnapshot,
    context: &ActorTickContext,
    name: &str,
    args: &[MolangValue],
) -> f32 {
    let input = ActorTickInput::default();
    query(
        &QueryInputs {
            actor,
            context,
            input: &input,
            anim_tick: 0,
            anim_time: None,
            life_tick: 0,
            finished: (false, false),
            bones: &[],
            bone_names: &[],
        },
        name,
        args,
    )
    .number()
}

#[test]
fn player_animation_approx_eq_compares_all_float_values_exactly() {
    let actor = super::super::tests::actor_with_metadata(HashMap::new());
    let context = ActorTickContext::default();
    let compare = |values: &[f32]| {
        read(
            &actor,
            &context,
            "query.approx_eq",
            &values
                .iter()
                .copied()
                .map(MolangValue::Number)
                .collect::<Vec<_>>(),
        )
    };
    assert_eq!(compare(&[]), 0.0);
    assert_eq!(compare(&[1.0]), 0.0);
    assert_eq!(compare(&[1.0, 1.0, 1.0]), 1.0);
    assert_eq!(compare(&[1.0, 1.0, 2.0]), 0.0);
    assert_eq!(compare(&[1.0, 1.0 + f32::EPSILON]), 0.0);
    assert_eq!(compare(&[-0.0, 0.0]), 1.0);
    assert_eq!(compare(&[f32::NAN, f32::NAN]), 0.0);
    assert_eq!(compare(&[f32::INFINITY, f32::INFINITY]), 1.0);
}

#[test]
fn player_animation_local_query_remains_true_in_third_person_and_hud() {
    let actor = super::super::tests::actor_with_metadata(HashMap::new());
    for (local, first_person, in_ui) in [
        (true, true, false),
        (true, false, false),
        (true, false, true),
        (false, false, false),
    ] {
        let context = ActorTickContext {
            is_local_player: local,
            is_local_first_person: first_person,
            is_in_ui: in_ui,
            ..Default::default()
        };
        assert_eq!(
            read(&actor, &context, "query.is_local_player", &[]),
            if local { 1.0 } else { 0.0 }
        );
    }
}

#[test]
fn player_animation_armor_presence_uses_worn_slots_and_fire_uses_actor_state() {
    let mut actor = super::super::tests::actor_with_metadata(HashMap::new());
    let mut context = ActorTickContext::default();
    context.armor[1] = Some(super::super::tick::WornArmor {
        item: Arc::from("minecraft:elytra"),
        dye_rgb: None,
    });
    let number = MolangValue::Number;
    assert_eq!(
        read(&actor, &context, "query.has_armor_slot", &[number(1.0)]),
        1.0
    );
    for slot in [-1.0, 0.0, 2.0, 4.0, f32::NAN] {
        assert_eq!(
            read(&actor, &context, "query.has_armor_slot", &[number(slot)]),
            0.0
        );
    }
    assert_eq!(read(&actor, &context, "query.has_armor_slot", &[]), 0.0);
    assert_eq!(
        read(
            &actor,
            &context,
            "query.has_armor_slot",
            &[number(1.0), number(1.0)]
        ),
        0.0
    );
    assert_eq!(read(&actor, &context, "query.is_on_fire", &[]), 0.0);
    actor.metadata.insert(0, ActorMetadataValue::Flags(1));
    assert_eq!(read(&actor, &context, "query.is_on_fire", &[]), 1.0);
}
