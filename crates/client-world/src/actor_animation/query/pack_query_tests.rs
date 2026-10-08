use super::*;
use crate::actor_store::properties::{PropertyDefinition, PropertyKind};

fn read(context: &ActorTickContext, name: &str, arguments: &[MolangValue]) -> f32 {
    let actor = crate::actor_animation::tests::actor_with_metadata(HashMap::new());
    let input = ActorTickInput {
        position: [1.0, 2.0, 3.0],
        ..ActorTickInput::default()
    };
    query(
        &QueryInputs {
            actor: &actor,
            input: &input,
            context,
            anim_tick: 0,
            anim_time: None,
            life_tick: 0,
            finished: (false, false),
            bones: &[],
            bone_names: &[],
        },
        name,
        arguments,
    )
    .number()
}

#[test]
fn camera_distance_ranges_clamp_reverse_and_jump_for_shared_endpoints() {
    let context = ActorTickContext {
        camera_position: [4.0, 6.0, 3.0],
        ..ActorTickContext::default()
    };
    assert_eq!(read(&context, "query.distance_from_camera", &[]), 5.0);
    for (start, end, expected) in [
        (2.0, 6.0, 0.75),
        (6.0, 2.0, 0.25),
        (5.0, 9.0, 0.0),
        (1.0, 5.0, 1.0),
        (9.0, 5.0, 1.0),
        (5.0, 1.0, 0.0),
        (5.0, 5.0, 0.0),
        (4.0, 4.0, 1.0),
        (6.0, 6.0, 0.0),
    ] {
        let arguments = [MolangValue::Number(start), MolangValue::Number(end)];
        assert_eq!(
            read(&context, "query.camera_distance_range_lerp", &arguments),
            expected,
            "range {start}..{end}"
        );
    }
    assert_eq!(read(&context, "query.camera_distance_range_lerp", &[]), 0.0);
    assert_eq!(
        read(
            &context,
            "query.camera_distance_range_lerp",
            &[MolangValue::Number(1.0)]
        ),
        0.0
    );
}

#[test]
fn has_property_checks_declaration_even_when_its_default_is_zero_or_unsynced() {
    let context = ActorTickContext {
        properties: Some(Arc::from([
            PropertyDefinition {
                name: "custom:hat".into(),
                kind: PropertyKind::Number,
                default: 0.0,
            },
            PropertyDefinition {
                name: "custom:style".into(),
                kind: PropertyKind::Enum(Arc::from([Arc::from("plain")])),
                default: 0.0,
            },
        ])),
        ..ActorTickContext::default()
    };
    for name in ["custom:hat", "custom:style"] {
        assert_eq!(
            read(
                &context,
                "query.has_property",
                &[MolangValue::String(name.into())]
            ),
            1.0
        );
    }
    for arguments in [
        vec![],
        vec![MolangValue::Number(0.0)],
        vec![MolangValue::String("custom:missing".into())],
        vec![
            MolangValue::String("custom:hat".into()),
            MolangValue::Number(0.0),
        ],
    ] {
        assert_eq!(read(&context, "query.has_property", &arguments), 0.0);
    }
    assert_eq!(
        read(
            &ActorTickContext::default(),
            "query.has_property",
            &[MolangValue::String("custom:hat".into())]
        ),
        0.0
    );
}

#[test]
fn has_armor_slot_reads_worn_stacks_and_rejects_invalid_arguments() {
    let mut context = ActorTickContext::default();
    context.armor[3] = Some(WornArmor {
        item: "minecraft:diamond_boots".into(),
        dye_rgb: None,
    });
    context.armor[4] = Some(WornArmor {
        item: "minecraft:wolf_armor".into(),
        dye_rgb: None,
    });
    for (slot, expected) in [
        (3.0, 1.0),
        (3.9, 1.0),
        (0.0, 0.0),
        (-0.1, 0.0),
        (4.0, 0.0),
        (5.0, 0.0),
    ] {
        assert_eq!(
            read(
                &context,
                "query.has_armor_slot",
                &[MolangValue::Number(slot)]
            ),
            expected
        );
    }
    for arguments in [
        vec![],
        vec![MolangValue::Number(f32::NAN)],
        vec![MolangValue::Number(f32::INFINITY)],
        vec![MolangValue::Number(3.0), MolangValue::Number(0.0)],
    ] {
        assert_eq!(read(&context, "query.has_armor_slot", &arguments), 0.0);
    }
}

#[test]
fn base_swing_query_reads_unmodified_duration_in_seconds() {
    let context = ActorTickContext::default();
    let duration =
        super::super::motion::ACTOR_SWING_TICKS as f32 * ACTOR_TICK_DURATION.as_secs_f32();
    assert_eq!(read(&context, "query.base_swing_duration", &[]), duration);
    assert_eq!(
        read(
            &context,
            "query.base_swing_duration",
            &[MolangValue::Number(0.0)]
        ),
        0.0
    );
}
