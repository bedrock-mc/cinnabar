//! Element filter bounds and pyramid regions.

use super::*;

#[test]
fn level_regions_cover_the_group_with_a_texel_of_reach() {
    let bounds = UiScissor::new(10, 20, 30, 10);
    assert_eq!(
        level_rect(bounds, 1, [100, 100]),
        Some(UiScissor::new(4, 9, 17, 7))
    );
    assert_eq!(
        level_rect(bounds, 3, [8, 8]),
        Some(UiScissor::new(0, 1, 6, 4))
    );
    assert_eq!(
        level_rect(UiScissor::new(90, 90, 10, 10), 1, [40, 40]),
        None
    );
}
