use bevy::math::Vec3;
use render::{BlockSelectionFrame, BlockSelectionTarget, CrackShape};

#[test]
fn a_block_pick_draws_outline_or_highlight_and_clears_after_looking_away() {
    let mut frame = BlockSelectionFrame::default();
    let target = BlockSelectionTarget {
        block: [0; 3],
        bounds: [[0.0; 3], [1.0; 3]],
        shape: CrackShape::Cube,
    };
    let eye = Vec3::new(2.0, 2.0, 4.0);
    let forward = (Vec3::splat(0.5) - eye).normalize();
    frame.update(Some(&target), eye, forward, true);
    assert_eq!(frame.outline.len(), 12 * 6);
    assert!(frame.highlight.is_empty());
    assert!(
        frame
            .outline
            .iter()
            .all(|vertex| vertex.color == [0.0, 0.0, 0.0, 1.0])
    );
    let revision = frame.revision;
    frame.update(Some(&target), eye, forward, true);
    assert_eq!(frame.revision, revision);
    frame.update(Some(&target), eye, forward, false);
    assert!(frame.outline.is_empty());
    assert_eq!(frame.highlight.len(), 6 * 6);
    frame.update(None, eye, forward, false);
    assert!(frame.outline.is_empty() && frame.highlight.is_empty());
}
