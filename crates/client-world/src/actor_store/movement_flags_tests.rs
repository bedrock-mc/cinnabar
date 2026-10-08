use protocol::{ActorMetadata, ActorMetadataValue};

use super::{ACTOR_FLAG_IMMOBILE, EXTENDED_FLAGS_METADATA_KEY, MovementFlagUpdate};

fn primary(flags: u64) -> ActorMetadata {
    ActorMetadata {
        key: 0,
        value: ActorMetadataValue::Flags(flags),
    }
}

#[test]
fn primary_movement_flags_preserve_explicit_immobile_set_and_clear() {
    let frozen = MovementFlagUpdate::from_metadata(&[primary(1 << ACTOR_FLAG_IMMOBILE)]).unwrap();
    assert_eq!(frozen.immobile, Some(true));
    assert_eq!(frozen.sprinting, Some(false));
    let released = MovementFlagUpdate::from_metadata(&[primary(0)]).unwrap();
    assert_eq!(released.immobile, Some(false));
}

#[test]
fn unrelated_or_unusable_flag_words_do_not_release_immobility() {
    let extended = MovementFlagUpdate::from_metadata(&[ActorMetadata {
        key: EXTENDED_FLAGS_METADATA_KEY,
        value: ActorMetadataValue::FlagsExtended(1),
    }])
    .unwrap();
    assert_eq!(extended.immobile, None);
    for value in [
        ActorMetadataValue::Int(0),
        ActorMetadataValue::String("odd".into()),
    ] {
        assert_eq!(
            MovementFlagUpdate::from_metadata(&[ActorMetadata { key: 0, value }]),
            None,
        );
    }
    let repeated =
        MovementFlagUpdate::from_metadata(&[primary(1 << ACTOR_FLAG_IMMOBILE), primary(0)])
            .unwrap();
    assert_eq!(repeated.immobile, Some(false));
}

#[test]
fn gravity_reads_the_primary_word_and_uniform_air_drag_the_third() {
    let primary_only = MovementFlagUpdate::from_metadata(&[primary(1 << 49)]).unwrap();
    assert_eq!(primary_only.has_gravity, Some(true));
    assert_eq!(primary_only.uniform_air_drag, None);
    let cleared = MovementFlagUpdate::from_metadata(&[primary(0)]).unwrap();
    assert_eq!(cleared.has_gravity, Some(false));
    let third = MovementFlagUpdate::from_metadata(&[ActorMetadata {
        key: protocol::ACTOR_DATA_ID_FLAGS_THIRD,
        value: ActorMetadataValue::FlagsExtended(1),
    }])
    .unwrap();
    assert_eq!(third.uniform_air_drag, Some(true));
    assert_eq!(third.has_gravity, None);
}
