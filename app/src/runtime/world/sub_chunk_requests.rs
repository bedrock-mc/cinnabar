use bevy::log::debug;
use chunk_pipeline::WorldStream;

pub(crate) fn flush_sub_chunk_requests(
    stream: &mut WorldStream,
    budget: usize,
    mut send: impl FnMut(
        world::ChunkKey,
        i32,
        usize,
        protocol::Packet,
    ) -> Result<(), crate::runtime::network::session::PacketSendError>,
) -> Result<usize, String> {
    let mut sent = 0;
    for _ in 0..budget {
        let Some(request) = stream.pop_next_request() else {
            break;
        };
        let chunk_pipeline::PendingSubChunkRequest {
            packet,
            dimension,
            chunk,
            base_sub_chunk_y,
            count,
        } = request;
        match send(chunk, base_sub_chunk_y, count, packet) {
            Ok(()) => {
                stream.record_sub_chunk_request_transport_pending(chunk, base_sub_chunk_y, count);
                debug!(
                    dimension,
                    chunk_x = chunk.x,
                    chunk_z = chunk.z,
                    base_sub_chunk_y,
                    count,
                    "requested streamed sub-chunk column"
                );
                sent += 1;
            }
            Err(error) => {
                let closed = error.is_closed();
                let retry = chunk_pipeline::PendingSubChunkRequest {
                    packet: error.into_packet(),
                    dimension,
                    chunk,
                    base_sub_chunk_y,
                    count,
                };
                if stream.retry_request_front(retry).is_err() {
                    return Err(
                        "failed to restore an unsent SubChunkRequest to the bounded FIFO"
                            .to_owned(),
                    );
                }
                if closed {
                    return Err(
                        "failed to send SubChunkRequest: network command channel is closed"
                            .to_owned(),
                    );
                }
                break;
            }
        }
    }
    Ok(sent)
}
