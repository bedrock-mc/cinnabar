use super::*;

/// Returns the bounding box of a nonempty set of wall faces.
fn bounds(quads: &[ModelQuad]) -> ([i16; 3], [i16; 3]) {
    let mut min = [i16::MAX; 3];
    let mut max = [i16::MIN; 3];
    for position in quads.iter().flat_map(|quad| quad.positions) {
        for axis in 0..3 {
            min[axis] = min[axis].min(position[axis]);
            max[axis] = max[axis].max(position[axis]);
        }
    }
    (min, max)
}

#[test]
fn disconnected_and_corner_walls_keep_the_center_post() {
    let isolated = wall_quads([1; 6], 0);
    assert_eq!(bounds(&isolated), ([64, 0, 64], [192, 256, 192]));
    let corner = wall_quads([1; 6], 1 | (2 << 2));
    assert_eq!(corner.len(), 18);
    assert_eq!(bounds(&corner[..6]), ([64, 0, 64], [192, 256, 192]));
    assert_eq!(bounds(&corner[6..12]), ([80, 0, 0], [176, 208, 64]));
    assert_eq!(bounds(&corner[12..]), ([192, 0, 80], [256, 256, 176]));
}

#[test]
fn postless_straight_walls_use_one_box_and_the_lower_connection_height() {
    for (first, second, height) in [(1, 1, 208), (1, 2, 208), (2, 1, 208), (2, 2, 256)] {
        for (connections, expected) in [
            (first | (second << 4), ([80, 0, 0], [176, height, 256])),
            (
                (first << 2) | (second << 6),
                ([0, 0, 80], [256, height, 176]),
            ),
        ] {
            let quads = wall_quads([1; 6], connections);
            assert_eq!(quads.len(), 6, "connections={connections:#x}");
            assert_eq!(bounds(&quads), expected);
        }
    }
}

#[test]
fn every_registered_wall_state_has_geometry() {
    let records = assets::read_registry_for_protocol(
        assets::pinned_block_registry_bytes(),
        assets::active_content_registry_protocol(),
    )
    .expect("active block registry");
    let mut seen = 0;
    for record in records
        .iter()
        .filter(|record| record.model_family == ModelFamily::Wall)
    {
        let connections = record
            .model_state
            .get(ModelStateField::Connections)
            .expect("wall state");
        assert!(
            wall_state_is_valid(connections),
            "{}",
            record.canonical_state
        );
        assert!(
            !wall_quads([1; 6], connections).is_empty(),
            "{} {}",
            record.name,
            record.canonical_state
        );
        seen += 1;
    }
    assert!(seen > 0, "registry must exercise walls");
}

#[test]
fn wall_face_uvs_follow_the_outside_view() {
    let quads = wall_quads([1; 6], 1 << 8);
    for (face, quad) in assets::BlockFace::ALL.into_iter().zip(quads) {
        for ([x, y, z], uv) in quad.positions.into_iter().zip(quad.uvs) {
            let expected = match face {
                assets::BlockFace::West => [z * 16, 4096 - y * 16],
                assets::BlockFace::East => [4096 - z * 16, 4096 - y * 16],
                assets::BlockFace::Down => [x * 16, 4096 - z * 16],
                assets::BlockFace::Up => [x * 16, z * 16],
                assets::BlockFace::North => [4096 - x * 16, 4096 - y * 16],
                assets::BlockFace::South => [x * 16, 4096 - y * 16],
            };
            assert_eq!(uv, expected.map(|value| value as u16), "{face:?}");
        }
    }
}

#[test]
fn arm_undersides_keep_their_cropped_texture_strip() {
    let quads = wall_quads([1; 6], 1);
    let bottom = &quads[6 + assets::BlockFace::Down as usize];
    for ([x, _, z], uv) in bottom.positions.into_iter().zip(bottom.uvs) {
        assert_eq!(uv, [(x * 16) as u16, ((64 - z) * 16) as u16]);
    }
}
