use super::*;
use crate::{test_actor::TestActor, tests::actor_with_metadata};
use assets::dye::actor_palette_color;
use protocol::ActorKind;
use std::collections::HashMap;

const COLOR: u32 = ActorDataKey::ColorIndex.wire_value();
const NAME: u32 = ActorDataKey::Name.wire_value();
const RED: [f32; 4] = [176.0 / 255.0, 46.0 / 255.0, 38.0 / 255.0, 0.0];

fn actor(class: NativeEntityClass, metadata: &[(u32, ActorMetadataValue)]) -> TestActor {
    let mut actor = actor_with_metadata(metadata.iter().cloned().collect::<HashMap<_, _>>());
    actor.kind = ActorKind::Entity {
        identifier: class.identifier().unwrap().into(),
    };
    actor
}

fn sheep(metadata: &[(u32, ActorMetadataValue)]) -> TestActor {
    actor(NativeEntityClass::Sheep, metadata)
}

fn named(name: &str) -> (u32, ActorMetadataValue) {
    (NAME, ActorMetadataValue::String(name.into()))
}

fn blend(from: u8, to: u8, weight: f32) -> [f32; 4] {
    let (from, to) = (actor_palette_color(from), actor_palette_color(to));
    std::array::from_fn(|channel| from[channel] * (1.0 - weight) + to[channel] * weight)
}

#[test]
fn sheep_wool_takes_the_byte_dye_palette_entry() {
    assert_eq!(
        default_render_color(&sheep(&[(COLOR, ActorMetadataValue::Byte(14))]), 7),
        RED
    );
    for index in 0..16 {
        let actor = sheep(&[(COLOR, ActorMetadataValue::Byte(index))]);
        assert_eq!(
            default_render_color(&actor, 0),
            actor_palette_color(index as u8)
        );
    }
    // The index keeps its low four bits, as the native read does.
    assert_eq!(
        default_render_color(&sheep(&[(COLOR, ActorMetadataValue::Byte(-2))]), 0),
        RED
    );
}

#[test]
fn white_absent_or_mistyped_sheep_colour_stays_neutral() {
    for metadata in [
        vec![(COLOR, ActorMetadataValue::Byte(0))],
        vec![],
        vec![(COLOR, ActorMetadataValue::Int(14))],
        vec![(COLOR, ActorMetadataValue::Long(14))],
    ] {
        assert_eq!(default_render_color(&sheep(&metadata), 3), [1.0; 4]);
    }
}

#[test]
fn only_sheep_and_tamed_wolves_inherit_their_colour_index() {
    let dyed = [(COLOR, ActorMetadataValue::Byte(14)), named(RAINBOW_NAME)];
    let mut ordinary = sheep(&dyed);
    ordinary.kind = ActorKind::Entity {
        identifier: "minecraft:cow".into(),
    };
    assert_eq!(default_render_color(&ordinary, 30), [1.0; 4]);
    let wolf = actor(NativeEntityClass::Wolf, &dyed);
    assert_eq!(default_render_color(&wolf, 30), [1.0; 4]);
    let mut tamed = wolf;
    tamed.metadata.insert(
        0,
        ActorMetadataValue::Flags(1 << ActorFlag::Tamed.wire_value()),
    );
    assert_eq!(default_render_color(&tamed, 30), RED);
}

#[test]
fn jeb_sheep_blend_palette_entries_every_25_ticks() {
    let mut actor = sheep(&[(COLOR, ActorMetadataValue::Byte(14)), named("jeb_")]);
    actor.unique_id = 0;
    assert_eq!(default_render_color(&actor, 0), actor_palette_color(0));
    assert_eq!(default_render_color(&actor, 25), actor_palette_color(1));
    assert_eq!(default_render_color(&actor, 30), blend(1, 2, 0.2));
    assert_eq!(default_render_color(&actor, 15 * 25 + 5), blend(15, 0, 0.2));
    assert_eq!(
        default_render_color(&actor, 16 * 25),
        actor_palette_color(0)
    );
}

#[test]
fn jeb_sheep_offset_their_phase_by_wrapping_unique_id_arithmetic() {
    let mut actor = sheep(&[named("jeb_")]);
    // 0x2106547 = 25 * 1_385_157 + 10, and 1_385_157 & 15 = 5.
    actor.unique_id = 1;
    assert_eq!(default_render_color(&actor, 0), blend(5, 6, 0.4));
    // Only the low 32 bits contribute: -0x2106547 mod 2^32 = 25 * 170_413_534 + 11.
    actor.unique_id = -1 - (1 << 40);
    assert_eq!(default_render_color(&actor, 0), blend(14, 15, 0.44));
}

#[test]
fn only_the_exact_string_name_jeb_selects_the_rainbow() {
    for name in [
        named("Jeb_"),
        named("jeb_ "),
        named("jeb"),
        (NAME, ActorMetadataValue::Int(0)),
    ] {
        let actor = sheep(&[(COLOR, ActorMetadataValue::Byte(14)), name]);
        assert_eq!(default_render_color(&actor, 30), RED);
    }
}
