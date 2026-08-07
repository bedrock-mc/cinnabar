use bytes::{Bytes, BytesMut};
use std::fmt::Debug;
use valentine::bedrock::codec::BedrockCodec;
use valentine::bedrock::version::v1_26_40::CraftingDataPacket;
use valentine::bedrock::version::v1_26_40::types::{
    ActorEventPacketEventId, BookEditActionAddPage, BookEditActionReplacePage,
    CraftingDataReservedEntry, CraftingDataReservedEntryOutput, DisconnectPacketReason,
    FullContainerNameContainerName, InteractPacketAction,
    ItemStackRequestCerealBeaconPaymentActionDataActiontype,
    ItemStackRequestCerealConsumeActionDataActiontype,
    ItemStackRequestCerealCraftCreativeActionDataActiontype,
    ItemStackRequestCerealCraftLoomActionDataActiontype,
    ItemStackRequestCerealCraftNonImplementedActionDataActiontype,
    ItemStackRequestCerealCraftRecipeActionDataActiontype,
    ItemStackRequestCerealCraftRecipeAutoActionDataActiontype,
    ItemStackRequestCerealCraftRecipeOptionalActionDataActiontype,
    ItemStackRequestCerealCraftRepairAndDisenchantActionDataActiontype,
    ItemStackRequestCerealCraftResultsActionDataActiontype,
    ItemStackRequestCerealCreateActionDataActiontype,
    ItemStackRequestCerealDestroyActionDataActiontype,
    ItemStackRequestCerealDropActionDataActiontype,
    ItemStackRequestCerealMineBlockActionDataActiontype,
    ItemStackRequestCerealPlaceActionDataActiontype, ItemStackRequestCerealRequestDataActionsItem,
    ItemStackRequestCerealSwapActionDataActiontype, ItemStackRequestCerealTakeActionDataActiontype,
    ItemStackRequestPacketDataRequestDataActionsItem, ItemStackRequestReserved7ActionData,
    LegacySetSlotContainerEnum, LevelSettings, LevelSettingsReserved12, LevelSettingsReserved44,
    PlayStatusPacketStatus,
};

fn assert_wire<T>(value: T, expected: &[u8])
where
    T: BedrockCodec<Args = ()> + PartialEq + Debug,
{
    let mut encoded = BytesMut::new();
    value.encode(&mut encoded).expect("encode reserved value");
    assert_eq!(encoded.as_ref(), expected);

    let mut input = encoded.freeze();
    let decoded = T::decode(&mut input, ()).expect("decode reserved value");
    assert_eq!(decoded, value);
    assert!(input.is_empty());
}

#[test]
fn mixed_reserved_enum_values_keep_their_wire_numbers() {
    assert_wire(LevelSettingsReserved12::Reserved0, &[0]);
    assert_wire(LevelSettingsReserved12::Reserved1, &[1]);
    assert_wire(LevelSettingsReserved12::Reserved2, &[2]);

    for (value, byte) in [
        (FullContainerNameContainerName::Reserved35, 35),
        (FullContainerNameContainerName::Reserved36, 36),
        (FullContainerNameContainerName::Reserved37, 37),
        (FullContainerNameContainerName::Reserved38, 38),
        (FullContainerNameContainerName::Reserved39, 39),
        (FullContainerNameContainerName::Reserved40, 40),
    ] {
        assert_wire(value, &[byte]);
    }
    for (value, byte) in [
        (LegacySetSlotContainerEnum::Reserved35, 35),
        (LegacySetSlotContainerEnum::Reserved36, 36),
        (LegacySetSlotContainerEnum::Reserved37, 37),
        (LegacySetSlotContainerEnum::Reserved38, 38),
        (LegacySetSlotContainerEnum::Reserved39, 39),
        (LegacySetSlotContainerEnum::Reserved40, 40),
    ] {
        assert_wire(value, &[byte]);
    }

    assert_wire(PlayStatusPacketStatus::Reserved4, &[0, 0, 0, 4]);
    assert_wire(PlayStatusPacketStatus::Reserved5, &[0, 0, 0, 5]);
    assert_wire(PlayStatusPacketStatus::Reserved6, &[0, 0, 0, 6]);

    for (value, expected) in [
        (DisconnectPacketReason::Reserved11, &[0x16][..]),
        (DisconnectPacketReason::Reserved27, &[0x36][..]),
        (DisconnectPacketReason::Reserved28, &[0x38][..]),
        (DisconnectPacketReason::Reserved47, &[0x5e][..]),
        (DisconnectPacketReason::Reserved109, &[0xda, 0x01][..]),
        (DisconnectPacketReason::Reserved131, &[0x86, 0x02][..]),
        (DisconnectPacketReason::Reserved132, &[0x88, 0x02][..]),
    ] {
        assert_wire(value, expected);
    }

    assert_wire(ActorEventPacketEventId::Reserved36, &[36]);
    assert_wire(ActorEventPacketEventId::Reserved71, &[71]);
    assert_wire(ActorEventPacketEventId::Reserved73, &[73]);
    assert_wire(InteractPacketAction::Reserved5, &[5]);
}

