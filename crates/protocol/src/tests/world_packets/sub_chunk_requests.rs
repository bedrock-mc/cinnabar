use super::*;
use render_api::sub_chunk_requests::{MAX_SUB_CHUNK_REQUESTS, sub_chunk_request_offset};

#[test]
fn grouped_request_preserves_sparse_absolute_order_around_camera_center() {
    let center = [-2, 0, 3];
    let members = [[8, -4, -2], [8, -2, -2], [0, -2, 1]];
    let packet = crate::request_sub_chunks(1, center, members.into_iter()).unwrap();
    let packets::Packet::SubChunkRequest(request) = *packet.data else {
        panic!("expected SubChunkRequest");
    };
    assert_eq!(request.dimension_type.0, 1);
    assert_eq!(request.center_pos, sub_chunk_pos(-2, 0, 3));
    let absolute: Vec<_> = request
        .sub_chunk_position_offset_list
        .iter()
        .map(|offset| {
            [
                center[0] + i32::from(offset.subchunk_offset_x.0),
                center[1] + i32::from(offset.subchunk_offset_y.0),
                center[2] + i32::from(offset.subchunk_offset_z.0),
            ]
        })
        .collect();
    assert_eq!(absolute, members);
}

#[test]
fn every_axis_accepts_signed_127_and_rejects_signed_128_without_overflow() {
    for axis in 0..3 {
        for distance in [-127, 127] {
            let mut position = [0; 3];
            position[axis] = distance;
            assert!(crate::request_sub_chunks(0, [0; 3], [position].into_iter()).is_ok());
        }
        for distance in [-128, 128, i32::MIN, i32::MAX] {
            let mut position = [0; 3];
            position[axis] = distance;
            assert_eq!(
                crate::request_sub_chunks(0, [0; 3], [position].into_iter()),
                Err(WorldPacketError::SubChunkRequestOutOfRange {
                    center: [0; 3],
                    position
                })
            );
        }
    }
    assert!(sub_chunk_request_offset([i32::MIN; 3], [i32::MAX; 3]).is_none());
    assert!(sub_chunk_request_offset([i32::MAX; 3], [i32::MIN; 3]).is_none());
}

#[test]
fn grouped_request_enforces_the_packet_bound_without_splitting() {
    let limit = MAX_SUB_CHUNK_REQUESTS;
    let packet = crate::request_sub_chunks(0, [0; 3], vec![[0; 3]; limit].into_iter()).unwrap();
    let packets::Packet::SubChunkRequest(request) = *packet.data else {
        panic!("expected SubChunkRequest");
    };
    assert_eq!(request.sub_chunk_position_offset_list.len(), limit);
    assert_eq!(
        crate::request_sub_chunks(0, [0; 3], vec![[0; 3]; limit + 1].into_iter()),
        Err(WorldPacketError::TooManySubChunkRequests {
            count: limit + 1,
            max: limit
        })
    );
}
