use super::super::geometry::is_horizontal;
use super::*;

/// The `[u0, u1, v0, v1]` extent a quad reads, whichever corner reads which end.
fn extent(quad: &ModelQuad) -> UvRect {
    let u = quad.uvs.map(|uv| uv[0]);
    let v = quad.uvs.map(|uv| uv[1]);
    [
        *u.iter().min().unwrap(),
        *u.iter().max().unwrap(),
        *v.iter().min().unwrap(),
        *v.iter().max().unwrap(),
    ]
}

/// The first quad covering `face` inside the piece bounds `min..=max` (model units).
fn face_at(quads: &[ModelQuad], [min, max]: [[i16; 3]; 2], face: BlockFace) -> ModelQuad {
    *quads
        .iter()
        .find(|quad| {
            quad.flags & MODEL_QUAD_FLAG_FACE_MASK == face.model_quad_face_id()
                && quad
                    .positions
                    .iter()
                    .all(|p| (0..3).all(|axis| (min[axis]..=max[axis]).contains(&p[axis])))
        })
        .unwrap_or_else(|| panic!("{face:?} face inside {min:?}..={max:?}"))
}

#[test]
fn bamboo_gate_faces_read_only_native_atlas_regions() {
    let native = [
        BAMBOO_POST_TOP,
        BAMBOO_POST_SIDE,
        BAMBOO_BAR_TOP,
        BAMBOO_BAR_OUTER,
        BAMBOO_BAR_INNER,
        BAMBOO_RAIL_BOTTOM,
        BAMBOO_RAIL_NORTH,
        BAMBOO_RAIL_SOUTH,
    ];
    for orientation in 0..4 {
        for open in [false, true] {
            for in_wall in [false, true] {
                let parts = gate_quads([7; 6], orientation, open, in_wall, true);
                assert_eq!(parts.each_ref().map(Vec::len), [24, 24], "all six faces");
                for quad in parts.iter().flatten() {
                    assert!(
                        native.contains(&extent(quad)),
                        "orientation {orientation}, open {open}: {quad:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn bamboo_gate_posts_and_rails_read_their_own_regions() {
    let [head, tail] = gate_quads([7; 6], 0, false, false, true);
    for quad in &head[..12] {
        let expected = if is_horizontal(quad) {
            BAMBOO_POST_TOP
        } else {
            BAMBOO_POST_SIDE
        };
        assert_eq!(extent(quad), expected, "{quad:?}");
    }
    let rail = [[32, 96, 112], [96, 144, 144]];
    assert_eq!(
        extent(&face_at(&tail, rail, BlockFace::Up)),
        BAMBOO_RAIL_BOTTOM
    );
    assert_eq!(
        extent(&face_at(&tail, rail, BlockFace::North)),
        BAMBOO_RAIL_NORTH
    );
    assert_eq!(
        extent(&face_at(&tail, rail, BlockFace::South)),
        BAMBOO_RAIL_SOUTH
    );
}

#[test]
fn bamboo_gate_rail_tops_follow_the_rail_along_either_axis() {
    // A rail top's u range spans the rail's length, along X or along Z.
    for (orientation, rail, length_axis) in [
        (0, [[32, 96, 112], [96, 144, 144]], 0),
        (1, [[112, 96, 32], [144, 144, 96]], 2),
    ] {
        let [_, tail] = gate_quads([7; 6], orientation, false, false, true);
        let top = face_at(&tail, rail, BlockFace::Up);
        let far = top.positions.iter().map(|p| p[length_axis]).max().unwrap();
        // Corners at the same end of the rail read the same u; the two ends read the
        // region's two u bounds.
        let u_at = |far_end: bool| {
            let ends: Vec<_> = top
                .positions
                .iter()
                .zip(top.uvs)
                .filter(|(position, _)| (position[length_axis] == far) == far_end)
                .map(|(_, uv)| uv[0])
                .collect();
            assert_eq!(ends[0], ends[1], "orientation {orientation}: {top:?}");
            ends[0]
        };
        let [u0, u1, ..] = BAMBOO_RAIL_BOTTOM;
        let mut ends = [u_at(false), u_at(true)];
        ends.sort_unstable();
        assert_eq!(ends, [u0, u1], "orientation {orientation}: {top:?}");
    }
}

#[test]
fn bamboo_gate_bars_keep_world_side_regions_when_turned_around() {
    for orientation in [0, 2] {
        let [head, _] = gate_quads([7; 6], orientation, false, false, true);
        let bar = [[96, 96, 112], [128, 240, 144]];
        assert_eq!(
            extent(&face_at(&head, bar, BlockFace::West)),
            BAMBOO_BAR_INNER,
            "orientation {orientation}"
        );
        assert_eq!(
            extent(&face_at(&head, bar, BlockFace::North)),
            BAMBOO_BAR_OUTER,
            "orientation {orientation}"
        );
    }
}
