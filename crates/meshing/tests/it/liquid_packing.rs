use meshing::liquid::{LIQUID_DEPTH_WRITE_BIT, LIQUID_TOP_INSET_BIT, LIQUID_TWO_SIDED_BIT};
use meshing::{Face, PackedLiquidQuad};

#[test]
fn liquid_material_id_never_admits_geometry_or_route_flags() {
    for flags in [
        LIQUID_TWO_SIDED_BIT,
        LIQUID_TOP_INSET_BIT,
        LIQUID_DEPTH_WRITE_BIT,
        LIQUID_TWO_SIDED_BIT | LIQUID_TOP_INSET_BIT,
        LIQUID_TWO_SIDED_BIT | LIQUID_DEPTH_WRITE_BIT,
        LIQUID_TOP_INSET_BIT | LIQUID_DEPTH_WRITE_BIT,
        LIQUID_TWO_SIDED_BIT | LIQUID_TOP_INSET_BIT | LIQUID_DEPTH_WRITE_BIT,
    ] {
        assert!(
            PackedLiquidQuad::try_pack(
                [3, 4, 5],
                Face::PositiveY,
                [255; 4],
                7 | flags,
                11,
                [-17, 23],
                true,
            )
            .is_none()
        );
    }
}

#[test]
fn liquid_raw_stream_round_trip_separates_all_face_flags_from_material() {
    let packed =
        PackedLiquidQuad::try_pack([3, 4, 5], Face::PositiveY, [255; 4], 7, 11, [-17, 23], true)
            .expect("bounded liquid record");
    assert!(!packed.has_top_height_inset());
    assert!(!packed.is_two_sided());
    for flags in [
        0,
        LIQUID_TWO_SIDED_BIT,
        LIQUID_TOP_INSET_BIT,
        LIQUID_DEPTH_WRITE_BIT,
        LIQUID_TWO_SIDED_BIT | LIQUID_TOP_INSET_BIT,
        LIQUID_TWO_SIDED_BIT | LIQUID_DEPTH_WRITE_BIT,
        LIQUID_TOP_INSET_BIT | LIQUID_DEPTH_WRITE_BIT,
        LIQUID_TWO_SIDED_BIT | LIQUID_TOP_INSET_BIT | LIQUID_DEPTH_WRITE_BIT,
    ] {
        let mut words = packed.words();
        words[2] |= flags;
        let decoded = PackedLiquidQuad::try_from_words(words).expect("valid flagged record");
        assert_eq!(decoded.material_id(), packed.material_id());
        assert_eq!(
            decoded.has_top_height_inset(),
            flags & LIQUID_TOP_INSET_BIT != 0
        );
        assert_eq!(
            decoded.is_depth_writing(),
            flags & LIQUID_DEPTH_WRITE_BIT != 0
        );
        assert_eq!(decoded.is_two_sided(), flags & LIQUID_TWO_SIDED_BIT != 0);
        assert_eq!(decoded.origin(), packed.origin());
        assert_eq!(decoded.face(), packed.face());
        assert_eq!(decoded.heights(), packed.heights());
        assert_eq!(decoded.flow_gradient(), packed.flow_gradient());
        assert_eq!(decoded.is_falling(), packed.is_falling());
        assert_eq!(decoded.lighting_index(), packed.lighting_index());
        assert_eq!(decoded.words(), words);
    }
}
