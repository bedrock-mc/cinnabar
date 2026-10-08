//! Pinned normalized-source ratchet. Runs without a Python installation.
use sha2::{Digest, Sha256};
use valentine::bedrock::{
    borrowed::BedrockBorrowDecode,
    codec::{BedrockCodec, BedrockSized},
    version::v1_26_51::{
        EnumsContainerEnumName, EnumsItemStackRequestActionType,
        EnumsLegacyTelemetryEventPacketPayloadType, ItemStackRequestCerealRequestDataActionsItem,
        ItemStackRequestPacketDataRequestDataActionsItem, LegacyTelemetryEventPacketEventData,
        ReservedStackRequestAction9, ReservedStackRequestAction9View,
    },
};

const MANIFEST: &str = include_str!("../../../../tools/protocol-normalize/manifest.json");
const SOURCES: [(&str, &[u8]); 5] = [
    (
        "common.rs",
        include_bytes!("../../vendor/valentine/bedrock_versions/v1_26_51/src/common.rs"),
    ),
    (
        "mcpe.rs",
        include_bytes!("../../vendor/valentine/bedrock_versions/v1_26_51/src/mcpe.rs"),
    ),
    (
        "proto.rs",
        include_bytes!("../../vendor/valentine/bedrock_versions/v1_26_51/src/proto.rs"),
    ),
    (
        "types.rs",
        include_bytes!("../../vendor/valentine/bedrock_versions/v1_26_51/src/types.rs"),
    ),
    (
        "borrowed.rs",
        include_bytes!("../../vendor/valentine/bedrock_versions/v1_26_51/src/borrowed.rs"),
    ),
];

#[test]
fn generated_reservations_match_the_complete_canonical_source_fingerprints() {
    let manifest: serde_json::Value = serde_json::from_str(MANIFEST).unwrap();
    for (name, bytes) in SOURCES {
        let text = std::str::from_utf8(bytes).unwrap().replace("\r\n", "\n");
        let actual = format!("{:x}", Sha256::digest(text.as_bytes()));
        assert_eq!(
            actual,
            manifest["normalized_sha256"][name].as_str().unwrap(),
            "{name}: run the pinned normalization check"
        );
    }
}

fn assert_round_trip<T: BedrockCodec<Args = ()> + BedrockSized>(bytes: &[u8]) -> T {
    let mut input = bytes;
    let value = T::decode(&mut input, ()).unwrap();
    assert!(input.is_empty());
    assert_eq!(value.encoded_size(), bytes.len());
    let mut output = Vec::new();
    value.encode(&mut output).unwrap();
    assert_eq!(output, bytes);
    value
}

#[test]
fn reserved_container_enum_values_round_trip() {
    for value in 35..=40 {
        let _: EnumsContainerEnumName = assert_round_trip(&[value]);
    }
}

#[test]
fn reserved_action_preserves_both_independent_wire_tags_and_borrowed_payload() {
    let action: EnumsItemStackRequestActionType = assert_round_trip(&[9]);
    assert_eq!(action, EnumsItemStackRequestActionType::Reserved9);
    let cereal: ItemStackRequestCerealRequestDataActionsItem = assert_round_trip(&[7, 9]);
    assert!(matches!(
        cereal,
        ItemStackRequestCerealRequestDataActionsItem::Reserved9(_)
    ));
    let packet: ItemStackRequestPacketDataRequestDataActionsItem = assert_round_trip(&[7, 9]);
    assert!(matches!(
        packet,
        ItemStackRequestPacketDataRequestDataActionsItem::Reserved9(_)
    ));
    let mut input = bytes::Bytes::from_static(&[9]);
    let borrowed = ReservedStackRequestAction9View::borrow_decode(&mut input, ()).unwrap();
    assert!(input.is_empty());
    let owned: ReservedStackRequestAction9 = borrowed.into();
    assert_eq!(
        owned.reserved_field_0,
        EnumsItemStackRequestActionType::Reserved9
    );
    for bytes in [&[][..], &[7][..]] {
        let mut input = bytes;
        assert!(ItemStackRequestCerealRequestDataActionsItem::decode(&mut input, ()).is_err());
        let mut input = bytes;
        assert!(ItemStackRequestPacketDataRequestDataActionsItem::decode(&mut input, ()).is_err());
    }
    let retail: ItemStackRequestCerealRequestDataActionsItem =
        assert_round_trip(&[9, 11, 0, 0, 0, 0, 0, 0]);
    assert!(matches!(
        retail,
        ItemStackRequestCerealRequestDataActionsItem::MineBlockActionData(_)
    ));
}

#[test]
fn reserved_event_alternatives_preserve_enum_and_union_selectors() {
    let first: EnumsLegacyTelemetryEventPacketPayloadType = assert_round_trip(&[52]);
    let second: EnumsLegacyTelemetryEventPacketPayloadType = assert_round_trip(&[54]);
    assert_eq!(
        first,
        EnumsLegacyTelemetryEventPacketPayloadType::Reserved26
    );
    assert_eq!(
        second,
        EnumsLegacyTelemetryEventPacketPayloadType::Reserved27
    );
    let first: LegacyTelemetryEventPacketEventData = assert_round_trip(&[18, 0]);
    let second: LegacyTelemetryEventPacketEventData = assert_round_trip(&[19, 0, 3]);
    assert!(matches!(
        first,
        LegacyTelemetryEventPacketEventData::Reserved18(_)
    ));
    assert!(matches!(
        second,
        LegacyTelemetryEventPacketEventData::Reserved19(_)
    ));
}
