use super::*;

/// The full existing quad caps remain admitted under the same checked byte ceiling.
#[test]
fn packed_vertex_flags_preserve_near_cap_draw_admission() {
    let quads = UiLimits::MAX_UI_VERTICES / 4;
    let counts = DrawCounts {
        quads,
        ..DrawCounts::default()
    };
    assert_eq!(std::mem::size_of::<UiVertex>(), 56);
    assert_eq!(
        counts.budget().unwrap(),
        (
            UiLimits::MAX_DRAW_BATCHES,
            UiLimits::MAX_UI_VERTICES,
            UiLimits::MAX_UI_INDICES
        ),
    );
}
