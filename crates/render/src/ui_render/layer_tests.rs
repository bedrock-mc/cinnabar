use super::*;

/// Makes a depth-free alpha batch suitable for the first raster pass.
fn batch() -> UiRenderBatch {
    UiRenderBatch::new(
        0,
        UiScissor::new(0, 0, 40, 40),
        0,
        3,
        render_model::UI_BLEND_ALPHA,
    )
}

#[test]
fn partial_clear_waits_for_its_pipeline_and_an_ordinary_first_pass() {
    let rect = UiDamage::Rect(UiScissor::new(2, 3, 4, 5));
    let first = batch();
    assert_eq!(damage_for_passes(rect, Some(&first), true, true), rect);
    assert_eq!(
        damage_for_passes(rect, Some(&first), false, true),
        UiDamage::Full
    );
    assert_eq!(damage_for_passes(rect, None, true, true), UiDamage::Full);
    assert_eq!(
        damage_for_passes(rect, Some(&first), true, false),
        UiDamage::Full
    );
    for change in 0..3 {
        let mut first = batch();
        match change {
            0 => first.depth_test = 1,
            1 => first.depth_write = 1,
            2 => first.isolated_depth_scope = Some(1),
            _ => unreachable!(),
        }
        assert_eq!(
            damage_for_passes(rect, Some(&first), true, true),
            UiDamage::Full
        );
    }
    assert_eq!(
        damage_for_passes(UiDamage::Unchanged, None, false, false),
        UiDamage::Unchanged
    );
}

#[test]
fn replay_scissors_never_expand_authored_clipping_or_dirty_coverage() {
    let scissor = UiScissor::new(10, 20, 30, 40);
    assert_eq!(clipped_scissor(scissor, None), Some(scissor));
    assert_eq!(
        clipped_scissor(scissor, Some(UiScissor::new(0, 0, 20, 30))),
        Some(UiScissor::new(10, 20, 10, 10))
    );
    assert_eq!(
        clipped_scissor(scissor, Some(UiScissor::new(40, 20, 3, 3))),
        None
    );
    assert_eq!(
        clipped_scissor(scissor, Some(UiScissor::new(u32::MAX, 0, 2, 30))),
        None
    );
}
