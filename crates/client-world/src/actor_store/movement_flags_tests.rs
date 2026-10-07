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
