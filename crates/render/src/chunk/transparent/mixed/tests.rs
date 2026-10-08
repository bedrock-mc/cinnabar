use super::plan::{MixedFace, MixedStream};
use super::*;

fn face(stream: MixedStream, index: u32, z: f32) -> MixedFace {
    MixedFace {
        stream,
        index,
        centroid: Vec3::new(0.5, 0.5, z),
        stable: [index, 0],
    }
}

#[test]
fn models_and_water_share_native_order_and_compress_contiguous_runs() {
    let faces = vec![
        face(MixedStream::Model, 0, 4.0),
        face(MixedStream::Model, 1, 3.0),
        face(MixedStream::Water, 10, 2.0),
        face(MixedStream::Model, 2, 1.0),
        face(MixedStream::Water, 11, 0.0),
    ];
    let segments = merge_faces(SubChunkKey::new(0, 0, 0, 0), Vec3::ZERO, faces, 4).unwrap();
    assert_eq!(
        segments,
        [
            MixedTerrainSegment {
                stream: MixedStream::Model,
                range: 0..2
            },
            MixedTerrainSegment {
                stream: MixedStream::Water,
                range: 10..11
            },
            MixedTerrainSegment {
                stream: MixedStream::Model,
                range: 2..3
            },
            MixedTerrainSegment {
                stream: MixedStream::Water,
                range: 11..12
            },
        ]
    );
}

#[test]
fn old_uploaded_subsequence_order_uses_actual_indices_not_current_rank() {
    let faces = vec![
        face(MixedStream::Model, 0, 1.0),
        face(MixedStream::Model, 1, 3.0),
        face(MixedStream::Water, 5, 2.0),
    ];
    let segments = merge_faces(SubChunkKey::new(0, 0, 0, 0), Vec3::ZERO, faces, 3).unwrap();
    assert_eq!(
        segments
            .iter()
            .map(|segment| (segment.stream, segment.range.clone()))
            .collect::<Vec<_>>(),
        [
            (MixedStream::Model, 1..2),
            (MixedStream::Water, 5..6),
            (MixedStream::Model, 0..1)
        ]
    );
}

#[test]
fn segment_guard_rejects_pathological_fragmentation_before_submission() {
    let faces = vec![
        face(MixedStream::Model, 0, 3.0),
        face(MixedStream::Water, 0, 2.0),
        face(MixedStream::Model, 1, 1.0),
    ];
    assert!(merge_faces(SubChunkKey::new(0, 0, 0, 0), Vec3::ZERO, faces, 2).is_none());
}

#[test]
fn ordinary_ice_water_overlap_never_hits_the_former_independent_segment_cap() {
    let side = chunk_origin(SubChunkKey::new(0, 1, 0, 0))[0] as u32;
    let count = 2 * side.pow(3);
    let camera = Vec3::new(0.1234567, 0.456789, 0.789123);
    // Half-block anchors survive centroid packing. The off-centre camera gives
    // them distinct distances; sub-packing-step spacing would collapse into ties.
    let mut centroids = (0..count)
        .map(|index| {
            Vec3::new(
                (index % (2 * side)) as f32 / 2.0,
                (index / (2 * side) % side) as f32 + 0.5,
                (index / (2 * side * side)) as f32 + 0.5,
            )
        })
        .collect::<Vec<_>>();
    centroids.sort_by(|left, right| {
        (*right - camera)
            .length_squared()
            .total_cmp(&(*left - camera).length_squared())
    });
    assert!(
        centroids.windows(2).all(|pair| {
            (pair[0] - camera).length_squared() > (pair[1] - camera).length_squared()
        })
    );
    let faces = centroids
        .into_iter()
        .enumerate()
        .map(|(index, centroid)| {
            let index = index as u32;
            let stream = if index.is_multiple_of(2) {
                MixedStream::Model
            } else {
                MixedStream::Water
            };
            MixedFace {
                stream,
                index: index / 2,
                centroid,
                stable: [index / 2, 0],
            }
        })
        .collect::<Vec<_>>();
    let expected = faces
        .iter()
        .map(|face| MixedTerrainSegment {
            stream: face.stream,
            range: face.index..face.index + 1,
        })
        .collect::<Vec<_>>();
    assert!(expected.len() > 4096, "exercise the former segment cap");
    let segments = merge_faces(
        SubChunkKey::new(0, 0, 0, 0),
        camera,
        faces,
        MAX_MIXED_TERRAIN_SEGMENTS_PER_FRAME,
    )
    .expect("all admitted references retain their interleaved order");
    assert_eq!(segments, expected);
}

#[test]
fn faces_in_the_same_packed_centroid_use_stable_stream_order() {
    // Water is farther away before packing, but both anchors pack identically.
    let faces = vec![
        face(MixedStream::Water, 4, 1.002),
        face(MixedStream::Model, 7, 1.001),
    ];
    let segments = merge_faces(SubChunkKey::new(0, 0, 0, 0), Vec3::ZERO, faces, 2).unwrap();
    assert_eq!(
        segments,
        [
            MixedTerrainSegment {
                stream: MixedStream::Model,
                range: 7..8,
            },
            MixedTerrainSegment {
                stream: MixedStream::Water,
                range: 4..5,
            },
        ]
    );
}