#[test]
fn reserved_struct_fields_and_item_stack_tag_are_neutral() {
    assert_wire(
        LevelSettingsReserved44 {
            reserved_0: "a".into(),
            reserved_1: "b".into(),
        },
        &[1, b'a', 1, b'b'],
    );
    assert_wire(
        CraftingDataReservedEntry {
            reserved_0: 3,
            reserved_1: vec![CraftingDataReservedEntryOutput {
                reserved_0: 4,
                reserved_1: 5,
            }],
        },
        &[6, 1, 8, 10],
    );
    assert_wire(
        BookEditActionAddPage {
            page_index: 0,
            page_text: "x".into(),
            reserved_2: "y".into(),
        },
        &[0, 1, b'x', 1, b'y'],
    );
    assert_wire(
        BookEditActionReplacePage {
            page_index: 0,
            page_text: "x".into(),
            reserved_2: "y".into(),
        },
        &[0, 1, b'x', 1, b'y'],
    );

    let cereal = ItemStackRequestCerealRequestDataActionsItem::Reserved7ActionData(
        ItemStackRequestReserved7ActionData { reserved_0: 0xa5 },
    );
    assert_wire(cereal, &[7, 0xa5]);
    let packet = ItemStackRequestPacketDataRequestDataActionsItem::Reserved7ActionData(
        ItemStackRequestReserved7ActionData { reserved_0: 0x5a },
    );
    assert_wire(packet, &[7, 0x5a]);
}

#[test]
fn every_retained_action_type_uses_reserved_value_nine() {
    macro_rules! assert_reserved_nine {
        ($type:ty) => {
            assert_wire(<$type>::Reserved9, &[9]);
        };
    }

    assert_reserved_nine!(ItemStackRequestCerealBeaconPaymentActionDataActiontype);
    assert_reserved_nine!(ItemStackRequestCerealConsumeActionDataActiontype);
    assert_reserved_nine!(ItemStackRequestCerealCraftCreativeActionDataActiontype);
    assert_reserved_nine!(ItemStackRequestCerealCraftLoomActionDataActiontype);
    assert_reserved_nine!(ItemStackRequestCerealCraftNonImplementedActionDataActiontype);
    assert_reserved_nine!(ItemStackRequestCerealCraftRecipeActionDataActiontype);
    assert_reserved_nine!(ItemStackRequestCerealCraftRecipeAutoActionDataActiontype);
    assert_reserved_nine!(ItemStackRequestCerealCraftRecipeOptionalActionDataActiontype);
    assert_reserved_nine!(ItemStackRequestCerealCraftRepairAndDisenchantActionDataActiontype);
    assert_reserved_nine!(ItemStackRequestCerealCraftResultsActionDataActiontype);
    assert_reserved_nine!(ItemStackRequestCerealCreateActionDataActiontype);
    assert_reserved_nine!(ItemStackRequestCerealDestroyActionDataActiontype);
    assert_reserved_nine!(ItemStackRequestCerealDropActionDataActiontype);
    assert_reserved_nine!(ItemStackRequestCerealMineBlockActionDataActiontype);
    assert_reserved_nine!(ItemStackRequestCerealPlaceActionDataActiontype);
    assert_reserved_nine!(ItemStackRequestCerealSwapActionDataActiontype);
    assert_reserved_nine!(ItemStackRequestCerealTakeActionDataActiontype);
}

#[test]
fn level_settings_reserved_fields_12_through_14_keep_their_shapes() {
    let settings = LevelSettings {
        reserved_12: LevelSettingsReserved12::Reserved2,
        reserved_13: true,
        reserved_14: "opaque".into(),
        ..Default::default()
    };
    let mut encoded = BytesMut::new();
    settings.encode(&mut encoded).expect("encode LevelSettings");
    let mut input = encoded.freeze();
    let decoded = LevelSettings::decode(&mut input, ()).expect("decode LevelSettings");
    assert_eq!(decoded.reserved_12, LevelSettingsReserved12::Reserved2);
    assert!(decoded.reserved_13);
    assert_eq!(decoded.reserved_14, "opaque");
    assert!(input.is_empty());
}

#[test]
fn default_crafting_data_keeps_reserved_vectors_empty() {
    let packet = CraftingDataPacket::default();
    assert!(packet.reserved_recipes_4.is_empty());
    assert!(packet.reserved_recipes_5.is_empty());
    assert!(packet.reserved_entries_10.is_empty());

    let mut encoded = BytesMut::new();
    packet.encode(&mut encoded).expect("encode CraftingData");
    let mut input = Bytes::from(encoded);
    let decoded = CraftingDataPacket::decode(&mut input, ()).expect("decode CraftingData");
    assert!(decoded.reserved_recipes_4.is_empty());
    assert!(decoded.reserved_recipes_5.is_empty());
    assert!(decoded.reserved_entries_10.is_empty());
    assert!(input.is_empty());
}
