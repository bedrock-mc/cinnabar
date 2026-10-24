use serde_json::{Value, json};

use {
    super::{icon, index, item_id_aux},
    ui::IconRef,
};

fn art(page: u16) -> IconRef {
    IconRef {
        color_mask: None,
        page,
        uv: [0, 0, 16, 16],
        glint: false,
    }
}

#[test]
fn reserved_empty_icon_indices_never_render() {
    let icons = [
        IconRef {
            uv: [0; 4],
            ..art(0)
        },
        art(1),
    ];
    let data = [("#item_renderer_data".into(), json!(0))]
        .into_iter()
        .collect();
    assert!(
        icon(&data, &icons, &[]).is_none(),
        "a retained index must not draw an empty cell"
    );
    let data = [("#item_renderer_data".into(), json!(1))]
        .into_iter()
        .collect();
    assert_eq!(icon(&data, &icons, &[]), Some(&icons[1]));
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

#[test]
fn pack_item_keys_preserve_signed_ids_and_auxiliary_values() {
    for id in [-123, 35, 12_000] {
        for aux in [0, 3, 0x7fff] {
            let packed = ((id << 16) | aux) as u32;
            for value in [json!(packed), json!(packed as i32)] {
                let data = [("#item_id_aux".into(), value)].into_iter().collect();
                assert_eq!(item_id_aux(&data), Some((id, aux as u32)));
            }
        }
    }
}

#[test]
fn malformed_or_empty_pack_item_keys_never_select_an_icon() {
    for value in [
        Value::Null,
        json!("42"),
        json!(0.5),
        json!(-2147483649i64),
        json!(4294967296u64),
        json!(-1),
        json!(u32::MAX),
    ] {
        let data = [("#item_id_aux".into(), value)].into_iter().collect();
        assert_eq!(item_id_aux(&data), None);
    }
    let data = [
        ("#item_id_aux".into(), json!(35 << 16)),
        ("#item_renderer_data".into(), Value::Null),
    ]
    .into_iter()
    .collect();
    assert_eq!(item_id_aux(&data), None);
}
