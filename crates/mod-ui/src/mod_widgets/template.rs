use json_ui::{Catalog, DataSource, Scalar};
use serde_json::{Value, json};
use ui::mod_hud::{Card, DEFAULT_ROW_HEIGHT, Hud, RowLayout};

const HEADER: f64 = 18.;
/// Keeps canonical artwork away from viewport edges until its actual bounds are measured.
pub(super) const CANVAS_INSET: f64 = ui::mod_panel::preview::MAX_PREVIEW_EXTENT;

#[cfg(test)]
mod tests;

/// Rebuilds only when card geometry, row font or icon topology changes.
pub(in super::super) fn same_shape(a: &Hud, b: &Hud) -> bool {
    super::world_labels::same_shape(&a.world_labels, &b.world_labels)
        && a.cards.len() == b.cards.len()
        && a.cards.iter().zip(&b.cards).all(|(left, right)| {
            left.id == right.id
                && left.scale == right.scale
                && left.row_layout == right.row_layout
                && left.width == right.width
                && left.row_height == right.row_height
                && left.icon_size == right.icon_size
                && left.text_scale == right.text_scale
                && left.font == right.font
                && left.shadow == right.shadow
                && left.title.is_empty() == right.title.is_empty()
                && left.compass.is_some() == right.compass.is_some()
                && left.text_plate.is_some() == right.text_plate.is_some()
                && left.text_plate.as_ref().map(|p| p.style.border_width)
                    == right.text_plate.as_ref().map(|p| p.style.border_width)
                && left.cells.len() == right.cells.len()
                && left.cells.iter().zip(&right.cells).all(|(a, b)| {
                    a.rect == b.rect
                        && a.shadow == b.shadow
                        && a.value.is_empty() == b.value.is_empty()
                })
                && left.rows.len() == right.rows.len()
                && left.rows.iter().zip(&right.rows).all(|(a, b)| {
                    a.effect_id == b.effect_id
                        && a.localized == b.localized
                        && a.item.is_some() == b.item.is_some()
                        && a.progress.is_some() == b.progress.is_some()
                        && (left.row_layout != RowLayout::IconRight
                            || a.label.is_empty() == b.label.is_empty())
                })
        })
}
pub(super) fn named(name: &str, node: Value) -> Value {
    json!({name: node})
}
pub(super) fn rounded(size: [f64; 2], offset: [f64; 2], radius: f64, color: [f32; 4]) -> Value {
    json!({"type":"custom","renderer":"cinnabar_rounded_rectangle","size":size,"offset":offset,
        "anchor_from":"top_left","anchor_to":"top_left","radius":radius,"color":color})
}
fn label(key: &str, size: [f64; 2], offset: [f64; 2], scale: f64, right: bool) -> Value {
    json!({"type":"label","size":size,"offset":offset,"anchor_from":"top_left","anchor_to":"top_left",
        "text":key,"bindings":[{"binding_name":key}],"localize":false,"shadow":true,"hide_hyphen":true,
        "text_alignment": if right {"right"} else {"left"},"font_scale_factor":0.8*scale,"color":[1,1,1,1],"clip_children":true})
}
/// Centers the measured text height on an icon row instead of its reserved band.
fn center_on_row(label: &mut Value, center_y: f64) {
    label["max_size"] = label["size"].clone();
    label["size"][1] = json!("default");
    label["anchor_to"] = json!("left_middle");
    label["offset"][1] = json!(center_y);
}
/// Shares card controls between gameplay publication and native previews.
pub(in super::super) fn card(card: &Card, card_index: usize, row_index: &mut usize) -> Value {
    let mut node = card_content(card, card_index, row_index);
    if card.font != ui::mod_hud::CardFont::Native {
        apply_font(&mut node, card.font.name());
    }
    node
}

/// Applies card typography to retained labels, including cells and measured text spans.
fn apply_font(node: &mut Value, font: &str) {
    match node {
        Value::Object(object) => {
            if object.get("type").and_then(Value::as_str) == Some("label") {
                object.insert("font_type".into(), json!(font));
            }
            for value in object.values_mut() {
                apply_font(value, font);
            }
        }
        Value::Array(values) => {
            for value in values {
                apply_font(value, font);
            }
        }
        _ => {}
    }
}

