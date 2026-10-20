//! Measured text plates share exact font metrics with JSON-UI labels and native editor bounds.

use json_ui::{DataSource, LabelShape, Scalar, TextMeasure};
use serde_json::{Value, json};
use ui::mod_hud::{Card, Hud, TextPlateLayout};

/// Reads only the plain value admitted by the text-plate schema.
fn value(card: &Card) -> &str {
    card.rows.first().map_or("", |row| row.value.as_str())
}

/// Measures actual glyph runs at their paint scale; stable runs reuse the engine's text cache.
pub(in super::super) fn measure(hud: &mut Hud, text: &dyn TextMeasure) -> bool {
    let mut changed = false;
    for card in &mut hud.cards {
        let Some(plate) = &card.text_plate else {
            continue;
        };
        let k = f64::from(card.scale);
        let shape = LabelShape {
            scale: 0.8 * k * f64::from(card.text_scale),
            line_padding: 0.,
            hide_hyphen: true,
        };
        let words = [
            if plate.style.brackets { "[" } else { "" },
            value(card),
            if plate.style.brackets { "]" } else { "" },
        ];
        let mut extents = words.map(|word| {
            text.named_label(word, card.font.name(), None, None, shape)
                .map(|v| if v.is_finite() { v.max(0.) } else { 0. })
        });
        let mut remaining = f64::from(ui::mod_hud::MAX_CARD_WIDTH) * k;
        // Reserve both brackets before fitting the value into the shared width limit.
        for index in [0, 2, 1] {
            extents[index][0] = extents[index][0].min(remaining);
            remaining -= extents[index][0];
        }
        let width = extents.iter().map(|v| v[0]).sum::<f64>();
        let height = extents
            .iter()
            .map(|v| v[1])
            .fold(0_f64, f64::max)
            .min(f64::from(ui::mod_hud::MAX_TEXT_PLATE_HEIGHT) * k);
        let background = [
            plate
                .style
                .width
                .map_or(width + 8. * k, |v| f64::from(v) * k),
            plate
                .style
                .height
                .map_or(height + 4. * k, |v| f64::from(v) * k),
        ];
        let size = [background[0].max(width), background[1].max(height)];
        let mut x = (size[0] - width) * 0.5;
        let spans = std::array::from_fn(|index| {
            let w = extents[index][0];
            let rect = [x, (size[1] - height) * 0.5, w, height];
            x += w;
            rect
        });
        let layout = TextPlateLayout {
            scale: card.scale,
            size,
            background: [
                (size[0] - background[0]) * 0.5,
                (size[1] - background[1]) * 0.5,
                background[0],
                background[1],
            ],
            spans,
        };
        let plate = card.text_plate.as_mut().expect("measured plate");
        if plate.layout.as_ref() != Some(&layout) {
            plate.layout = Some(layout);
            changed = true;
        }
    }
    changed
}

/// Keeps native corner resizing coherent before the next font measurement is painted.
pub(super) fn dimensions(card: &Card) -> [f64; 2] {
    let plate = card.text_plate.as_ref().expect("text plate");
    let k = f64::from(card.scale);
    plate.layout.as_ref().map_or_else(
        || {
            [
                f64::from(plate.style.width.unwrap_or(card.width)) * k,
                f64::from(plate.style.height.unwrap_or(card.row_height)) * k,
            ]
        },
        |layout| layout.size.map(|v| v * k / f64::from(layout.scale)),
    )
}

/// Creates retained spans and inside-border chrome whose geometry comes from measured bindings.
pub(super) fn control(card: &Card, n: usize) -> Value {
    let plate = card.text_plate.as_ref().expect("text plate");
    let prefix = format!("#plate_{n}");
    let mut controls = vec![
        json!({"background":{"type":"custom","renderer":"cinnabar_rounded_rectangle","radius":0,
        "size":[1,1],"border_width":plate.style.border_width*card.scale,"border_color":plate.style.border_color,
        "anchor_from":"top_left","anchor_to":"top_left","bindings":[
            {"binding_name":format!("{prefix}_background_width"),"binding_name_override":"#size_binding_x_absolute"},
            {"binding_name":format!("{prefix}_background_height"),"binding_name_override":"#size_binding_y_absolute"},
            {"binding_name":format!("{prefix}_background_offset"),"binding_name_override":"#offset"},
            {"binding_name":format!("{prefix}_background"),"binding_name_override":"#color"},
            {"binding_name":format!("{prefix}_border"),"binding_name_override":"#border_color"}]}}),
    ];
    for span in 0..3 {
        let key = format!("{prefix}_{span}");
        controls.push(json!({format!("span_{span}"):{"type":"label","text":format!("{key}_text"),"size":[1,1],
            "anchor_from":"top_left","anchor_to":"top_left","localize":false,"hide_hyphen":true,"shadow":card.shadow,
            "font_scale_factor":0.8*card.scale*card.text_scale,"clip_children":false,"bindings":[
                {"binding_name":format!("{key}_text")},
                {"binding_name":format!("{key}_width"),"binding_name_override":"#size_binding_x_absolute"},
                {"binding_name":format!("{key}_height"),"binding_name_override":"#size_binding_y_absolute"},
                {"binding_name":format!("{key}_offset"),"binding_name_override":"#offset"},
                {"binding_name":format!("{key}_color"),"binding_name_override":"#color"}]}}));
    }
    json!({"type":"panel","size":dimensions(card),"anchor_from":"top_left","anchor_to":"top_left","controls":controls,
        "bindings":[{"binding_name":format!("#card_{n}_offset"),"binding_name_override":"#offset"},
            {"binding_name":format!("{prefix}_width"),"binding_name_override":"#size_binding_x_absolute"},
            {"binding_name":format!("{prefix}_height"),"binding_name_override":"#size_binding_y_absolute"}]})
}

