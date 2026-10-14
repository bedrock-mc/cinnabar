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

#[test]
fn aggregate_mesh_batches_cap_reservation_and_preserve_the_upper_bound() {
    let batches = UiLimits::MAX_DRAW_BATCHES + 1;
    let counts = DrawCounts {
        mesh_vertices: batches * 3,
        mesh_indices: batches * 3,
        mesh_batches: batches,
        ..Default::default()
    };
    assert_eq!(
        counts.budget().unwrap(),
        (UiLimits::MAX_DRAW_BATCHES, batches * 3, batches * 3)
    );
    assert_eq!(counts.batch_upper_bound().unwrap(), batches);
}