/// Builds card geometry before applying its selected typography.
fn card_content(card: &Card, card_index: usize, row_index: &mut usize) -> Value {
    if card.text_plate.is_some() {
        *row_index += card.rows.len();
        return super::text_plate::control(card, card_index);
    }
    let k = f64::from(card.scale);
    let width = f64::from(card.width);
    let row_height = f64::from(card.row_height);
    let icon_size = f64::from(card.icon_size);
    let font_multiplier = f64::from(card.text_scale);
    let header = if card.title.is_empty() { 0. } else { HEADER };
    let size = dimensions(card);
    let mut controls = vec![
        named(
            "surface",
            rounded(
                size,
                [0.; 2],
                3. * k,
                [0.045, 0.05, 0.065, card.background_opacity],
            ),
        ),
        named(
            "title",
            label(
                &format!("#card_{card_index}_title"),
                [(width - 12.) * k, 12. * k],
                [6. * k, 5. * k],
                k,
                false,
            ),
        ),
        named(
            "divider",
            rounded(
                [(width - 12.) * k, 0.5 * k],
                [6. * k, 15. * k],
                0.,
                [1., 1., 1., 0.16],
            ),
        ),
    ];
    controls[0]["surface"]["bindings"] = json!([{ "binding_name":format!("#card_{card_index}_background"),"binding_name_override":"#color" }]);
    if card.title.is_empty() {
        controls.truncate(1);
    }
    if card.compass.is_some() {
        controls.push(named(
            "compass",
            super::compass::control(card, card_index, header),
        ));
        return panel_for_card(card_index, size, controls);
    }
    if !card.cells.is_empty() {
        controls.extend(super::cells::controls(card, card_index, header));
    }
    if card.row_layout == RowLayout::Grid {
        controls.extend(grid(card, row_index, header));
        return panel_for_card(card_index, size, controls);
    }
    for (local, row) in card.rows.iter().enumerate() {
        let index = *row_index;
        *row_index += 1;
        let y = (header + row_height * local as f64) * k;
        let has_icon = row.item.is_some() || row.effect_id.is_some();
        let icon_x = if card.row_layout == RowLayout::IconRight {
            width - icon_size - 5.
        } else {
            5.
        };
        let icon_y = y + if card.row_layout == RowLayout::Standard {
            0.
        } else {
            (row_height - icon_size) * k * 0.5
        };
        if row.item.is_some() {
            controls.push(named(&format!("icon_{index}"),json!({"type":"custom","renderer":"inventory_item_renderer",
                "size":[icon_size*k,icon_size*k],"offset":[icon_x*k,icon_y],"anchor_from":"top_left","anchor_to":"top_left",
                "bindings":[{"binding_name":format!("#row_{index}_icon"),"binding_name_override":"#item_renderer_data"}]})));
        }
        if let Some(role) = row
            .effect_id
            .and_then(|id| assets::effect_descriptor(id).and_then(|descriptor| descriptor.icon))
        {
            let path = role
                .source_path()
                .strip_suffix(".png")
                .unwrap_or(role.source_path());
            controls.push(named(&format!("effect_{index}"), json!({"type":"image","texture":path,
                "size":[icon_size*k,icon_size*k],"offset":[icon_x*k,icon_y],"anchor_from":"top_left","anchor_to":"top_left"})));
        }
        let left = if has_icon && card.row_layout != RowLayout::IconRight {
            icon_size + 10.
        } else {
            6.
        };
        let right = if has_icon && card.row_layout == RowLayout::IconRight {
            icon_x - 4.
        } else {
            width - 6.
        };
        let text_width = (right - left).max(0.);
        let stacked = card.row_layout == RowLayout::StackedText;
        let value_width =
            if stacked || card.row_layout == RowLayout::IconRight && row.label.is_empty() {
                text_width
            } else {
                44_f64.min((text_width - 6.).max(0.))
            };
        let label_width = if stacked {
            text_width
        } else {
            (text_width - value_width - 6.).max(0.)
        };
        // Reserve two pixels each for top/bottom padding, the bar and its gap.
        // Standard rows at the original height keep their existing placement.
        let reserve_progress = row.progress.is_some()
            && !stacked
            && (card.row_layout == RowLayout::IconRight
                || row_height < f64::from(DEFAULT_ROW_HEIGHT)
                || font_multiplier > 1.);
        let base_line_height = if stacked {
            (row_height - if row.progress.is_some() { 8. } else { 4. }) * 0.5
        } else if reserve_progress {
            (row_height - 8.).min(12.)
        } else {
            (row_height - 4.).min(12.)
        };
        let line_height = if stacked {
            base_line_height
        } else {
            (row_height - if reserve_progress { 8. } else { 4. }).min(12. * font_multiplier)
        };
        let text_scale = k * (base_line_height / 12.).min(1.) * font_multiplier;
        let text_top = if card.row_layout == RowLayout::IconRight {
            if reserve_progress {
                2. + (row_height - 8. - line_height) * 0.5
            } else {
                (row_height - line_height) * 0.5
            }
        } else {
            2.
        };
        let mut row_label = label(
            &format!("#row_{index}_label"),
            [label_width * k, line_height * k],
            [left * k, y + text_top * k],
            text_scale,
            false,
        );
        let mut value = label(
            &format!("#row_{index}_value"),
            [value_width * k, line_height * k],
            [
                if stacked { left } else { right - value_width } * k,
                y + (text_top + if stacked { line_height } else { 0. }) * k,
            ],
            text_scale,
            !stacked,
        );
        row_label["shadow"] = json!(card.shadow);
        row_label["localize"] = json!(row.localized);
        value["shadow"] = json!(card.shadow);
        if card.row_layout == RowLayout::IconRight && !reserve_progress {
            let center_y = y + row_height * k * 0.5;
            center_on_row(&mut row_label, center_y);
            center_on_row(&mut value, center_y);
        }
        controls.push(named(&format!("label_{index}"), row_label));
        value["bindings"].as_array_mut().unwrap().push(
            json!({"binding_name":format!("#row_{index}_color"),"binding_name_override":"#color"}),
        );
        controls.push(named(&format!("value_{index}"), value));
        if row.progress.is_some() {
            let size = [text_width * k, 2. * k];
            let track = rounded(size, [0.; 2], 0., [1., 1., 1., 0.18]);
            let mut fill = rounded(size, [0.; 2], 0., [1.; 4]);
            fill["bindings"] = json!([
                {"binding_name":format!("#row_{index}_progress"),"binding_name_override":"#size_binding_x"},
                {"binding_name":format!("#row_{index}_color"),"binding_name_override":"#color"}]);
            controls.push(named(
                &format!("progress_{index}"),
                json!({
                    "type":"panel", "size":size,
                    "offset":[left*k,y+(row_height-if stacked || reserve_progress {4.} else {6.})*k],
                    "anchor_from":"top_left", "anchor_to":"top_left",
                    "controls":[named("track",track),named("fill",fill)]
                }),
            ));
        }
    }
    panel_for_card(card_index, size, controls)
}

