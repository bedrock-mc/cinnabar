use super::*;

#[test]
fn groups_admitted_columns_in_one_request() {
    let mut stream = chunk_pipeline::WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    });
    for (sequence, x) in [(1, 0), (2, 1)] {
        stream
            .submit(
                sequence,
                WorldEvent::LevelChunk(LevelChunkEvent {
                    dimension: 0,
                    x,
                    z: 0,
                    mode: LevelChunkMode::LimitedRequests { highest: 1 },
                    payload: overworld_biome_payload(),
                }),
            )
            .unwrap();
    }
    complete_world_stream_decodes(&mut stream);
    let mut packets = Vec::new();
    let sent = flush_sub_chunk_requests(&mut stream, 8, |_, _, _, packet| {
        packets.push(packet);
        Ok(())
    })
    .unwrap();

    assert_eq!(sent, 1, "admitted columns share one request envelope");
    assert_eq!(packets.len(), 1);
    assert_eq!(stream.pending_request_count(), 0);
    assert_eq!(stream.stats().awaiting_sub_chunk_responses, 0);
}
