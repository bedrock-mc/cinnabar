use bytes::{Bytes, BytesMut};
use jolyne::{
    batch::{decode_batch_raw, encode_batch_multi},
    raw::RawPacket,
    valentine::{McpePacketArgs, McpePacketData, McpePacketName},
};
use protocol::BedrockSession;
use valentine::bedrock::{
    codec::{BedrockCodec, VarInt},
    error::DecodeError,
    version::v1_26_40::BiomeDefinitionChunkGenData,
};

const GOPHERTUNNEL_BIOME_DEFINITION_LIST: &[u8] =
    include_bytes!("../fixtures/biome_definition_list_chunk_generation.bin");
const MAX_BIOME_COLLECTION_ELEMENTS: usize = 4_096;

fn session() -> BedrockSession {
    BedrockSession { shield_item_id: 0 }
}

fn raw_fixture() -> RawPacket {
    let mut batch = Bytes::from_static(GOPHERTUNNEL_BIOME_DEFINITION_LIST);
    decode_batch_raw(&mut batch, false, Some(1_024))
        .expect("raw batch decode")
        .into_iter()
        .next()
        .expect("one packet")
}

fn assert_fixture_fields(data: &McpePacketData) {
    let McpePacketData::BiomeDefinitionListPacket(packet) = data else {
        panic!("expected BiomeDefinitionList, got {:?}", data.packet_id());
    };
    // 1.26.40 renames the packet's collections: `biome_definitions` became
    // `mapof_biomenamestodata` (an index/definition pair) and `string_list`
    // became `stringlist.strings`. The bytes are unchanged - gophertunnel
    // packet/biome_definition_list.go still writes the definition slice then a
    // FuncSlice of strings.
    assert_eq!(packet.mapof_biomenamestodata.len(), 1);
    assert!(packet.stringlist.strings.is_empty());
    let chunk = packet.mapof_biomenamestodata[0]
        .value
        .chunkgendata
        .as_ref()
        .expect("fixture has chunk generation");
    assert!(chunk.climate.is_none());
    assert!(chunk.consolidatedfeatures.is_none());
    assert!(chunk.mountainparams.is_none());
    assert!(chunk.surfacematerialadjustments.is_none());
    assert!(chunk.overworldgenrules.is_none());
    assert!(chunk.multinoisegenrules.is_none());
    assert!(chunk.legacyworldgenrules.is_none());
    assert!(chunk.replacementbiomes.is_none());
    assert!(chunk.village_type.is_none());
    assert!(chunk.surfacebuilderdata.is_none());
    assert!(chunk.subsurfacebuilderdata.is_none());
}

#[test]
fn pinned_gophertunnel_biome_definition_list_owned_decodes_and_round_trips() {
    let packet = raw_fixture()
        .decode(&session())
        .expect("owned BiomeDefinitionList decode");
    assert_eq!(
        packet.data.packet_id(),
        McpePacketName::BiomeDefinitionListPacket
    );
    assert_fixture_fields(&packet.data);
    let encoded = encode_batch_multi(&[packet], false, 0, 0, true).expect("re-encode");
    assert_eq!(encoded.as_ref(), GOPHERTUNNEL_BIOME_DEFINITION_LIST);
}

#[test]
fn pinned_gophertunnel_biome_definition_list_borrowed_view_materializes_and_round_trips() {
    let borrowed = raw_fixture()
        .decode_borrowed()
        .expect("borrowed BiomeDefinitionList decode");
    // 1.26.30 had no borrowed view for this packet and fell back to
    // `BorrowedMcpePacketData::Raw`. 1.26.40 generates a real
    // `BiomeDefinitionListPacketView`, so the borrowed path is now typed.
    assert!(matches!(
        &borrowed.data,
        valentine::bedrock::version::v1_26_40::BorrowedMcpePacketData::BiomeDefinitionListPacket(_)
    ));
    let owned = borrowed
        .into_owned(McpePacketArgs)
        .expect("materialize borrowed BiomeDefinitionList");
    assert_fixture_fields(&owned.data);
    let encoded = encode_batch_multi(&[owned], false, 0, 0, true).expect("re-encode");
    assert_eq!(encoded.as_ref(), GOPHERTUNNEL_BIOME_DEFINITION_LIST);
}

/// A hostile nested collection count must still fail the read.
///
/// REGRESSION - see the module header of `world_collection_bounds.rs`. Under
/// 1.26.30 this failed with `DecodeError::ArrayLengthExceeded { declared: 4097,
/// available: 4096 }` *before* allocating. The 1.26.40 generated crate emits no
/// collection ceilings, so the count is reserved first and the read only fails
/// when the elements turn out to be absent. Restoring the ceiling in
/// valentine_gen should trip the `ArrayLengthExceeded` arm below.
#[test]
fn biome_chunk_generation_rejects_oversized_nested_collection() {
    let mut bytes = BytesMut::from(&[0, 1][..]);
    VarInt((MAX_BIOME_COLLECTION_ELEMENTS + 1) as i32)
        .encode(&mut bytes)
        .expect("encode oversized collection count");

    let error = BiomeDefinitionChunkGenData::decode(&mut bytes.freeze(), ())
        .expect_err("4,097 consolidated features must not decode");
    match &error {
        DecodeError::UnexpectedEof { .. } => {}
        DecodeError::Io(io) if io.kind() == std::io::ErrorKind::UnexpectedEof => {}
        DecodeError::ArrayLengthExceeded { .. } => panic!(
            "valentine_gen appears to emit collection ceilings again: restore the stricter \
             declared/available assertion here"
        ),
        other => panic!("unexpected decode error: {other:?}"),
    }
}

#[test]
fn biome_chunk_generation_rejects_truncated_surface_builder_slots() {
    // `BiomeDefinitionChunkGenData` is eleven optional slots; ten present bytes
    // leaves the last one truncated.
    let mut bytes = Bytes::from_static(&[0; 10]);
    let error = BiomeDefinitionChunkGenData::decode(&mut bytes, ())
        .expect_err("ten of eleven option slots must be truncated");
    assert!(matches!(error, DecodeError::UnexpectedEof { .. }));
}