/// Places one card at its live normalized or anchored position.
fn panel_for_card(card_index: usize, size: [f64; 2], controls: Vec<Value>) -> Value {
    json!({"type":"panel","size":size,"offset":[0,0],
        "anchor_from":"top_left","anchor_to":"top_left","controls":controls,
        "bindings":[{"binding_name":format!("#card_{card_index}_offset"),"binding_name_override":"#offset"}]})
}

/// Draws item cells using the same row icon bindings as ordinary equipment lists.
fn grid(card: &Card, row_index: &mut usize, header: f64) -> Vec<Value> {
    let k = f64::from(card.scale);
    let step = f64::from(card.row_height);
    let columns = grid_columns(card);
    let icon = f64::from(card.icon_size);
    let mut controls = Vec::new();
    for (local, row) in card.rows.iter().enumerate() {
        let index = *row_index;
        *row_index += 1;
        let x = local % columns;
        let y = local / columns;
        let offset = [
            (x as f64 * step + (step - icon) * 0.5) * k,
            (header + y as f64 * step + (step - icon) * 0.5) * k,
        ];
        if row.item.is_some() {
            controls.push(named(&format!("grid_icon_{index}"),json!({"type":"custom","renderer":"inventory_item_renderer","size":[icon*k,icon*k],"offset":offset,"anchor_from":"top_left","anchor_to":"top_left","bindings":[{"binding_name":format!("#row_{index}_icon"),"binding_name_override":"#item_renderer_data"}]})));
        }
        let mut count = label(
            &format!("#row_{index}_value"),
            [step * k, 10. * k],
            [
                x as f64 * step * k,
                (header + (y + 1) as f64 * step - 10.) * k,
            ],
            0.75 * k * f64::from(card.text_scale),
            true,
        );
        count["shadow"] = json!(card.shadow);
        count["bindings"].as_array_mut().unwrap().push(
            json!({"binding_name":format!("#row_{index}_color"),"binding_name_override":"#color"}),
        );
        controls.push(named(&format!("grid_count_{index}"), count));
    }
    controls
}

