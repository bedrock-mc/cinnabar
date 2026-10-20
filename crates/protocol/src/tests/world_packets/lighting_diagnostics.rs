use bedrock_protocol_1_26_51::enums::{
    GeneratorType, HeightMapDataType as HeightmapType, SubChunkRequestResult as ResultType,
};
use bedrock_protocol_1_26_51::packets::{DimensionData, SubChunk};
use bedrock_protocol_1_26_51::types::{
    DimensionDefinition, DimensionType, HeightmapData, SubChunkData,
};
use bedrock_protocol_1_26_51::wire;
use protocol::{SubChunkResult, WorldEvent, into_world_event};

#[test]
fn retains_sub_chunk_payload_presence_and_server_heightmap_facts() {
    let packet = SubChunk {
        sub_chunk_data: vec![
            SubChunkData {
                sub_chunk_request_result: ResultType::Successallair,
                height_map_data: HeightmapData {
                    height_map_type: HeightmapType::Hasdata,
                    subchunk_height_map: Some(std::array::from_fn(|row| {
                        if row == 0 {
                            vec![wire::I8(-12), wire::I8(7), wire::I8(22)]
                        } else {
                            vec![]
                        }
                    })),
                    render_height_map_type: HeightmapType::Allcopied,
                    ..Default::default()
                },
                ..Default::default()
            },
            SubChunkData {
                sub_chunk_request_result: ResultType::Success,
                serialized_sub_chunk: Some(bytes::Bytes::new()),
                ..Default::default()
            },
            SubChunkData {
                sub_chunk_request_result: ResultType::Indexoutofbounds,
                height_map_data: HeightmapData {
                    height_map_type: HeightmapType::Unknown(231),
                    ..Default::default()
                },
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    let WorldEvent::SubChunks(batch) = into_world_event(protocol::Packet::new(packet), 0)
        .unwrap()
        .unwrap()
    else {
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
    let packet = DimensionData {
        definitions: (0..1000)
            .map(|dimension| {
                (
                    format!("custom:{dimension}").into(),
                    DimensionDefinition {
                        minimum_y: wire::ZigZag32(-128),
                        height_range: wire::ZigZag32(1024),
                        dimension_type: DimensionType(dimension),
                        ..Default::default()
                    },
                )
            })
            .collect(),
    };
    let WorldEvent::DimensionHeights(heights) = into_world_event(protocol::Packet::new(packet), 0)
        .unwrap()
        .unwrap()
    else {
        panic!("expected dimension height facts");
    };
    assert_eq!(heights.len(), 64);
    assert_eq!(heights[0].dimension, 0);
    assert_eq!(heights[0].name.as_ref(), "custom:0");
    assert_eq!(heights[0].minimum_y, -128);
    assert_eq!(heights[0].maximum_y, 1024);
    assert_eq!(heights.last().unwrap().dimension, 63);
}

#[test]
fn retains_named_overworld_definition_with_a_distinct_numeric_type() {
    let packet = DimensionData {
        definitions: vec![(
            "minecraft:overworld".into(),
            DimensionDefinition {
                minimum_y: wire::ZigZag32(-64),
                height_range: wire::ZigZag32(320),
                dimension_type: DimensionType(3),
                generator_type: GeneratorType::Overworld,
                ..Default::default()
            },
        )],
    };
    let WorldEvent::DimensionHeights(heights) = into_world_event(protocol::Packet::new(packet), 0)
        .unwrap()
        .unwrap()
    else {
        panic!("expected dimension definitions");
    };
    assert_eq!(heights[0].name.as_ref(), "minecraft:overworld");
    assert_eq!(heights[0].dimension, 3);
    assert_eq!(heights[0].minimum_y, -64);
    assert_eq!(heights[0].maximum_y, 320);
    assert_eq!(heights[0].generator, 1);
}
