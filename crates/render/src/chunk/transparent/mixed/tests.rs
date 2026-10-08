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

fn ice_and_water(origin: Vec3, water_offset: u32) -> Vec<MixedFace> {
    let (mut water, mut model) = (water_offset, 0);
    (0..96_u32)
        .map(|index| {
            let local = Vec3::new(
                (index % 8) as f32 + 0.5,
                (index / 8) as f32 * 0.5 + 0.25,
                ((index * 5) % 16) as f32,
            );
            let (stream, index) = if index % 3 == 0 {
                water += 1;
                (MixedStream::Water, water - 1)
            } else {
                model += 1;
                (MixedStream::Model, model - 1)
            };
            MixedFace {
                stream,
                index,
                centroid: origin + local,
                stable: [index, 0],
            }
        })
        .collect()
}

/// A far ice/water plan is reused across camera motion, so it must not depend on it.
#[test]
fn far_mixed_order_is_identical_for_every_camera_in_its_class() {
    let key = SubChunkKey::new(0, 2, 0, -3);
    let origin = Vec3::from_array(chunk_origin(key).map(|value| value as f32));
    let cameras = [
        Vec3::new(1.0, 2.0, 3.0),
        Vec3::new(12.5, 9.0, 0.5),
        Vec3::new(0.1, 15.9, 15.9),
    ];
    let class = TransparentFaceMetric::new(cameras[0]).class(key);
    let expected = merge_faces(key, cameras[0], ice_and_water(origin, 0), usize::MAX).unwrap();
    assert!(expected.len() > 2, "the scene interleaves both streams");
    for camera in cameras {
        assert_eq!(TransparentFaceMetric::new(camera).class(key), class);
        assert_eq!(
            merge_faces(key, camera, ice_and_water(origin, 0), usize::MAX).unwrap(),
            expected
        );
    }
}

/// Group-relative water ranges draw exactly what snapshot-absolute ranges drew.
#[test]
fn group_relative_water_segments_match_absolute_ones() {
    let key = SubChunkKey::new(0, 0, 0, 0);
    let origin = Vec3::ZERO;
    let camera = Vec3::new(4.3, 7.1, 9.8);
    let relative = merge_faces(key, camera, ice_and_water(origin, 0), usize::MAX).unwrap();
    let absolute = merge_faces(key, camera, ice_and_water(origin, 100), usize::MAX).unwrap();
    assert_eq!(relative.len(), absolute.len());
    for (relative, absolute) in relative.iter().zip(&absolute) {
        let shift = if relative.stream == MixedStream::Water {
            100
        } else {
            0
        };
        assert_eq!(relative.stream, absolute.stream);
        assert_eq!(
            relative.range.start + shift..relative.range.end + shift,
            absolute.range
        );
    }
}
