use serde_json::{Value, json};

use super::{IconRef, icon, index};

fn art(page: u16) -> IconRef {
    IconRef {
        page,
        uv: [0, 0, 16, 16],
        glint: false,
    }
}

#[test]
fn explicit_empty_never_falls_back_to_the_alternate_item_lookup() {
    let icons = [art(0), art(1)];
    let alternate = [(42, art(2))];
    let data = [
        ("#item_renderer_data".into(), Value::Null),
        ("#item_id_aux".into(), json!(42)),
    ]
    .into_iter()
    .collect();
    assert!(icon(&data, &icons, &alternate).is_none());
    let data = [("#item_id_aux".into(), json!(42))].into_iter().collect();
    assert_eq!(icon(&data, &icons, &alternate), Some(&alternate[0].1));
}

#[test]
fn malformed_present_indices_never_fall_back_or_saturate_to_the_first_icon() {
    let icons = [art(0), art(1)];
    let alternate = [(42, art(2))];
    for value in [json!(-1), json!(0.5), json!(2), json!(1e100), json!("0")] {
        let data = [
            ("#item_renderer_data".into(), value),
            ("#item_id_aux".into(), json!(42)),
        ]
        .into_iter()
        .collect();
        assert!(icon(&data, &icons, &alternate).is_none());
    }
    let data = [("#item_renderer_data".into(), json!(1))]
        .into_iter()
        .collect();
    assert_eq!(icon(&data, &icons, &alternate), Some(&icons[1]));
}

#[test]
fn numeric_indices_are_checked_before_casting() {
    for value in [
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
        -1.0,
        0.5,
        2.0,
        f64::MAX,
    ] {
        assert_eq!(index(value, 2), None, "{value}");
    }
    assert_eq!(index(0.0, 2), Some(0));
    assert_eq!(index(1.0, 2), Some(1));
    assert_eq!(index(0.0, 0), None);
}
