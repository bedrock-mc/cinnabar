use bytes::{Bytes, BytesMut};
use std::fmt::Debug;
use valentine::bedrock::codec::BedrockCodec;
use valentine::bedrock::version::v1_26_51::types::{
    BookEditActionAddPage, BookEditActionReplacePage, EnumsActorEvent, EnumsActorType,
    EnumsConnectionDisconnectFailReason, EnumsContainerEnumName, EnumsItemStackRequestActionType,
    EnumsLegacyTelemetryEventPacketPayloadType, EnumsPhotoType, EnumsPlayStatus,
    ResourcePackClientResponseCanceljson, ResourcePackClientResponseDownloadingFinishedjson,
    ResourcePackClientResponseDownloadingjson, ResourcePackClientResponsePacketResponse,
    ResourcePackClientResponseResourcePackStackFinishedjson,
};

fn assert_wire<T>(value: T, expected: &[u8])
where
    T: BedrockCodec<Args = ()> + PartialEq + Debug,
{
    let mut encoded = BytesMut::new();
    value.encode(&mut encoded).expect("encode value");
    assert_eq!(encoded.as_ref(), expected);

    let mut input = encoded.freeze();
    let decoded = T::decode(&mut input, ()).expect("decode value");
    assert_eq!(decoded, value);
    assert!(input.is_empty());
}

#[test]
fn generated_unknown_enum_values_keep_their_wire_numbers() {
    // Protocolgen no longer emits placeholder `ReservedN` variants. It does
    // preserve unsupported values through generated unknown-value arms where the
    // authoritative schema marks the enum open.
    assert_wire(EnumsActorEvent::Unknown(9), &[9]);
    assert_wire(EnumsActorEvent::Unknown(255), &[255]);
    assert_wire(EnumsContainerEnumName::Unknown(250), &[250]);
    assert_wire(EnumsItemStackRequestActionType::Unknown(250), &[250]);
    assert_wire(EnumsPlayStatus::Unknown(10), &[0, 0, 0, 10]);
}

#[test]
fn reserved_telemetry_values_keep_their_wire_numbers() {
    assert_wire(EnumsLegacyTelemetryEventPacketPayloadType::Reserved8, &[16]);
    assert_wire(EnumsLegacyTelemetryEventPacketPayloadType::Reserved9, &[18]);
}

#[test]
fn reserved_shared_enum_values_keep_their_wire_numbers() {
    assert_wire(EnumsPlayStatus::Reserved5, &[0, 0, 0, 5]);
    assert_wire(EnumsPlayStatus::Reserved6, &[0, 0, 0, 6]);
    assert_wire(EnumsConnectionDisconnectFailReason::Reserved11, &[22]);
    assert_wire(
        EnumsConnectionDisconnectFailReason::Reserved131,
        &[0x86, 0x02],
    );
    assert_wire(
        EnumsConnectionDisconnectFailReason::Reserved132,
        &[0x88, 0x02],
    );
    assert_wire(EnumsActorType::Reserved78, &[0x9c, 0x01]);
    assert_wire(EnumsActorType::Reserved107, &[0xd6, 0x01]);
    assert_wire(EnumsActorType::Reserved312, &[0xf0, 0x04]);
    assert_wire(EnumsActorType::Reserved318, &[0xfc, 0x04]);
    assert_wire(EnumsActorType::Reserved4194410, &[0xd4, 0x81, 0x80, 0x04]);
    assert_wire(EnumsActorType::Npc, &[0xe6, 0x04]);
    assert_wire(EnumsActorEvent::Reserved36, &[36]);
    assert_wire(EnumsActorEvent::Reserved71, &[71]);
    assert_wire(EnumsActorEvent::Reserved73, &[73]);
    assert_wire(EnumsPhotoType::Reserved0, &[0]);
    assert_wire(EnumsPhotoType::Reserved1, &[1]);
    assert_wire(EnumsPhotoType::Book, &[2]);
    assert_eq!(EnumsPhotoType::default(), EnumsPhotoType::Reserved0);
}

#[test]
fn renamed_book_photo_name_field_keeps_its_wire_shape() {
    assert_wire(
        BookEditActionAddPage {
            page_index: 0,
            page_text: "x".into(),
            photo_name: "y".into(),
        },
        &[0, 1, b'x', 1, b'y'],
    );
    assert_wire(
        BookEditActionReplacePage {
            page_index: 0,
            page_text: "x".into(),
            photo_name: "y".into(),
        },
        &[0, 1, b'x', 1, b'y'],
    );
}

#[test]
fn resource_pack_client_response_keeps_vanilla_wire_numbers() {
    // Vanilla Bedrock numbers this response from zero: cancel=0, downloading=1,
    // downloadingfinished=2, resourcepackstackfinished=3. A one-based tag makes a
    // server read `downloading` as `downloadingfinished` and abandon the pack list.
    assert_wire(
        ResourcePackClientResponsePacketResponse::Cancel(ResourcePackClientResponseCanceljson {
            response_type: "cancel".to_string(),
        }),
        &[0, 6, b'c', b'a', b'n', b'c', b'e', b'l'],
    );
    assert_wire(
        ResourcePackClientResponsePacketResponse::Downloading(
            ResourcePackClientResponseDownloadingjson {
                response_type: "downloading".to_string(),
                downloading_packs: vec!["a_1.0.0".to_string()],
            },
        ),
        &[
            1, 11, b'd', b'o', b'w', b'n', b'l', b'o', b'a', b'd', b'i', b'n', b'g', 1, 7, b'a',
            b'_', b'1', b'.', b'0', b'.', b'0',
        ],
    );
    assert_wire(
        ResourcePackClientResponsePacketResponse::DownloadingFinished(
            ResourcePackClientResponseDownloadingFinishedjson {
                response_type: "downloadingfinished".to_string(),
            },
        ),
        &[
            2, 19, b'd', b'o', b'w', b'n', b'l', b'o', b'a', b'd', b'i', b'n', b'g', b'f', b'i',
            b'n', b'i', b's', b'h', b'e', b'd',
        ],
    );
    assert_wire(
        ResourcePackClientResponsePacketResponse::ResourcePackStackFinished(
            ResourcePackClientResponseResourcePackStackFinishedjson {
                response_type: "resourcepackstackfinished".to_string(),
            },
        ),
        &[
            3, 25, b'r', b'e', b's', b'o', b'u', b'r', b'c', b'e', b'p', b'a', b'c', b'k', b's',
            b't', b'a', b'c', b'k', b'f', b'i', b'n', b'i', b's', b'h', b'e', b'd',
        ],
    );
}

#[test]
fn resource_pack_client_response_selector_is_varuint32() {
    let mut input = Bytes::from_static(&[0x80, 0x01]);
    let error = ResourcePackClientResponsePacketResponse::decode(&mut input, ())
        .expect_err("reserved selector 128 must be rejected after decoding its full varuint32");
    assert!(input.is_empty(), "the two-byte varuint32 must be consumed");
    assert!(matches!(
        error,
        valentine::bedrock::error::DecodeError::InvalidEnumValue { value: 128, .. }
    ));
}
