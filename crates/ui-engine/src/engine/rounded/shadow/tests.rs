use super::*;
use serde_json::json;

/// Authored shadow data for cache and paint bounds checks.
fn data() -> BTreeMap<String, Value> {
    serde_json::from_value(json!({"shadow_color":[0,0,0,0.5], "shadow_blur":8,
        "shadow_offset":[4,6], "shadow_spread":2, "radius":10}))
    .unwrap()
}

/// Draws one fixture without a font, carrier or canvas allocation.
fn paint(data: &BTreeMap<String, Value>) -> Option<(UiVisual, [f32; 4])> {
    visual(data, [40.0, 40.0, 120.0, 80.0], 1.0, 1.0, 0, &|color| color)
}

#[test]
fn rounded_shadows_expand_paint_with_one_analytic_quad_and_clamped_radius() {
    let (UiVisual::TintedMesh { mesh, color }, bounds) = paint(&data()).unwrap() else {
        panic!()
    };
    assert_eq!(bounds, [18.0, 20.0, 150.0, 112.0]);
    assert_eq!(color, [0, 0, 0, 128]);
    assert_eq!(mesh.vertices().len(), 4);
    assert_eq!(mesh.indices(), [0, 1, 2, 0, 2, 3]);
    assert_eq!(mesh.vertices()[0].overlay_color, [42.0, 22.0, 12.0, 8.0]);
    assert_eq!(mesh.vertices()[0].uv, [-66.0, -46.0]);
    let mut shrunk = data();
    shrunk.insert("shadow_spread".into(), json!(-21));
    assert!(paint(&shrunk).is_none());
}

#[test]
fn rounded_shadows_reuse_geometry_during_steady_frames_and_color_offset_transitions() {
    let mut data = data();
    let (UiVisual::TintedMesh { mesh: first, .. }, _) = paint(&data).unwrap() else {
        panic!()
    };
    let (UiVisual::TintedMesh { mesh: steady, .. }, _) = paint(&data).unwrap() else {
        panic!()
    };
    assert!(Arc::ptr_eq(&first, &steady));
    data.insert("#shadow_color".into(), json!([1, 0, 0, 1]));
    data.insert("#shadow_offset".into(), json!([-10, 12]));
    let (UiVisual::TintedMesh { mesh: moved, color }, bounds) = paint(&data).unwrap() else {
        panic!()
    };
    assert!(Arc::ptr_eq(&first, &moved));
    assert_eq!(color, [255, 0, 0, 255]);
    assert_eq!(bounds, [4.0, 26.0, 136.0, 118.0]);
    let (_, allocations) = test_alloc::measure(|| {
        for _ in 0..20 {
            assert!(paint(&data).is_some());
        }
    });
    assert_eq!(allocations.calls, 0);
}

#[test]
fn rounded_shadows_absent_or_transparent_do_no_work() {
    assert!(paint(&BTreeMap::new()).is_none());
    let mut data = data();
    data.insert("shadow_color".into(), json!([0, 0, 0, 0]));
    assert!(paint(&data).is_none());
}

#[test]
fn rounded_shadows_geometry_cache_stays_bounded_during_size_changes() {
    let mut cache = MeshCache::default();
    for width in 1..=crate::engine::rounded::MAX_CACHED_SHAPES * 2 {
        let key = Key {
            size: [(width as f32).to_bits(), 20.0f32.to_bits()],
            radius: 0.0f32.to_bits(),
            blur: 4.0f32.to_bits(),
            fringe: 1.0f32.to_bits(),
            page: 0,
        };
        cache.get_or_build(key, mesh).unwrap();
    }
    assert_eq!(
        cache.entries.len(),
        crate::engine::rounded::MAX_CACHED_SHAPES
    );
}
