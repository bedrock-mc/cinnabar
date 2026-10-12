use std::fmt::Debug;

use bedrock_protocol_1_26_51::enums::{
    ActorEventType, ActorType, ConnectionDisconnectFailReason, ContainerEnumName,
    ItemStackRequestActionType, LegacyTelemetryType, PhotoType, PlayStatusType,
};
use bedrock_protocol_1_26_51::types::{
    BookEditAction, EventData, ResourcePackClientResponseData, StackRequestAction,
};
use bedrock_protocol_1_26_51::wire::{self, Decode, Encode};

fn assert_wire<T>(value: T, expected: &[u8])
where
    T: Encode + Decode + PartialEq + Debug,
{
    let encoded = value.encode_to_vec().expect("encode value");
    assert_eq!(encoded.as_slice(), expected);
    assert_eq!(T::decode_exact(&encoded).expect("decode value"), value);
}

#[test]
fn generated_unknown_enum_values_keep_their_wire_numbers() {
    // Unsupported values survive through the generated unknown-value arms.
    assert_wire(ActorEventType::Unknown(9), &[9]);
    assert_wire(ActorEventType::Unknown(255), &[255]);
    assert_wire(ContainerEnumName::Unknown(250), &[250]);
    assert_wire(ItemStackRequestActionType::Unknown(250), &[250]);
    assert_wire(PlayStatusType::Unknown(10), &[0, 0, 0, 10]);
}

#[test]
fn education_telemetry_values_keep_their_wire_numbers() {
    assert_wire(LegacyTelemetryType::AgentcommandObsolete, &[16]);
    assert_wire(LegacyTelemetryType::Agentcreated, &[18]);
}

#[test]
fn education_shared_enum_values_keep_their_wire_numbers() {
    assert_wire(
        PlayStatusType::LoginfailedEditionmismatchedutovanilla,
        &[0, 0, 0, 5],
    );
    assert_wire(
        PlayStatusType::LoginfailedEditionmismatchvanillatoedu,
        &[0, 0, 0, 6],
    );
    assert_wire(
        ConnectionDisconnectFailReason::Edulevelsettingsmissing,
        &[22],
    );
    assert_wire(
        ConnectionDisconnectFailReason::Editionmismatchvanillatoedu,
        &[0x86, 0x02],
    );
    assert_wire(
        ConnectionDisconnectFailReason::Editionmismatchedutovanilla,
        &[0x88, 0x02],
    );
    assert_wire(ActorType::Chalkboard, &[0x9c, 0x01]);
    assert_wire(ActorType::Balloon, &[0xd6, 0x01]);
    assert_wire(ActorType::Agent, &[0xf0, 0x04]);
    assert_wire(ActorType::Tripodcamera, &[0xfc, 0x04]);
    assert_wire(ActorType::Icebomb, &[0xd4, 0x81, 0x80, 0x04]);
    assert_wire(ActorType::Npc, &[0xe6, 0x04]);
    assert_wire(ActorEventType::AgentSwingArm, &[36]);
    assert_wire(ActorEventType::BalloonPop, &[71]);
    assert_wire(ActorEventType::SummonAgent, &[73]);
    assert_wire(PhotoType::Portfolio, &[0]);
    assert_wire(PhotoType::Photoitem, &[1]);
    assert_wire(PhotoType::Book, &[2]);
    assert_eq!(PhotoType::default(), PhotoType::Portfolio);
}

#[test]
fn book_page_actions_keep_their_selector_and_photo_name() {
    assert_wire(
        BookEditAction::AddPage {
            page_index: wire::ZigZag32(0),
            page_text: "x".into(),
            photo_name: "y".into(),
        },
        &[1, 0, 1, b'x', 1, b'y'],
    );
    assert_wire(
        BookEditAction::ReplacePage {
            page_index: wire::ZigZag32(0),
            page_text: "x".into(),
            photo_name: "y".into(),
        },
        &[0, 0, 1, b'x', 1, b'y'],
    );
}