/// Updates measured geometry and colors without discarding sibling item-icon bindings.
pub(in super::super) fn data(data: &mut DataSource, card: &Card, n: usize, viewport: [f64; 2]) {
    let Some(plate) = &card.text_plate else {
        return;
    };
    let prefix = format!("#plate_{n}");
    let size = dimensions(card);
    let ratio = plate
        .layout
        .as_ref()
        .map_or(1., |v| f64::from(card.scale / v.scale));
    let rect = plate
        .layout
        .as_ref()
        .map_or([0., 0., size[0], size[1]], |v| {
            v.background.map(|v| v * ratio)
        });
    write_size(data, &prefix, size);
    data.set_global(
        format!("#card_{n}_offset"),
        Scalar::Json(json!(super::template::origin(card, viewport))),
    );
    write_size(data, &format!("{prefix}_background"), [rect[2], rect[3]]);
    data.set_global(
        format!("{prefix}_background_offset"),
        Scalar::Json(json!([rect[0], rect[1]])),
    );
    data.set_global(
        format!("{prefix}_background"),
        Scalar::Json(json!(plate.style.background)),
    );
    data.set_global(
        format!("{prefix}_border"),
        Scalar::Json(json!(plate.style.border_color)),
    );
    let words = [
        if plate.style.brackets { "[" } else { "" },
        value(card),
        if plate.style.brackets { "]" } else { "" },
    ];
    for (span, word) in words.into_iter().enumerate() {
        let key = format!("{prefix}_{span}");
        let rect = plate
            .layout
            .as_ref()
            .map_or([0.; 4], |v| v.spans[span].map(|v| v * ratio));
        data.set_global(format!("{key}_text"), Scalar::Text(word.into()));
        write_size(data, &key, [rect[2], rect[3]]);
        data.set_global(
            format!("{key}_offset"),
            Scalar::Json(json!([rect[0], rect[1]])),
        );
        data.set_global(
            format!("{key}_color"),
            Scalar::Json(json!(if span == 1 {
                card.rows.first().map_or([1.; 4], |r| r.color)
            } else {
                plate.style.bracket_color
            })),
        );
    }
}

/// Native JSON-UI exposes absolute size as two scalar targets rather than a vector target.
fn write_size(data: &mut DataSource, prefix: &str, size: [f64; 2]) {
    data.set_global(format!("{prefix}_width"), Scalar::Num(size[0]));
    data.set_global(format!("{prefix}_height"), Scalar::Num(size[1]));
}

