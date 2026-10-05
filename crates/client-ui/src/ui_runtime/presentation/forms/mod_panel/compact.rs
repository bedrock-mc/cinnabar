use json_ui::Catalog;
use serde_json::{Value, json};
use ui::mod_panel::{Control, Icon, Panel};

use super::{icons, layout::Layout, widgets::*};

pub(super) fn row_height(control: &Control) -> f64 {
    match control {
        Control::Slider { .. } => 32.,
        Control::Keybind { .. } => 28.,
        _ => 26.,
    }
}

pub(super) fn catalog(
    spec: &Panel,
    viewport: [f64; 2],
    category: usize,
    page: usize,
    rows: usize,
) -> Result<(Catalog, usize), String> {
    let top = super::layout::top_offset(viewport);
    let layout = Layout::new(spec, [viewport[0], viewport[1] - top - 10.], category, rows);
    let page = page.min(layout.pages.len() - 1);
    let width = layout.width;
    let height = layout.height(page) + 10.;
    let palette = Palette::for_panel(spec);
    let mut outer = palette;
    if spec.dark {
        outer.card = [0.10, 0.13, 0.18, 0.95];
    }
    let mut controls = vec![named(
        "shadow",
        rounded(
            [width + 4., height + 4.],
            [-2., 2.],
            13.,
            [0., 0., 0., 0.18],
        ),
    )];
    controls.extend(chrome([width, height], outer, 12.));
    controls.push(named(
        "navigation",
        navigation(spec, &layout, category, palette),
    ));
    controls.push(named(
        "header_rule",
        rounded([width - 2., 0.5], [1., 35.5], 0., palette.border),
    ));
    for (card_index, card) in layout.pages[page].iter().enumerate() {
        let w = layout.card_width;
        let mut contents = chrome([w, card.height], palette, 8.);
        let has_icon = !card.flat && card.icon != Icon::None;
        if has_icon {
            contents.push(named(
                "icon",
                icons::mark(card.icon, 13., [10., 11.], palette.accent),
            ));
        }
        let title_x = if has_icon { 30. } else { 10. };
        if !card.flat {
            contents.push(named(
                "title",
                title(
                    card.label,
                    [
                        w - title_x - if card.toggle.is_some() { 32. } else { 10. },
                        17.,
                    ],
                    [title_x, 13.],
                    palette.text,
                    false,
                ),
            ));
        }
        if let Some(index) = card.toggle {
            contents.push(named("toggle", toggle(index, [w - 30., 13.])));
        }
        if !card.flat {
            contents.push(named(
                "header_rule",
                rounded([w - 20., 0.5], [10., 31.], 0., palette.border),
            ));
        }
        let mut y = if card.flat { 10. } else { 39. };
        let pin_key = card
            .controls
            .iter()
            .filter(|index| matches!(spec.controls[**index], Control::Keybind { .. }))
            .count()
            == 1;
        for index in &card.controls {
            let control = &spec.controls[*index];
            let keybind = matches!(control, Control::Keybind { .. });
            let position = if keybind && pin_key {
                card.height - 31.
            } else {
                y
            };
            let mut node = if keybind {
                keybind_row(*index, w - 20., position, palette, 28., true)
            } else if matches!(control, Control::Choice { .. }) {
                choice_row(*index, w - 20., position, palette)
            } else {
                row(control, *index, w - 20., position, palette)
            };
            node["offset"][0] = json!(10.);
            contents.push(named(&format!("row_{index}"), node));
            if !keybind || !pin_key {
                y += row_height(control);
            }
        }
        controls.push(named(
            &format!("card_{card_index}"),
            panel([w, card.height], card.offset, contents),
        ));
    }
    if layout.pages.len() > 1 {
        controls.extend(super::template::pagination(
            width,
            height,
            page,
            layout.pages.len(),
            palette,
        ));
    }
    let document = json!({"namespace":"cinnabar_personal","panel":{
        "type":"screen","size":["100%","100%"],"render_game_behind":true,"absorbs_input":true,"should_steal_mouse":false,
        "controls":[{"dialog":{"type":"panel","size":[width,height],"anchor_from":"top_left","anchor_to":"top_left",
            "offset":[((viewport[0]-width)*0.5).round(),top],"controls":controls}}]
    }});
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

fn choice_row(index: usize, width: f64, y: f64, palette: Palette) -> Value {
    let split = width * 0.37;
    let cap_width = width - split;
    let mut cap_palette = palette;
    cap_palette.card = palette.raised;
    let mut cap = chrome([cap_width, 22.], cap_palette, 4.);
    let mut value = label(
        &format!("#row_{index}_value"),
        [cap_width - 16., 14.],
        [4., 7.],
        palette.text,
        true,
    );
    value["font_scale_factor"] = json!(0.75);
    cap.push(named("value", value));
    cap.push(named(
        "chevron",
        icons::chevron([cap_width - 10., 10.], palette.muted),
    ));
    panel(
        [width, 26.],
        [0., y],
        vec![
            named(
                "label",
                label(
                    &format!("#row_{index}_label"),
                    [split - 3., 15.],
                    [0., 7.],
                    palette.muted,
                    true,
                ),
            ),
            named(
                "select",
                button(
                    &format!("mod.control:{index}"),
                    [cap_width, 22.],
                    [split, 0.],
                    cap,
                ),
            ),
        ],
    )
}

fn navigation(spec: &Panel, layout: &Layout<'_>, category: usize, palette: Palette) -> Value {
    let width = layout.width;
    let categories = layout.categories.len().max(1) as f64;
    let regular_title = (width * 0.34).min(150.);
    let narrow = (width - regular_title - 28.) / categories < 24.;
    let title_width = if narrow { 4. } else { regular_title };
    let mut nodes = Vec::new();
    if !narrow {
        nodes.push(named("brand", icons::brand(18., [12., 9.], palette.accent)));
    }
    if title_width > 85. {
        nodes.push(named(
            "title",
            title(
                &spec.title,
                [title_width - 38., 20.],
                [39., 12.],
                palette.text,
                false,
            ),
        ));
    }
    let tab_space = width - title_width - 28.;
    let tab_width = (tab_space / categories).min(100.);
    let tab_gap = if narrow { 2. } else { 8. };
    let hit_width = (tab_width - tab_gap).max(1.);
    for (index, name) in layout.categories.iter().enumerate() {
        let selected = index == category;
        let mut tab = Vec::new();
        if selected {
            tab.push(named(
                "outline",
                rounded(
                    [hit_width, 24.],
                    [0.; 2],
                    12.0_f64.min(hit_width * 0.5),
                    palette.accent,
                ),
            ));
            let fill = if spec.dark {
                [0.22, 0.12, 0.14, 0.98]
            } else {
                [1., 0.88, 0.86, 1.]
            };
            tab.push(named(
                "surface",
                rounded(
                    [(hit_width - 1.).max(0.5), 23.],
                    [0.5; 2],
                    11.5_f64.min((hit_width - 1.) * 0.5),
                    fill,
                ),
            ));
        }
        let color = if selected {
            palette.text
        } else {
            palette.muted
        };
        let icon = spec
            .sections
            .iter()
            .find(|section| section.category == *name)
            .map_or(Icon::None, |section| section.icon);
        let icon_size = if narrow {
            (hit_width - 4.).clamp(4., 13.)
        } else {
            13.
        };
        let icon_x = if tab_width > 65. {
            12.
        } else {
            (hit_width - icon_size) * 0.5
        };
        tab.push(named(
            "icon",
            if icon == Icon::Settings {
                icons::mark(
                    Icon::Settings,
                    icon_size.min(12.),
                    [icon_x, (24. - icon_size.min(12.)) * 0.5],
                    color,
                )
            } else {
                icons::sword(
                    icon_size,
                    [icon_x, (24. - icon_size) * 0.5],
                    if selected { palette.accent } else { color },
                )
            },
        ));
        if tab_width > 65. {
            tab.push(named(
                "label",
                label(name, [tab_width - 40., 16.], [32., 8.], color, false),
            ));
        }
        nodes.push(named(
            &format!("category_{index}"),
            button(
                &format!("mod.category:{index}"),
                [hit_width, 24.],
                [title_width + index as f64 * tab_width, 6.],
                tab,
            ),
        ));
    }
    nodes.push(named(
        "close",
        button(
            "mod.close",
            [20., 24.],
            [width - 27., 6.],
            vec![named("icon", icons::close(9., [5., 7.], palette.muted))],
        ),
    ));
    panel([width, 36.], [0.; 2], nodes)
}
