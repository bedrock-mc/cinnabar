use super::*;

#[path = "../../build_support.rs"]
mod build_support;

#[test]
fn generated_groups_cover_every_fixture_value_and_preserve_wire_ids() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("../../data/protocol-ids.json")).unwrap();
    macro_rules! check {
        ($($group:ident),* $(,)?) => {$({
            let values = fixture["groups"][stringify!($group)].as_object().unwrap();
            let extra = usize::from(stringify!($group) == "ActorDataKey");
            assert_eq!($group::ALL.len(), values.len() + extra);
            for variant in $group::ALL {
                let name = format!("{variant:?}");
                if name == "FlagsThird" { continue; }
                let expected = values[&name].as_i64().unwrap();
                assert_eq!(variant.wire_value() as i64, expected, "{}::{name}", stringify!($group));
                assert_eq!($group::from_wire(variant.wire_value()).unwrap().wire_value(), variant.wire_value());
            }
        })*};
    }
    check!(
        ActorDataKey,
        ActorFlag,
        Effect,
        LevelEvent,
        WindowType,
        ContainerName,
        WindowID,
        CommandArgType,
        CommandArgFlag
    );
}

#[test]
fn enchantments_cover_the_vendored_codec_including_sentinels() {
    use bedrock_protocol_1_26_51::enums::EnchantType;
    for raw in u8::MIN..=u8::MAX {
        let vendor = EnchantType::from(raw);
        let named = Enchantment::from_wire(raw);
        assert_eq!(named.is_some(), !matches!(vendor, EnchantType::Unknown(_)));
        if let Some(named) = named {
            assert_eq!(named.wire_value(), vendor.to_raw());
        }
    }
}

#[test]
fn fallback_air_ids_identify_the_active_registry_air_record() {
    let target =
        serde_json::from_str(include_str!("../../../../assets/bedrock-target.json")).unwrap();
    let schema =
        serde_json::from_str(include_str!("../../../assets/data/registry-schema.json")).unwrap();
    let bytes = include_bytes!("../../../assets/data/block-registry-v2193.bin");
    assert_eq!(
        (SEQUENTIAL_AIR_NETWORK_ID, HASHED_AIR_NETWORK_ID),
        build_support::air_ids(bytes, &schema, &target)
    );
}

#[test]
fn third_flag_word_keeps_its_explicit_contract_despite_the_pinned_key_alias() {
    let extensions: serde_json::Value =
        serde_json::from_str(include_str!("../../data/actor-data-extensions.json")).unwrap();
    assert_eq!(
        ActorDataKey::FlagsThird.wire_value() as u64,
        extensions["FlagsThird"].as_u64().unwrap()
    );
    assert_eq!(
        ActorDataKey::FlagsThird.wire_value(),
        ActorDataKey::ArrowShooterID.wire_value()
    );
    assert_ne!(
        ActorDataKey::FlagsThird.wire_value(),
        ActorDataKey::Reserved139.wire_value()
    );
}

#[test]
fn fixed_open_window_lengths_match_the_pinned_container_contract() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("../../data/protocol-ids.json")).unwrap();
    for (name, count) in fixture["slot_counts"].as_object().unwrap() {
        let raw = fixture["groups"]["WindowType"][name].as_i64().unwrap() as i8;
        let Some(kind) = crate::WindowKind::from_window_type(raw) else {
            continue;
        };
        if !kind.is_ui_backed() {
            assert!(
                kind.content_lengths()
                    .contains(&(count.as_u64().unwrap() as usize)),
                "{name}"
            );
        }
    }
}
