use super::*;
use crate::ui_runtime::presentation::forms::oreui::{
    review_tests::paint,
    theme::{BODY, TEXT},
};
use ui::UiVisual;

#[test]
fn inserted_glyphs_share_the_final_layout_and_existing_glyphs_stay_in_place() {
    let bounds = [10.0, 20.0, 400.0, 68.0];
    let draw = |insertion| {
        paint(Default::default(), |canvas| {
            canvas
                .text_line_typing("ABC", bounds, BODY, TEXT, insertion)
                .unwrap();
        })
        .2
    };
    let settled = draw(None);
    let animated = draw(Some(Insertion {
        characters: 1..2,
        progress: 0.5,
    }));
    let settled = settled
        .iter()
        .find(|n| matches!(n.visual(), UiVisual::Text { .. }))
        .unwrap();
    let UiVisual::Text { layout: target, .. } = settled.visual() else {
        unreachable!()
    };
    let text: Vec<_> = animated
        .iter()
        .filter_map(|node| match node.visual() {
            UiVisual::Text { layout, color, .. } => Some((node, layout, color)),
            _ => None,
        })
        .collect();
    assert!(
        text.iter()
            .all(|(_, layout, _)| layout.glyphs() == target.glyphs())
    );
    assert!(
        text.iter()
            .all(|(_, layout, _)| Arc::ptr_eq(layout, text[0].1))
    );
    let old_center = settled.bounds().min().x()
        + (target.glyphs()[0].bounds_64[0] + target.glyphs()[0].bounds_64[2]) as f32 / 128.0;
    let new_center = settled.bounds().min().x()
        + (target.glyphs()[1].bounds_64[0] + target.glyphs()[1].bounds_64[2]) as f32 / 128.0;
    for (node, layout, color) in text {
        let clip = animated
            .iter()
            .find(|n| Some(n.id()) == node.parent())
            .unwrap()
            .bounds();
        let world = [
            clip.min().x() + node.bounds().min().x(),
            clip.min().y() + node.bounds().min().y(),
        ];
        if clip.min().x() <= old_center && clip.max().x() >= old_center {
            assert_eq!(*color, TEXT);
            assert_eq!(
                world,
                [settled.bounds().min().x(), settled.bounds().min().y()]
            );
        }
        if color[3] < TEXT[3] {
            assert!(color[3] > 0);
            assert!(clip.min().x() <= new_center && clip.max().x() >= new_center);
            assert!(world[1] > settled.bounds().min().y());
            assert_eq!(layout.glyphs(), target.glyphs());
        }
    }
}

#[test]
fn long_ellipsized_values_keep_their_final_placement_during_edits() {
    let draw = |insertion| {
        paint(Default::default(), |canvas| {
            canvas
                .text_line_typing(
                    "A long server name",
                    [10.0, 20.0, 75.0, 68.0],
                    BODY,
                    TEXT,
                    insertion,
                )
                .unwrap();
        })
        .2
    };
    let settled = draw(None);
    let animated = draw(Some(Insertion {
        characters: 14..18,
        progress: 0.0,
    }));
    let texts = |nodes: Vec<ui::UiNode>| {
        nodes
            .into_iter()
            .filter(|n| matches!(n.visual(), UiVisual::Text { .. }))
            .collect::<Vec<_>>()
    };
    let (settled, animated) = (texts(settled), texts(animated));
    assert_eq!(settled[0].bounds(), animated[0].bounds());
    assert_eq!(settled[0].visual(), animated[0].visual());
}
