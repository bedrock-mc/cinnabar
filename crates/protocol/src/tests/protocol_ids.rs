use protocol_ids::*;

#[test]
fn generated_groups_cover_every_fixture_value_and_preserve_wire_ids() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("../../../protocol-ids/data/protocol-ids.json")).unwrap();
    let extensions: serde_json::Value = serde_json::from_str(include_str!(
        "../../../protocol-ids/data/actor-data-extensions.json"
    ))
    .unwrap();
    macro_rules! check {
        ($($group:ident),* $(,)?) => {$({
            let values = fixture["groups"][stringify!($group)].as_object().unwrap();
            let extra = match stringify!($group) { "ActorDataKey" => extensions.as_object().unwrap().len(), "Effect" => 1, _ => 0 };
            assert_eq!($group::ALL.len(), values.len() + extra);
            for variant in $group::ALL {
                let name = format!("{variant:?}");
                if (stringify!($group) == "ActorDataKey" && extensions.get(&name).is_some()) || name == "Weaving" { continue; }
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
fn third_flag_word_keeps_its_explicit_contract_despite_the_pinned_key_alias() {
    let extensions: serde_json::Value = serde_json::from_str(include_str!(
        "../../../protocol-ids/data/actor-data-extensions.json"
    ))
    .unwrap();
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
        serde_json::from_str(include_str!("../../../protocol-ids/data/protocol-ids.json")).unwrap();
    for (name, count) in fixture["slot_counts"].as_object().unwrap() {
        let raw = fixture["groups"]["WindowType"][name].as_i64().unwrap() as i8;
        let Some(kind) = protocol::WindowKind::from_window_type(raw) else {
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

/// Checks movement names and word-specific flags against their explicit contract.
#[test]
fn movement_extensions_match_the_named_contract() {
    let contract: serde_json::Value = serde_json::from_str(include_str!(
        "../../../protocol-ids/data/movement-contract.json"
    ))
    .unwrap();
    assert_eq!(
        Attribute::ALL.len(),
        contract["attributes"].as_object().unwrap().len()
    );
    for attribute in Attribute::ALL {
        assert_eq!(
            attribute.wire_name(),
            contract["attributes"][format!("{attribute:?}")]
                .as_str()
                .unwrap()
        );
    }
    for flag in HorseFlag::ALL {
        assert_eq!(
            u64::from(flag.wire_value()),
            contract["numeric"]["HorseFlag"][format!("{flag:?}")]
                .as_u64()
                .unwrap()
        );
    }
    for flag in PlayerFlag::ALL {
        assert_eq!(
            u64::from(flag.wire_value()),
            contract["numeric"]["PlayerFlag"][format!("{flag:?}")]
                .as_u64()
                .unwrap()
        );
    }
    assert_eq!(
        Effect::Weaving.wire_value() as i64,
        contract["numeric"]["Effect"]["Weaving"].as_i64().unwrap()
    );
    assert_eq!(
        protocol_ids::SPRINT_SPEED_MODIFIER_ID,
        contract["sprint_speed_modifier_id"].as_str().unwrap()
    );
}

#[test]
fn active_nameplate_distance_key_is_distinct_from_the_pinned_version_name() {
    assert_eq!(ActorDataKey::NameplateRenderDistance.wire_value(), 140);
    assert_ne!(
        ActorDataKey::NameplateRenderDistance.wire_value(),
        ActorDataKey::NameplateRenderDistanceMax.wire_value()
    );
}
