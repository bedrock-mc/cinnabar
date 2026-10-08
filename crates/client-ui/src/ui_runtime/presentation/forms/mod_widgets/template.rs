use json_ui::{Catalog, DataSource, Scalar};
use serde_json::{Value, json};
use ui::mod_hud::{Anchor, Card, Hud};

const WIDTH: f64 = 148.;
const ROW: f64 = 20.;
const HEADER: f64 = 18.;

pub(super) fn same_shape(a: &Hud, b: &Hud) -> bool {
    a.cards.len() == b.cards.len()
        && a.cards.iter().zip(&b.cards).all(|(a, b)| {
            a.id == b.id
                && a.anchor == b.anchor
                && a.offset == b.offset
                && a.scale == b.scale
                && a.rows.len() == b.rows.len()
                && a.rows.iter().zip(&b.rows).all(|(a, b)| {
                    a.effect_id == b.effect_id
                        && a.item.is_some() == b.item.is_some()
                        && a.progress.is_some() == b.progress.is_some()
                })
        })
}
fn named(name: &str, node: Value) -> Value {
    json!({name: node})
}
fn rounded(size: [f64; 2], offset: [f64; 2], radius: f64, color: [f32; 4]) -> Value {
    json!({"type":"custom","renderer":"cinnabar_rounded_rectangle","size":size,"offset":offset,
        "anchor_from":"top_left","anchor_to":"top_left","radius":radius,"color":color})
}
fn label(key: &str, size: [f64; 2], offset: [f64; 2], scale: f64, right: bool) -> Value {
    json!({"type":"label","size":size,"offset":offset,"anchor_from":"top_left","anchor_to":"top_left",
        "text":key,"bindings":[{"binding_name":key}],"localize":false,"shadow":true,"hide_hyphen":true,
        "text_alignment": if right {"right"} else {"left"},"font_scale_factor":0.8*scale,"color":[1,1,1,1],"clip_children":true})
}
fn card(card: &Card, card_index: usize, row_index: &mut usize) -> Value {
    let k = f64::from(card.scale);
    let height = HEADER + ROW * card.rows.len() as f64 + 4.;
    let mut controls = vec![
        named(
            "surface",
            rounded(
                [WIDTH * k, height * k],
                [0.; 2],
                3. * k,
                [0.045, 0.05, 0.065, 0.82],
            ),
        ),
        named(
            "title",
            label(
                &format!("#card_{card_index}_title"),
                [(WIDTH - 12.) * k, 12. * k],
                [6. * k, 5. * k],
                k,
                false,
            ),
        ),
        named(
            "divider",
            rounded(
                [(WIDTH - 12.) * k, 0.5 * k],
                [6. * k, 15. * k],
                0.,
                [1., 1., 1., 0.16],
            ),
        ),
    ];
    for (local, row) in card.rows.iter().enumerate() {
        let index = *row_index;
        *row_index += 1;
        let y = (HEADER + ROW * local as f64) * k;
        let left = if row.item.is_some() || row.effect_id.is_some() {
            26.
        } else {
            6.
        };
        if row.item.is_some() {
            controls.push(named(&format!("icon_{index}"),json!({"type":"custom","renderer":"inventory_item_renderer",
                "size":[16.*k,16.*k],"offset":[5.*k,y],"anchor_from":"top_left","anchor_to":"top_left",
                "bindings":[{"binding_name":format!("#row_{index}_icon"),"binding_name_override":"#item_renderer_data"}]})));
        }
        if let Some(role) = row
            .effect_id
            .and_then(super::super::super::hud_layout::effect_icon_role)
        {
            let path = role
                .source_path()
                .strip_suffix(".png")
                .unwrap_or(role.source_path());
            controls.push(named(&format!("effect_{index}"), json!({"type":"image","texture":path,
                "size":[16.*k,16.*k],"offset":[5.*k,y],"anchor_from":"top_left","anchor_to":"top_left"})));
        }
        controls.push(named(
            &format!("label_{index}"),
            label(
                &format!("#row_{index}_label"),
                [(WIDTH - left - 56.) * k, 12. * k],
                [left * k, y + 2. * k],
                k,
                false,
            ),
        ));
        let mut value = label(
            &format!("#row_{index}_value"),
            [44. * k, 12. * k],
            [(WIDTH - 50.) * k, y + 2. * k],
            k,
            true,
        );
        value["bindings"].as_array_mut().unwrap().push(
            json!({"binding_name":format!("#row_{index}_color"),"binding_name_override":"#color"}),
        );
        controls.push(named(&format!("value_{index}"), value));
        if row.progress.is_some() {
            controls.push(named(
                &format!("track_{index}"),
                rounded(
                    [(WIDTH - left - 6.) * k, 2. * k],
                    [left * k, y + 14. * k],
                    0.,
                    [1., 1., 1., 0.18],
                ),
            ));
            let mut fill = rounded(
                [(WIDTH - left - 6.) * k, 2. * k],
                [left * k, y + 14. * k],
                0.,
                [1.; 4],
            );
            fill["bindings"] = json!([
                {"binding_name":format!("#row_{index}_progress"),"binding_name_override":"#size_binding_x"},
                {"binding_name":format!("#row_{index}_color"),"binding_name_override":"#color"}]);
            controls.push(named(&format!("progress_{index}"), fill));
        }
    }
    let anchor = match card.anchor {
        Anchor::TopLeft => "top_left",
        Anchor::TopRight => "top_right",
        Anchor::BottomLeft => "bottom_left",
        Anchor::BottomRight => "bottom_right",
    };
    json!({"type":"panel","size":[WIDTH*k,height*k],"offset":card.offset,"anchor_from":anchor,"anchor_to":anchor,"controls":controls})
}
pub(super) fn catalog(hud: &Hud) -> Result<Catalog, String> {
    let mut index = 0;
    let controls: Vec<_> = hud
        .cards
        .iter()
        .enumerate()
        .map(|(n, c)| named(&c.id, card(c, n, &mut index)))
        .collect();
    let definition = json!({"namespace":"cinnabar_personal_hud","cards":{"type":"panel","size":["100%","100%"],"controls":controls}}).to_string();
    Catalog::from_files([
        ("ui/_global_variables.json", &b"{}"[..]),
        (
            "ui/_ui_defs.json",
            &br#"{"ui_defs":["ui/personal_hud.json"]}"#[..],
        ),
        ("ui/personal_hud.json", definition.as_bytes()),
    ])
    .map_err(|error| error.to_string())
}
pub(super) fn data(hud: &Hud) -> DataSource {
    let mut data = DataSource::new();
    let mut index = 0;
    for (n, card) in hud.cards.iter().enumerate() {
        data.set_global(format!("#card_{n}_title"), Scalar::Text(card.title.clone()));
        for row in &card.rows {
            data.set_global(
                format!("#row_{index}_label"),
                Scalar::Text(row.label.clone()),
            );
            data.set_global(
                format!("#row_{index}_value"),
                Scalar::Text(row.value.clone()),
            );
            data.set_global(
                format!("#row_{index}_color"),
                Scalar::Json(json!(row.color)),
            );
            data.set_global(
                format!("#row_{index}_progress"),
                Scalar::Num(f64::from(row.progress.unwrap_or(0.))),
            );
            data.set_global(format!("#row_{index}_icon"), Scalar::Json(Value::Null));
            index += 1;
        }
    }
    data
}
