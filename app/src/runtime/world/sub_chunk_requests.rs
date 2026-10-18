use chunk_pipeline::WorldStream;
use client_presentation::local_player_camera_receipt::{CameraOwner, CameraPublicationAttempt};

/// Chooses the current render camera, falling back to the caller's current actor position.
pub(crate) fn sub_chunk_request_center(
    stream: &WorldStream,
    publication: Option<&CameraPublicationAttempt>,
    session_generation: u64,
    actor_position: [f32; 3],
) -> Option<[i32; 3]> {
    if stream.local_player_runtime_id() == 0 {
        return None;
    }
    let owner = CameraOwner::current(
        stream,
        simulation_clock::identity::SessionGeneration::new(session_generation),
    );
    let position = publication
        .and_then(CameraPublicationAttempt::published)
        .filter(|camera| camera.owner == owner)
        .map_or(actor_position, |camera| {
            camera.transform.translation.to_array()
        });
    if !position[0].is_finite() || !position[2].is_finite() {
        return None;
    }
    Some([
        (position[0].floor() as i32).div_euclid(16),
        0,
        (position[2].floor() as i32).div_euclid(16),
    ])
}

/// Flattens at most `intent_budget` column intents into one atomic transport handoff.
pub(crate) fn flush_sub_chunk_requests(
    stream: &mut WorldStream,
    intent_budget: usize,
    center: [i32; 3],
    mut send: impl FnMut(
        render_api::sub_chunk_requests::SubChunkRequestReceipt,
        protocol::Packet,
    ) -> Result<(), client_session::PacketSendError>,
) -> Result<usize, String> {
    let Some(batch) = stream.prepare_sub_chunk_request(center, intent_budget) else {
        return Ok(0);
    };
    let packet = protocol::request_sub_chunks(
        batch.receipt.dimension,
        center,
        batch.receipt.members.iter().map(|member| member.position),
    )
    .expect("bounded current request members have admitted offsets");
    match send(batch.receipt.clone(), packet) {
        Ok(()) => {
            stream.record_sub_chunk_request_transport_pending(&batch);
            Ok(1)
        }
        Err(error) => {
            let closed = error.is_closed();
            if stream.retry_sub_chunk_request_front(batch).is_err() {
                return Err(
                    "failed to restore an unsent SubChunkRequest to the bounded FIFO".to_owned(),
                );
            }
            if closed {
                return Err(
                    "failed to send SubChunkRequest: network command channel is closed".to_owned(),
                );
            }
            Ok(0)
        }
    }
}
