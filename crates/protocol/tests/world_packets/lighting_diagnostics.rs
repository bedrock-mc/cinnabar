use protocol::{SubChunkResult, WorldEvent, into_world_event};
use valentine::bedrock::version::v1_26_51::{
    DimensionDataPacket, DimensionDataPacketDefinitionsItem,
    DimensionDefinitionGroupDimensionDefinition, DimensionType,
    EnumsSubChunkPacketPayloadHeightMapDataType as HeightmapType,
    EnumsSubChunkPacketPayloadSubChunkRequestResult as ResultType, SubChunkPacket,
    SubChunkPacketPayloadHeightmapData, SubChunkPacketPayloadHeightmapDataRenderHeightMapType,
    SubChunkPacketPayloadSubChunkPacketData,
};

#[test]
fn retains_sub_chunk_payload_presence_and_server_heightmap_facts() {
    let packet = SubChunkPacket {
        sub_chunk_data: vec![
            SubChunkPacketPayloadSubChunkPacketData {
                sub_chunk_request_result: ResultType::Successallair,
                height_map_data: SubChunkPacketPayloadHeightmapData {
                    height_map_type: HeightmapType::Hasdata,
                    subchunk_height_map: Some(std::array::from_fn(|row| {
                        if row == 0 { vec![-12, 7, 22] } else { vec![] }
                    })),
                    render_height_map_type:
                        SubChunkPacketPayloadHeightmapDataRenderHeightMapType::Allcopied,
                    ..Default::default()
                },
                ..Default::default()
            },
            SubChunkPacketPayloadSubChunkPacketData {
                sub_chunk_request_result: ResultType::Success,
                serialized_sub_chunk: Some(vec![]),
                ..Default::default()
            },
            SubChunkPacketPayloadSubChunkPacketData {
                sub_chunk_request_result: ResultType::Indexoutofbounds,
                height_map_data: SubChunkPacketPayloadHeightmapData {
                    height_map_type: HeightmapType::Unknown(231),
                    ..Default::default()
                },
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    let WorldEvent::SubChunks(batch) = into_world_event(packet.into(), 0).unwrap().unwrap() else {
        panic!("expected sub-chunks");
    };
    assert_eq!(batch.entries[0].result, SubChunkResult::AllAir);
    let first = batch.entries[0].diagnostics.unwrap();
    assert!(!first.payload_present);
    assert_eq!(first.heightmap.kind, 1);
    assert!(first.heightmap.payload_present);
    assert_eq!(first.heightmap.sample_count, 3);
    assert_eq!(first.heightmap.min, Some(-12));
    assert_eq!(first.heightmap.max, Some(22));
    assert_eq!(first.render_heightmap.kind, 4);
    assert!(!first.render_heightmap.payload_present);
    assert_eq!(first.render_heightmap.sample_count, 0);
    assert_eq!(first.render_heightmap.min, None);
    assert_eq!(first.render_heightmap.max, None);
    let second = batch.entries[1].diagnostics.unwrap();
    assert!(second.payload_present);
    assert_eq!(second.heightmap.kind, 0);
    assert!(!second.heightmap.payload_present);
    assert_eq!(batch.entries[2].diagnostics.unwrap().heightmap.kind, 231);
}

#[test]
fn bounds_advertised_dimension_heights_without_rejecting_custom_ranges() {
    let packet = DimensionDataPacket {
        definitions: (0..1000)
            .map(|dimension| DimensionDataPacketDefinitionsItem {
                key: format!("custom:{dimension}"),
                value: DimensionDefinitionGroupDimensionDefinition {
                    minimum_y: -128,
                    height_range: 1024,
                    dimension_type: DimensionType { value: dimension },
                    ..Default::default()
                },
            })
            .collect(),
    };
    let WorldEvent::DimensionHeights(heights) =
        into_world_event(packet.into(), 0).unwrap().unwrap()
    else {
        panic!("expected dimension height facts");
    };
    assert_eq!(heights.len(), 64);
    assert_eq!(heights[0].dimension, 0);
    assert_eq!(heights[0].minimum_y, -128);
    assert_eq!(heights[0].height_range, 1024);
    assert_eq!(heights.last().unwrap().dimension, 63);
}
