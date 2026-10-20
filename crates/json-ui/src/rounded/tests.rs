use super::*;
use serde_json::json;

/// Renderer data from a compact property fixture.
fn data(value: Value) -> BTreeMap<String, Value> {
    value
        .as_object()
        .unwrap()
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
}

#[test]
fn rounded_shadows_parse_overrides_defaults_and_dynamic_bounds() {
    assert_eq!(Shadow::read(&data(json!({}))), None);
    assert_eq!(Shadow::read(&data(json!({"shadow_blur":8}))), None);
    assert_eq!(Shadow::read(&data(json!({"shadow_color":[0,0,0,0]}))), None);
    let shadow = Shadow::read(&data(json!({"shadow_color":[0,0,0,1],
        "#shadow_color":[1,0.5,0,0.5],"shadow_blur":3,"#shadow_blur":100,
        "shadow_offset":[1,2],"#shadow_offset":[-9000,9000],"shadow_spread":-100})))
    .unwrap();
    assert_eq!(shadow.color, [255, 128, 0, 128]);
    assert_eq!(shadow.blur, MAX_SHADOW_BLUR as f32);
    assert_eq!(
        shadow.offset,
        [-MAX_SHADOW_OFFSET as f32, MAX_SHADOW_OFFSET as f32]
    );
    assert_eq!(shadow.spread, -MAX_SHADOW_SPREAD as f32);
    let default = Shadow::read(&data(json!({"shadow_color":[0,0,0,1]}))).unwrap();
    assert_eq!(
        (default.blur, default.spread, default.offset),
        (0.0, 0.0, [0.0, 0.0])
    );
}

#[test]
fn rounded_shadows_expand_culling_bounds_without_changing_the_rect() {
    let data = data(json!({"shadow_color":[0,0,0,1],"shadow_offset":[4,6],
        "shadow_blur":8,"shadow_spread":2}));
    let dest = Rect::new(40.0, 40.0, 80.0, 40.0);
    assert_eq!(
        paint_bounds(&data, dest, 1.0, None),
        Rect::new(17.5, 19.5, 133.0, 93.0)
    );
    assert_eq!(paint_bounds(&BTreeMap::new(), dest, 1.0, None), dest);
    let transform = crate::transform::Affine {
        linear: [0.25, 0.0, 0.0, 4.0],
        translation: [0.0; 2],
    };
    let bounds = paint_bounds(&data, dest, 1.0, Some(transform));
    assert_eq!(
        transform.bounds(bounds),
        Rect::new(-12.5, 139.5, 73.0, 213.0)
    );
}
