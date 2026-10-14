use super::*;

/// Evaluates one typed query with retained item facts and a pinned actor fixture.
fn read(context: &ActorTickContext, query_id: Q, arguments: &[MolangValue]) -> f32 {
    let actor = crate::tests::actor_with_metadata(HashMap::new());
    query(
        &QueryInputs {
            actor: &actor,
            input: &ActorTickInput::default(),
            context,
            anim_tick: 0,
            anim_time: None,
            swell_amount: None,
            life_tick: 0,
            finished: (false, false),
            state_time: 0.0,
            bones: &[],
            bone_names: &[],
        },
        query_id,
        arguments,
    )
    .number()
}

#[test]
fn equipped_tag_queries_use_both_admitted_hands_and_any_pinned_tag() {
    let facts = assets::facts::ItemFacts::builtin().unwrap();
    let context = ActorTickContext {
        held_item_facts: [
            *facts.by_identifier("minecraft:iron_sword").unwrap(),
            *facts.by_identifier("minecraft:wooden_spear").unwrap(),
        ],
        ..Default::default()
    };
    let arguments = |slot: &str, tag: &str| {
        [
            MolangValue::String(slot.into()),
            MolangValue::String(tag.into()),
        ]
    };
    for (slot, tag, expected) in [
        ("slot.weapon.mainhand", "minecraft:is_sword", 1.0),
        ("slot.weapon.mainhand", "minecraft:is_spear", 0.0),
        ("slot.weapon.offhand", "minecraft:is_spear", 1.0),
        ("slot.weapon.offhand", "minecraft:is_sword", 0.0),
    ] {
        assert_eq!(
            read(&context, Q::EquippedItemAnyTag, &arguments(slot, tag)),
            expected
        );
    }
}

#[test]
fn armour_queries_preserve_material_indices_tint_and_glider_admission() {
    let facts = assets::facts::ItemFacts::builtin().unwrap();
    for (identifier, expected) in [
        ("minecraft:leather_chestplate", 1.0),
        ("minecraft:iron_chestplate", 2.0),
        ("minecraft:golden_chestplate", 3.0),
        ("minecraft:diamond_chestplate", 4.0),
        ("minecraft:copper_chestplate", 5.0),
        ("minecraft:netherite_chestplate", 6.0),
        ("minecraft:elytra", 5.0),
        ("minecraft:golden_horse_armor", 3.0),
        ("minecraft:leather_horse_armor", 1.0),
    ] {
        let mut context = ActorTickContext::default();
        context.armor[1] = Some(super::super::tick::WornArmor {
            item: identifier.into(),
            fact: *facts.by_identifier(identifier).unwrap(),
            dye_rgb: None,
        });
        assert_eq!(armor_texture_slot(&context, 1.0), expected);
        assert_eq!(wearing_elytra(&context), identifier == "minecraft:elytra");
        let expected_red = if facts.by_identifier(identifier).unwrap().is_leather() {
            (assets::DEFAULT_LEATHER_RGB >> 16) as f32 / 255.0
        } else {
            1.0
        };
        assert_eq!(
            armor_color_slot(&context, Some(1.0), Some(0.0)),
            expected_red
        );
    }
}
