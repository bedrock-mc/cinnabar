use super::*;
use crate::ui_runtime::presentation::tests::fixture_font;

/// Measures repeated label lookup after the same font, text and width are warm.
fn repeated_line(value: &str, width: f32) -> usize {
    let (mut nodes, mut next, mut layouts) =
        (Vec::new(), 1, TextLayoutCache::new(128, 1024 * 1024));
    let font = fixture_font();
    let metrics = TextMetrics::for_viewport([1280, 720], ui::DpiScale::new(1.0).unwrap(), Some(2));
    let mut canvas = Canvas::new(&mut nodes, &mut next, &mut layouts, &font, metrics, 0, None);
    let before = canvas.line_layout(value, width, BODY).unwrap().unwrap();
    let (_, allocations) = crate::allocation_count::count(|| {
        for _ in 0..10 {
            let again = canvas.line_layout(value, width, BODY).unwrap().unwrap();
            assert!(Arc::ptr_eq(&again, &before));
        }
    });
    allocations
}

#[test]
fn unchanged_fitting_normalized_and_clipped_labels_do_not_allocate() {
    for (text, width) in [
        ("Servers", 400.0),
        ("  Creator\t experiences\n", 400.0),
        ("A long experience title that needs clipping", 90.0),
    ] {
        assert_eq!(repeated_line(text, width), 0, "warm label: {text}");
    }
}

#[test]
fn a_changed_width_changes_clipping_without_replacing_the_full_text_layout() {
    let (mut nodes, mut next, mut layouts) =
        (Vec::new(), 1, TextLayoutCache::new(128, 1024 * 1024));
    let font = fixture_font();
    let metrics = TextMetrics::for_viewport([1280, 720], ui::DpiScale::new(1.0).unwrap(), Some(2));
    let mut canvas = Canvas::new(&mut nodes, &mut next, &mut layouts, &font, metrics, 0, None);
    let source = "Featured experiences and more";
    let full = canvas.line_layout(source, 1000.0, BODY).unwrap().unwrap();
    let clipped = canvas.line_layout(source, 90.0, BODY).unwrap().unwrap();
    assert!(clipped.glyphs().len() < full.glyphs().len());
    assert!(clipped.size_64()[0] <= 90 * 64);
    assert!(Arc::ptr_eq(
        &canvas.line_layout(source, 1000.0, BODY).unwrap().unwrap(),
        &full
    ));
    assert!(canvas.line_layout(source, 0.0, BODY).unwrap().is_none());
}
