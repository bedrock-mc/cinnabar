use super::*;
use ui::UiVisual;

#[test]
fn empty_cards_keep_wrapped_copy_inside_their_padding() {
    use std::collections::HashMap;
    let mut bottom = 0.0;
    let (_, _, nodes) = super::super::review_tests::paint(HashMap::new(), |canvas| {
        bottom =
            empty::draw(canvas, 0, [0.0, 0.0, 300.0, 600.0], &|_| None).unwrap() - canvas.r(1.6);
    });
    let mut wrapped = false;
    for node in nodes {
        if let UiVisual::Text { layout, .. } = node.visual() {
            wrapped |= layout.line_count() > 1;
            assert!(
                node.bounds().max().y() <= bottom,
                "all wrapped copy must stay above the card's bottom padding"
            );
        }
    }
    assert!(wrapped, "the small card exercises wrapping");
}
