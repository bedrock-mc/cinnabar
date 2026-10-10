use render::{BlockSelectionFrame, BlockSelectionTarget, CrackShape};

#[test]
fn a_block_pick_draws_outline_or_highlight_and_clears_after_looking_away() {
    let mut frame = BlockSelectionFrame::default();
    let target = BlockSelectionTarget {
        block: [0; 3],
        bounds: [[0.0; 3], [1.0; 3]],
        shape: CrackShape::Cube,
    };
    frame.update(Some(&target), true);
    assert_eq!(frame.outline.len(), 12 * 2);
    assert!(frame.highlight.is_empty());
    assert!(
        frame
            .outline
            .iter()
            .all(|vertex| vertex.color == [0.0, 0.0, 0.0, 1.0])
    );
    let revision = frame.revision;
    let vertices = frame.outline.clone();
    frame.update(Some(&target), true);
    assert_eq!(frame.revision, revision);
    assert!(std::sync::Arc::ptr_eq(&vertices, &frame.outline));
    frame.update(Some(&target), false);
    assert!(frame.outline.is_empty());
    assert_eq!(frame.highlight.len(), 6 * 6);
    frame.update(None, false);
    assert!(frame.outline.is_empty() && frame.highlight.is_empty());
}

#[test]
fn selection_edges_stay_on_the_pick_bounds_without_camera_expansion() {
    for bounds in [
        [[0.0f32; 3], [1.0; 3]],
        [[-33.75, 40.0, 8.25], [-33.25, 41.0, 8.75]],
        [[0.0; 3], [1.0, 0.125, 1.0]],
    ] {
        let target = BlockSelectionTarget {
            block: bounds[0].map(|value| value.floor() as i32),
            bounds,
            shape: CrackShape::Cube,
        };
        let mut frame = BlockSelectionFrame::default();
        frame.update(Some(&target), true);
        let mut edges = std::collections::BTreeSet::new();
        for pair in frame.outline.as_chunks::<2>().0 {
            let mut corner_indices = [0u8; 2];
            for (index, vertex) in pair.iter().enumerate() {
                for (axis, &coordinate) in vertex.position.iter().enumerate() {
                    assert!(
                        coordinate == bounds[0][axis] || coordinate == bounds[1][axis],
                        "wire endpoints must remain on the visual box: {coordinate}"
                    );
                    if coordinate == bounds[1][axis] {
                        corner_indices[index] |= 1 << axis;
                    }
                }
            }
            assert_eq!(
                (corner_indices[0] ^ corner_indices[1]).count_ones(),
                1,
                "a line must join adjacent corners"
            );
            corner_indices.sort_unstable();
            assert!(edges.insert(corner_indices), "each box edge is drawn once");
        }
        assert_eq!(edges.len(), 12);
    }
}