/// Derives grid width from the published card geometry, with no separate column setting.
fn grid_columns(card: &Card) -> usize {
    ((card.width / card.row_height).floor() as usize).max(1)
}
pub(super) fn catalog(hud: &Hud) -> Result<Catalog, String> {
    let mut document = json!({"namespace":"cinnabar_personal_hud"});
    let mut row = 0;
    let mut controls = Vec::new();
    let mut gameplay = Vec::new();
    for (index, source) in hud.cards.iter().enumerate() {
        let mut body = card(source, index, &mut row);
        let mut placement = body["bindings"].clone();
        placement
            .as_array_mut()
            .expect("card bindings")
            .push(json!({
            "binding_name":format!("#card_{index}_visible"),"binding_name_override":"#visible"}));
        body["bindings"]
            .as_array_mut()
            .expect("card bindings")
            .retain(|binding| {
                !matches!(
                    binding["binding_name_override"].as_str(),
                    Some("#offset" | "#visible")
                )
            });
        body["offset"] = json!([CANVAS_INSET, CANVAS_INSET]);
        document[format!("card_{index}")] = body;
        controls.push(named(
            &format!("{}@cinnabar_personal_hud.card_{index}", source.id),
            json!({"bindings":placement}),
        ));
        gameplay.push(named(
            &format!("card_{index}"),
            json!({"type":"custom","renderer":"cinnabar_gameplay_card",
            "card_index":index,"size":["100%","100%"]}),
        ));
    }
    for (index, label) in hud.world_labels.iter().enumerate() {
        let control = super::world_labels::control(index, label);
        controls.push(control.clone());
        gameplay.push(control);
    }
    document["cards"] = json!({"type":"panel","size":["100%","100%"],"controls":controls});
    document["gameplay"] = json!({"type":"panel","size":["100%","100%"],"controls":gameplay});
    let definition = document.to_string();
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
pub(in super::super) fn data(hud: &Hud, viewport: [f64; 2]) -> DataSource {
    let mut data = DataSource::new();
    let mut index = 0;
    for (n, card) in hud.cards.iter().enumerate() {
        card_data(&mut data, card, n, index, viewport);
        index += card.rows.len();
    }
    data
}

/// Writes one card's indexed bindings for both aggregate HUDs and isolated retained cards.
pub(super) fn card_data(
    data: &mut DataSource,
    card: &Card,
    n: usize,
    mut index: usize,
    viewport: [f64; 2],
) {
    data.set_global(
        format!("#card_{n}_visible"),
        Scalar::Bool(!card.preview_only),
    );
    data.set_global(format!("#card_{n}_title"), Scalar::Text(card.title.clone()));
    data.set_global(
        format!("#card_{n}_offset"),
        Scalar::Json(json!(origin(card, viewport))),
    );
    data.set_global(
        format!("#card_{n}_background"),
        Scalar::Json(json!([0.045, 0.05, 0.065, card.background_opacity])),
    );
    super::cells::data(data, card, n);
    super::text_plate::data(data, card, n, viewport);
    super::compass::data(data, card, n);
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

/// Shared card geometry for gameplay publication and native layout editing.
pub(in super::super) fn dimensions(card: &Card) -> [f64; 2] {
    if card.text_plate.is_some() {
        return super::text_plate::dimensions(card);
    }
    if card.compass.is_some() {
        return [
            f64::from(card.width * card.scale),
            (if card.title.is_empty() { 0. } else { HEADER }
                + f64::from(ui::mod_hud::COMPASS_HEIGHT))
                * f64::from(card.scale),
        ];
    }
    if card.row_layout == RowLayout::Grid {
        return [
            f64::from(card.width * card.scale),
            (if card.title.is_empty() { 0. } else { HEADER }
                + card.rows.len().div_ceil(grid_columns(card)) as f64 * f64::from(card.row_height)
                + 4.)
                * f64::from(card.scale),
        ];
    }
    if !card.cells.is_empty() {
        return [
            f64::from(card.width * card.scale),
            (if card.title.is_empty() { 0. } else { HEADER }
                + f64::from(
                    card.cells
                        .iter()
                        .map(|c| c.rect[1] + c.rect[3])
                        .fold(0_f32, f32::max),
                ))
                * f64::from(card.scale),
        ];
    }
    let header = if card.title.is_empty() { 0. } else { HEADER };
    [
        f64::from(card.width) * f64::from(card.scale),
        (header + f64::from(card.row_height) * card.rows.len() as f64 + 4.) * f64::from(card.scale),
    ]
}
/// Resolves saved normalized travel or the authored anchored offset.
pub(in super::super) fn origin(card: &Card, viewport: [f64; 2]) -> [f64; 2] {
    let size = dimensions(card);
    let available = std::array::from_fn::<_, 2, _>(|axis| (viewport[axis] - size[axis]).max(0.));
    if let Some(position) = card.position {
        return std::array::from_fn(|axis| f64::from(position[axis]) * available[axis]);
    }
    card.anchor.origin(card.offset, size, viewport)
}
