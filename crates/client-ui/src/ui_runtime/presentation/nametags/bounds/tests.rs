use super::*;
use crate::ui_runtime::presentation::nametags::tests::anchor;

#[test]
fn cached_bounds_refresh_font_and_text_but_project_live_pose_without_layout_work() {
    let font = ui_engine::test_support::fixture_font();
    let mut layouts = TextLayoutCache::new(8, 1 << 20);
    let mut cache = BoundsCache::default();
    let mut tag = anchor("A");
    let clip = glam::camera::rh::proj::directx::perspective_infinite_reverse(
        std::f32::consts::FRAC_PI_3,
        1.0,
        0.025,
    ) * Mat4::from_translation(Vec3::new(0.0, -2.5, 0.0));
    tag.position.x = std::f32::consts::FRAC_PI_6.tan() * 5.0 + 0.15;
    assert!(!cache.intersects_view(&tag, clip, Quat::IDENTITY, &font, &mut layouts, None));
    tag.lines = Arc::from([Arc::from("AAAAAAAAAAAA")]);
    assert!(cache.intersects_view(&tag, clip, Quat::IDENTITY, &font, &mut layouts, None));
    tag.lines = Arc::from([Arc::from("A")]);
    let mut metrics = *font.glyph('0').unwrap();
    metrics.codepoint = 'A';
    metrics.advance_64 = 64 * 64;
    let wider = font.with_glyphs(
        &[assets::SheetGlyph {
            metrics,
            draw_size_64: [64 * 64, 8 * 64],
        }],
        |_| true,
    );
    assert!(cache.intersects_view(&tag, clip, Quat::IDENTITY, &wider, &mut layouts, None));
    let built = layouts.built_layout_count();
    tag.position.z = 5.0;
    tag.depth_tested = true;
    tag.sneaking = true;
    let (visible, allocations) = test_alloc::count(|| {
        cache.intersects_view(&tag, clip, Quat::IDENTITY, &wider, &mut layouts, None)
    });
    assert!(!visible);
    assert_eq!(layouts.built_layout_count(), built);
    assert_eq!(allocations, 0);
}

#[test]
fn bounds_cache_releases_departed_candidates_and_excess_capacity() {
    let font = ui_engine::test_support::fixture_font();
    let mut layouts = TextLayoutCache::new(8, 1 << 20);
    let mut cache = BoundsCache::default();
    let mut tag = anchor("A");
    cache.begin_frame();
    for id in 0..MAX_PRESENTED_NAMETAGS {
        tag.runtime_id = id as u64;
        cache.intersects_view(&tag, Mat4::ZERO, Quat::IDENTITY, &font, &mut layouts, None);
    }
    cache.finish_frame();
    assert_eq!(cache.entries.len(), MAX_PRESENTED_NAMETAGS);
    cache.begin_frame();
    cache.intersects_view(&tag, Mat4::ZERO, Quat::IDENTITY, &font, &mut layouts, None);
    cache.finish_frame();
    assert_eq!(cache.entries.len(), 1);
    assert!(cache.entries.capacity() <= 4);
    cache.begin_frame();
    cache.finish_frame();
    assert!(cache.entries.is_empty());
    assert_eq!(cache.entries.capacity(), 0);
}
