use json_ui::Catalog;
use serde_json::{Value, json};
use ui::mod_panel::{Control, Panel};

use super::{
    layout::{CARD_HEADER, Layout, NAV_HEIGHT},
    widgets::*,
};

pub(super) fn same_shape(a: &Panel, b: &Panel) -> bool {
    a.title == b.title
        && a.style == b.style
        && a.dark == b.dark
        && a.sections == b.sections
        && a.controls.len() == b.controls.len()
        && a.controls.iter().zip(&b.controls).all(|(a, b)| {
            a.id() == b.id()
                && a.label() == b.label()
                && match (a, b) {
                    (Control::Toggle { .. }, Control::Toggle { .. })
                    | (Control::Keybind { .. }, Control::Keybind { .. })
                    | (Control::Button { .. }, Control::Button { .. }) => true,
                    (
                        Control::Slider {
                            min: a_min,
                            max: a_max,
                            step: a_step,
                            ..
                        },
                        Control::Slider {
                            min: b_min,
                            max: b_max,
                            step: b_step,
                            ..
                        },
                    ) => (a_min, a_max, a_step) == (b_min, b_max, b_step),
                    (Control::Choice { options: a, .. }, Control::Choice { options: b, .. }) => {
                        a == b
                    }
                    _ => false,
                }
        })
}

pub(super) fn catalog(
    panel: &Panel,
    viewport: [f64; 2],
    category: usize,
    page: usize,
    rows: usize,
    editor: Option<&super::edit::Editor>,
) -> Result<(Catalog, usize), String> {
    if panel.style == ui::mod_panel::Style::Compact {
        return super::compact::catalog(panel, viewport, category, page, rows, editor);
    }
    let top = super::layout::top_offset(viewport);
    let layout = Layout::new(
        panel,
        [viewport[0], viewport[1] - top - 12.0],
        category,
        rows,
    );
    let page = page.min(layout.pages.len() - 1);
    let palette = Palette::new(panel.dark);
    let width = layout.width;
    let height = layout.height(page);
    let mut controls = vec![named(
        "navigation",
        navigation(panel, &layout, category, palette),
    )];
    for (card_index, card) in layout.pages[page].iter().enumerate() {
        let card_width = layout.card_width;
        let mut contents = chrome([card_width, card.height], palette, 8.0);
        let title_width = card_width - if card.toggle.is_some() { 42.0 } else { 16.0 };
        contents.push(named(
            "title",
            title(
                card.label,
                [title_width, 18.0],
                [8.0, 9.0],
                palette.text,
                false,
            ),
        ));
        if let Some(index) = card.toggle {
            contents.push(named("toggle", toggle(index, [card_width - 28.0, 9.0])));
        }
        if !card.controls.is_empty() {
            contents.push(named(
                "separator",
                rounded([card_width - 16.0, 0.5], [8.0, 26.0], 0.0, palette.border),
            ));
        }
        let mut y = CARD_HEADER;
        for index in &card.controls {
            let mut control_row = row(
                &panel.controls[*index],
                *index,
                card_width - 16.0,
                y,
                palette,
            );
            control_row["offset"][0] = json!(8.0);
            contents.push(named(&format!("row_{index}"), control_row));
            y += row_height(&panel.controls[*index]);
        }
        controls.push(named(
            &format!("card_{card_index}"),
            super::widgets::panel([card_width, card.height], card.offset, contents),
        ));
    }
    if layout.pages.len() > 1 {
        controls.extend(pagination(width, height, page, layout.pages.len(), palette));
    }
    let mut document = json!({"namespace":"cinnabar_personal","panel":{
        "type":"screen","size":["100%","100%"],"render_game_behind":true,"absorbs_input":true,"should_steal_mouse":false,
        "controls":[{"dialog":{"type":"panel","size":[width,height],"anchor_from":"top_left","anchor_to":"top_left","offset":[((viewport[0]-width)*0.5).round(),top],"controls":controls}}]
    }});
    super::edit::append_overlay(&mut document, panel, viewport, editor);
    let bytes = serde_json::to_vec(&document).map_err(|error| error.to_string())?;
    let catalog = Catalog::from_files([
        ("ui/_global_variables.json", &b"{}"[..]),
        (
            "ui/_ui_defs.json",
            &br#"{"ui_defs":["ui/cinnabar_personal.json"]}"#[..],
        ),
        ("ui/cinnabar_personal.json", &bytes),
    ])
    .map_err(|error| error.to_string())?;
    Ok((catalog, layout.pages.len()))
}

fn navigation(panel: &Panel, layout: &Layout<'_>, category: usize, palette: Palette) -> Value {
    let width = layout.width;
    let mut contents = chrome([width, NAV_HEIGHT], palette, 14.0);
    let title_width = if layout.categories.is_empty() {
        width - 46.0
    } else {
        width * 0.35
    };
    contents.push(named(
        "title",
        title(
            &panel.title,
            [title_width, 18.0],
            [12.0, 10.0],
            palette.text,
            false,
        ),
    ));
    let space = width - title_width - 42.0;
    let tab_width = space / layout.categories.len().max(1) as f64;
    for (index, name) in layout.categories.iter().enumerate() {
        let mut tab = Vec::new();
        if index == category {
            tab.push(named(
                "selected",
                rounded([tab_width - 2.0, 22.0], [0.0; 2], 11.0, palette.raised),
            ));
        }
        let mut text = label(
            name,
            [tab_width - 2.0, 18.0],
            [0.0, 7.0],
            if index == category {
                palette.text
            } else {
                palette.muted
            },
            false,
        );
        text["text_alignment"] = json!("center");
        tab.push(named("label", text));
        contents.push(named(
            &format!("category_{index}"),
            button(
                &format!("mod.category:{index}"),
                [tab_width - 2.0, 22.0],
                [title_width + 12.0 + index as f64 * tab_width, 3.0],
                tab,
            ),
        ));
    }
    let mut close = title("×", [20.0, 20.0], [0.0, 6.0], palette.muted, false);
    close["text_alignment"] = json!("center");
    contents.push(named(
        "close",
        button(
            "mod.close",
            [20.0, 22.0],
            [width - 28.0, 3.0],
            vec![named("label", close)],
        ),
    ));
    super::widgets::panel([width, NAV_HEIGHT], [0.0; 2], contents)
}

pub(super) fn pagination(
    width: f64,
    height: f64,
    page: usize,
    count: usize,
    palette: Palette,
) -> Vec<Value> {
    let y = height - 19.0;
    vec![
        named(
            "previous",
            button(
                "mod.prev",
                [24.0, 18.0],
                [width * 0.5 - 48.0, y],
                vec![named(
                    "label",
                    label("<", [24.0, 18.0], [0.0; 2], palette.text, false),
                )],
            ),
        ),
        named(
            "next",
            button(
                "mod.next",
                [24.0, 18.0],
                [width * 0.5 + 36.0, y],
                vec![named(
                    "label",
                    label(">", [24.0, 18.0], [0.0; 2], palette.text, false),
                )],
            ),
        ),
        named(
            "page",
            label(
                &format!("{} / {count}", page + 1),
                [58.0, 18.0],
                [width * 0.5 - 24.0, y],
                palette.muted,
                false,
            ),
        ),
    ]
}