#[cfg(test)]
mod tests {
    use super::*;
    use json_ui::{Context, Draw, LayoutEnv, TextureMeta, TextureSource, ViewState};
    use ui_engine::cached_screen::CachedScreen;
    struct Metrics;
    impl TextMeasure for Metrics {
        /// Fixed glyph metrics isolate dimensions and span alignment from local font carriers.
        fn extent(&self, text: &str) -> [f64; 2] {
            if text.is_empty() {
                [0.; 2]
            } else {
                [text.chars().count() as f64 * 5., 8.]
            }
        }
    }
    impl TextureSource for Metrics {
        /// Plates use native rectangles and need no texture fixtures.
        fn texture(&self, _: &str) -> Option<TextureMeta> {
            None
        }
    }
    /// Creates distinct translucent colors so span tint and chrome can be checked independently.
    fn fixture() -> Hud {
        serde_json::from_str(r#"{"cards":[{"id":"clock","rows":[{"label":"","value":"12:05 AM","color":[1,0,0,0.5]}],"text_plate":{"brackets":true,"bracket_color":[0,1,0,0.5],"background":[0,0,1,0.5],"width":56,"height":18,"border_width":0.5,"border_color":[1,1,0,0.5]}}]}"#).unwrap()
    }
    /// Native fonts drive fitting while fixed dimensions remain exact, even if text is wider.
    #[test]
    fn measured_plate_respects_fixed_and_dynamic_background_dimensions() {
        let mut hud = fixture();
        hud.validate().unwrap();
        assert!(measure(&mut hud, &Metrics));
        let card = &mut hud.cards[0];
        assert_eq!(dimensions(card), [56., 18.]);
        let plate = card.text_plate.as_mut().unwrap();
        plate.style.width = None;
        plate.style.height = None;
        assert!(measure(&mut hud, &Metrics));
        let size = dimensions(&hud.cards[0]);
        assert_eq!(size, [48., 10.4]);
        hud.cards[0].rows[0].value = "1:05".into();
        assert!(measure(&mut hud, &Metrics));
        assert_eq!(dimensions(&hud.cards[0]), [32., 10.4]);
        assert!(
            !measure(&mut hud, &Metrics),
            "unchanged text must not invalidate native geometry"
        );
        hud.cards[0].scale = 5.;
        assert_eq!(
            dimensions(&hud.cards[0]),
            [160., 52.],
            "resize geometry follows the new scale before paint"
        );
        assert!(measure(&mut hud, &Metrics));
        assert_eq!(dimensions(&hud.cards[0]), [160., 52.]);
    }

    /// Long admitted values share one width budget with both brackets at every HUD scale.
    #[test]
    fn measured_plate_keeps_long_spans_within_combined_width_limit() {
        use ui::mod_hud::{MAX_CARD_SCALE, MAX_CARD_WIDTH, MAX_TEXT_BYTES, MIN_CARD_SCALE};

        for scale in [MIN_CARD_SCALE, 1., MAX_CARD_SCALE] {
            for brackets in [true, false] {
                for background_width in [None, Some(56.)] {
                    let mut hud = fixture();
                    let card = &mut hud.cards[0];
                    card.scale = scale;
                    card.text_scale = 2.;
                    card.rows[0].value = "W".repeat(MAX_TEXT_BYTES);
                    let style = &mut card.text_plate.as_mut().unwrap().style;
                    style.brackets = brackets;
                    style.width = background_width;
                    hud.validate().unwrap();
                    assert!(measure(&mut hud, &Metrics));

                    let card = &hud.cards[0];
                    let layout = card.text_plate.as_ref().unwrap().layout.as_ref().unwrap();
                    let limit = f64::from(MAX_CARD_WIDTH * scale);
                    let total_width = layout.spans.iter().map(|rect| rect[2]).sum::<f64>();
                    assert_eq!(total_width, limit, "spans must share the card width cap");
                    let mut right = layout.spans[0][0];
                    for [x, _, width, _] in layout.spans {
                        assert_eq!(x, right, "spans must remain adjacent");
                        assert!(width >= 0.);
                        right = x + width;
                        assert!(right <= layout.size[0], "span exceeds editor bounds");
                    }
                    assert_eq!(
                        layout.spans[0][0],
                        layout.size[0] - right,
                        "the combined text must stay centered"
                    );
                    let bracket_width = Metrics.label(
                        if brackets { "[" } else { "" },
                        None,
                        LabelShape {
                            scale: 0.8 * f64::from(scale) * f64::from(card.text_scale),
                            line_padding: 0.,
                            hide_hyphen: true,
                        },
                    )[0];
                    assert_eq!(layout.spans[0][2], bracket_width);
                    assert_eq!(layout.spans[2][2], bracket_width);
                }
            }
        }
    }

    /// The retained template draws independently colored literal spans without rebuilding on a tick.
    #[test]
    fn rendered_plate_keeps_span_colors_border_and_catalog_on_text_changes() {
        let mut hud = fixture();
        measure(&mut hud, &Metrics);
        let catalog = std::sync::Arc::new(super::super::template::catalog(&hud).unwrap());
        let mut screen = CachedScreen::default();
        let mut render = |hud: &Hud| {
            screen
                .render_shared_with(
                    "cinnabar_personal_hud.cards",
                    &catalog,
                    &Context::default(),
                    std::sync::Arc::new(super::super::template::data(hud, [400., 200.])),
                    ([400., 200.], 1., [0; 3]),
                    &LayoutEnv {
                        text: &Metrics,
                        textures: &Metrics,
                    },
                    &ViewState::default(),
                )
                .unwrap()
                .nodes
                .clone()
        };
        let nodes = render(&hud);
        for (word, color) in [
            ("[", [0, 255, 0, 128]),
            ("12:05 AM", [255, 0, 0, 128]),
            ("]", [0, 255, 0, 128]),
        ] {
            assert!(nodes.iter().any(|node|matches!(&node.draw,Draw::Text{text,color:actual,..} if text==word && *actual==color)),"missing tinted span {word}");
        }
        assert!(nodes.iter().any(|node|matches!(&node.draw,Draw::Custom{renderer,data} if renderer=="cinnabar_rounded_rectangle" && data.get("border_width").and_then(Value::as_f64)==Some(0.5))));
        let before = hud.clone();
        hud.cards[0].rows[0].value = "1:06 PM".into();
        measure(&mut hud, &Metrics);
        assert!(super::super::template::same_shape(&before, &hud));
        let nodes = render(&hud);
        assert!(
            nodes
                .iter()
                .any(|node| matches!(&node.draw,Draw::Text{text,..} if text=="1:06 PM"))
        );
    }
}