#[test]
fn resource_pack_client_response_keeps_vanilla_wire_numbers() {
    // Vanilla Bedrock numbers this response from zero: cancel=0, downloading=1,
    // downloadingfinished=2, resourcepackstackfinished=3. A one-based tag makes a
    // server read `downloading` as `downloadingfinished` and abandon the pack list.
    assert_wire(
        ResourcePackClientResponseData::Cancel {
            response_type: "cancel".into(),
        },
        &[0, 6, b'c', b'a', b'n', b'c', b'e', b'l'],
    );
    assert_wire(
        ResourcePackClientResponseData::Downloading {
            response_type: "downloading".into(),
            downloading_packs: vec!["a_1.0.0".into()],
        },
        &[
            1, 11, b'd', b'o', b'w', b'n', b'l', b'o', b'a', b'd', b'i', b'n', b'g', 1, 7, b'a',
            b'_', b'1', b'.', b'0', b'.', b'0',
        ],
    );
    assert_wire(
        ResourcePackClientResponseData::DownloadingFinished {
            response_type: "downloadingfinished".into(),
        },
        &[
            2, 19, b'd', b'o', b'w', b'n', b'l', b'o', b'a', b'd', b'i', b'n', b'g', b'f', b'i',
            b'n', b'i', b's', b'h', b'e', b'd',
        ],
    );
    assert_wire(
        ResourcePackClientResponseData::ResourcePackStackFinished {
            response_type: "resourcepackstackfinished".into(),
        },
        &[
            3, 25, b'r', b'e', b's', b'o', b'u', b'r', b'c', b'e', b'p', b'a', b'c', b'k', b's',
            b't', b'a', b'c', b'k', b'f', b'i', b'n', b'i', b's', b'h', b'e', b'd',
        ],
    );
}

#[test]
fn resource_pack_client_response_selector_is_varuint32() {
    let input = [0x80, 0x01];
    let mut reader = wire::Reader::new(&input);
    let error = ResourcePackClientResponseData::decode(&mut reader)
        .expect_err("reserved selector 128 must be rejected after decoding its full varuint32");
    assert_eq!(
        reader.position(),
        2,
        "the two-byte varuint32 must be consumed"
    );
    assert!(matches!(
        error,
        wire::DecodeError::UnknownVariant { value: 128, .. }
    ));
}

/// Education container names 35..=40 decode as named values and re-encode byte for byte.
#[test]
fn education_container_names_round_trip() {
    for value in 35..=40_u8 {
        let name = ContainerEnumName::decode_exact(&[value]).expect("named container");
        assert!(!matches!(name, ContainerEnumName::Unknown(_)), "{value}");
        assert_wire(name, &[value]);
    }
}

/// The lab-table action keeps both its union selector (7) and its action type (9), and is
/// distinct from the retail mine-block action.
#[test]
fn lab_table_stack_action_keeps_both_wire_tags() {
    assert_wire(ItemStackRequestActionType::Screenlabtablecombine, &[9]);
    assert_wire(
        StackRequestAction::LabTableCombineActionData {
            action_type: ItemStackRequestActionType::Screenlabtablecombine,
        },
        &[7, 9],
    );
    for truncated in [&[][..], &[7][..]] {
        assert!(StackRequestAction::decode_exact(truncated).is_err());
    }
    let retail =
        StackRequestAction::decode_exact(&[9, 11, 0, 0, 0, 0, 0, 0]).expect("mine-block action");
    assert!(matches!(
        retail,
        StackRequestAction::MineBlockActionData { .. }
    ));
    assert_eq!(retail.encode_to_vec().unwrap(), [9, 11, 0, 0, 0, 0, 0, 0]);
}

/// Code Builder telemetry keeps its enum values and its union selectors 18 and 19.
#[test]
fn code_builder_telemetry_keeps_enum_and_union_selectors() {
    assert_wire(LegacyTelemetryType::Codebuilderruntimeaction, &[52]);
    assert_wire(LegacyTelemetryType::Codebuilderscoreboard, &[54]);
    assert_wire(
        EventData::CodeBuilderRuntimeAction {
            code_builder_runtime_action: wire::Str::default(),
        },
        &[18, 0],
    );
    assert_wire(
        EventData::CodeBuilderScoreboard {
            objective_name: wire::Str::default(),
            score: wire::ZigZag32(-2),
        },
        &[19, 0, 3],
    );
}
