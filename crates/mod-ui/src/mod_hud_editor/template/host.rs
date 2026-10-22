//! Touch-sized editing chrome remains independent of gameplay artwork and GUI scale.
use super::*;

const HANDLE_WIDTH: f64 = 44.;
const ACTION_WIDTH: f64 = 48.;
const PAGE_WIDTH: f64 = 24.;
const TARGET_HEIGHT: f64 = 24.;
const HANDLE_PITCH: f64 = 26.;
pub(in super::super) const ACTIONS: &[(&str, &str)] = &[
    ("save", "Save"),
    ("cancel", "Cancel"),
    ("undo", "Undo"),
    ("redo", "Redo"),
    ("reset", "Reset"),
    ("grid", "#grid_label"),
    ("snap", "#snap_label"),
];

/// Chrome geometry also reserves its coverage from distance labels.
pub(in super::super) struct Layout {
    pub palette: [f64; 4],
    pub toolbar: [f64; 4],
    palette_columns: usize,
    pub page_size: usize,
    pub pages: usize,
    pub toolbar_page_size: usize,
    pub toolbar_pages: usize,
    pub usable: bool,
    toolbar_columns: usize,
    compact: bool,
}
impl Layout {
    /// Wraps labeled handles and actions without changing the editable canvas.
    pub fn new(viewport: [f64; 2], count: usize, scale: f64) -> Self {
        let mut compact = viewport[1] < 160. * scale;
        let mut palette_columns = (((viewport[0] - 8. * scale) / (HANDLE_WIDTH * scale)).floor()
            as usize)
            .max(1)
            .min(count.max(1));
        let palette_width = palette_columns as f64 * HANDLE_WIDTH * scale;
        let mut palette = [
            ((viewport[0] - palette_width) * 0.5).max(0.),
            20. + if compact { 20. } else { 34. } * scale,
            palette_width,
            count.div_ceil(palette_columns) as f64 * HANDLE_PITCH * scale,
        ];
        let mut toolbar_columns =
            if viewport[0] < (ACTIONS.len() as f64 * ACTION_WIDTH + 136.) * scale {
                4.min(ACTIONS.len())
            } else {
                ACTIONS.len()
            }
            .min((((viewport[0] - 8. * scale) / (ACTION_WIDTH * scale)).floor() as usize).max(1));
        let mut width = (toolbar_columns as f64 * ACTION_WIDTH + 8.) * scale;
        let mut height =
            (ACTIONS.len().div_ceil(toolbar_columns) as f64 * TARGET_HEIGHT + 4.) * scale;
        let mut toolbar_page_size = ACTIONS.len();
        let single_palette = TARGET_HEIGHT * scale;
        if single_palette + height + 12. * scale > viewport[1] {
            toolbar_columns = (((viewport[0] - (8. + 2. * PAGE_WIDTH) * scale)
                / (ACTION_WIDTH * scale))
                .floor() as usize)
                .max(1)
                .min(ACTIONS.len());
            toolbar_page_size = toolbar_columns;
            width = (toolbar_columns as f64 * ACTION_WIDTH + 2. * PAGE_WIDTH + 8.) * scale;
            height = (TARGET_HEIGHT + 4.) * scale;
        }
        let spare = (viewport[1] - 2. * TARGET_HEIGHT * scale).max(0.);
        let bottom = (4. * scale).min(spare * 0.25);
        let toolbar_padding = (4. * scale).min(spare * 0.25);
        if toolbar_page_size < ACTIONS.len() {
            height = TARGET_HEIGHT * scale + toolbar_padding;
        }
        let gap = (8. * scale).min(spare * 0.5);
        let toolbar_y = viewport[1] - height - bottom;
        let paged = palette[1] + palette[3] + gap > toolbar_y;
        let page_size = if paged {
            compact = true;
            palette_columns = (((viewport[0] - (8. + 2. * PAGE_WIDTH) * scale)
                / (HANDLE_WIDTH * scale))
                .floor() as usize)
                .max(1)
                .min(count.max(1));
            let width = (palette_columns as f64 * HANDLE_WIDTH + 2. * PAGE_WIDTH) * scale;
            let height = TARGET_HEIGHT * scale;
            palette = [
                ((viewport[0] - width) * 0.5).max(0.),
                (20. + 20. * scale).min(toolbar_y - height - gap).max(0.),
                width,
                height,
            ];
            palette_columns
        } else {
            count.max(1)
        };
        Self {
            palette,
            toolbar: [
                ((viewport[0] - width) * 0.5).max(0.),
                toolbar_y,
                width,
                height,
            ],
            palette_columns,
            page_size,
            pages: count.div_ceil(page_size).max(1),
            toolbar_columns,
            toolbar_page_size,
            toolbar_pages: ACTIONS.len().div_ceil(toolbar_page_size),
            usable: viewport[1] >= 2. * TARGET_HEIGHT * scale
                && palette[0] + palette[2] <= viewport[0]
                && width <= viewport[0]
                && palette[1] + palette[3] <= toolbar_y,
            compact,
        }
    }
}

/// Adds labeled drag handles and reversible actions with at least 48 logical-pixel targets.
pub(super) fn append(controls: &mut Vec<Value>, hud: &Hud, viewport: [f64; 2], scale: f64) {
    let mut style = hud.editor_style.unwrap_or_default();
    style.text_scale *= scale as f32;
    let layout = Layout::new(viewport, hud.cards.len(), scale);
    if !layout.usable {
        controls.push(named(
            "blocked_editor",
            button(
                "hud.blocked",
                viewport,
                [0.; 2],
                vec![
                    named("surface", rect(viewport, [0.; 2], [0.055, 0.065, 0.08, 1.])),
                    named(
                        "message",
                        label(
                            style,
                            "More screen space\nis needed to edit.",
                            [viewport[0], 24. * scale],
                            [0., 0.],
                        ),
                    ),
                ],
            ),
        ));
        controls.push(named(
            "blocked_cancel",
            button(
                "hud.cancel",
                [48. * scale, TARGET_HEIGHT * scale],
                [
                    ((viewport[0] - 48. * scale) * 0.5).max(0.),
                    (viewport[1] - TARGET_HEIGHT * scale).max(0.),
                ],
                vec![named(
                    "text",
                    label(
                        style,
                        "Cancel",
                        [48. * scale, 12. * scale],
                        [2. * scale, 6. * scale],
                    ),
                )],
            ),
        ));
        return;
    }
    let compact = layout.compact;
    let title_y = 20. + if compact { 4. } else { 6. } * scale;
    let mut title = label(
        style,
        "#hud_handles_title",
        [viewport[0], 16. * scale],
        [0., title_y],
    );
    title["text_alignment"] = json!("center");
    title["bindings"] = json!([{"binding_name":"#hud_handles_title"}]);
    if title_y + 16. * scale <= layout.palette[1] {
        controls.push(named("touch_title", title));
    }
    if !compact {
        let mut help = label(
            style,
            "Drag a control or its name. Guides align edges and centers.",
            [viewport[0], 12. * scale],
            [0., 20. + 20. * scale],
        );
        help["text_alignment"] = json!("center");
        controls.push(named("touch_help", help));
    }
    let [left, palette_y, _, palette_height] = layout.palette;
    let slot = HANDLE_WIDTH * scale;
    let columns = layout.palette_columns;
    for (index, card) in hud.cards.iter().enumerate() {
        let item = index % layout.page_size;
        let x =
            left + if layout.pages > 1 {
                PAGE_WIDTH * scale
            } else {
                0.
            } + (item % columns) as f64 * slot;
        let y = palette_y + (item / columns) as f64 * HANDLE_PITCH * scale;
        let mut text = label(
            style,
            card.editor_name(),
            [slot - 4. * scale, 12. * scale],
            [2. * scale, 6. * scale],
        );
        text["text_alignment"] = json!("center");
        let mut outline = rect(
            [slot - 2. * scale, scale],
            [scale, (TARGET_HEIGHT - 1.) * scale],
            [0.; 4],
        );
        outline["bindings"] = json!([{"binding_name":format!("#bounds_{index}_color"),"binding_name_override":"#color"}]);
        let mut handle = button(
            &format!("hud.card:{index}"),
            [slot, TARGET_HEIGHT * scale],
            [x, y],
            vec![
                named(
                    "surface",
                    rect(
                        [slot - 2. * scale, TARGET_HEIGHT * scale],
                        [scale, 0.],
                        [0.055, 0.065, 0.08, 0.97],
                    ),
                ),
                named("selection", outline),
                named("text", text),
            ],
        );
        handle["bindings"] = json!([{"binding_name":format!("#hud_handle_{index}_visible"),"binding_name_override":"#visible"}]);
        controls.push(named(&format!("move_{index}"), handle));
    }
    if layout.pages > 1 {
        for (name, text, x) in [
            ("prev", "Prev", left),
            (
                "next",
                "Next",
                left + layout.palette[2] - PAGE_WIDTH * scale,
            ),
        ] {
            let mut text = label(
                style,
                text,
                [(PAGE_WIDTH - 2.) * scale, 12. * scale],
                [scale, 6. * scale],
            );
            text["text_alignment"] = json!("center");
            let mut button = button(
                &format!("hud.page_{name}"),
                [PAGE_WIDTH * scale, TARGET_HEIGHT * scale],
                [x, palette_y],
                vec![
                    named(
                        "surface",
                        rect(
                            [PAGE_WIDTH * scale, TARGET_HEIGHT * scale],
                            [0.; 2],
                            [0.055, 0.065, 0.08, 0.97],
                        ),
                    ),
                    named("text", text),
                ],
            );
            button["bindings"] = json!([
                {"binding_name":format!("#hud_page_{name}"),"binding_name_override":"#enabled"},
                {"binding_name":format!("#hud_page_{name}_alpha"),"binding_name_override":"#alpha"}]);
            controls.push(named(&format!("page_{name}"), button));
        }
    }
    let [x, y, width, height] = layout.toolbar;
    let slot = ACTION_WIDTH * scale;
    let columns = layout.toolbar_columns;
    controls.push(named(
        "touch_toolbar_surface",
        button(
            "hud.blocked",
            [width, height],
            [x, y],
            vec![named(
                "surface",
                rect([width, height], [0.; 2], [0.055, 0.065, 0.08, 0.97]),
            )],
        ),
    ));
    let top_padding =
        (height - layout.toolbar_page_size.div_ceil(columns) as f64 * TARGET_HEIGHT * scale) * 0.5;
    for (index, (action, text)) in ACTIONS.iter().copied().enumerate() {
        let item = index % layout.toolbar_page_size;
        let mut textnode = label(
            style,
            text,
            [slot - 4. * scale, 12. * scale],
            [2. * scale, 6. * scale],
        );
        textnode["text_alignment"] = json!("center");
        if matches!(action, "grid" | "snap") {
            textnode["bindings"] = json!([{"binding_name":text}]);
        }
        let mut control = button(
            &format!("hud.{action}"),
            [slot, TARGET_HEIGHT * scale],
            [
                x + 4. * scale
                    + if layout.toolbar_pages > 1 {
                        PAGE_WIDTH * scale
                    } else {
                        0.
                    }
                    + (item % columns) as f64 * slot,
                y + top_padding + (item / columns) as f64 * TARGET_HEIGHT * scale,
            ],
            vec![named("text", textnode)],
        );
        control["bindings"] = json!([{"binding_name":format!("#hud_action_{index}_visible"),"binding_name_override":"#visible"}]);
        if matches!(action, "undo" | "redo") {
            control["bindings"].as_array_mut().unwrap().extend(
                json!([
                {"binding_name":format!("#hud_can_{action}"),"binding_name_override":"#enabled"},
                {"binding_name":format!("#hud_{action}_alpha"),"binding_name_override":"#alpha"}])
                .as_array()
                .unwrap()
                .iter()
                .cloned(),
            );
        }
        controls.push(named(&format!("touch_{action}"), control));
    }
    if layout.toolbar_pages > 1 {
        for (name, text, x) in [
            ("prev", "Prev", x + 4. * scale),
            ("next", "Next", x + width - (PAGE_WIDTH + 4.) * scale),
        ] {
            let mut text = label(
                style,
                text,
                [(PAGE_WIDTH - 2.) * scale, 12. * scale],
                [scale, 6. * scale],
            );
            text["text_alignment"] = json!("center");
            let mut control = button(
                &format!("hud.toolbar_{name}"),
                [PAGE_WIDTH * scale, TARGET_HEIGHT * scale],
                [x, y + (height - TARGET_HEIGHT * scale) * 0.5],
                vec![named("text", text)],
            );
            control["bindings"] = json!([
                {"binding_name":format!("#hud_toolbar_{name}"),"binding_name_override":"#enabled"},
                {"binding_name":format!("#hud_toolbar_{name}_alpha"),"binding_name_override":"#alpha"}]);
            controls.push(named(&format!("toolbar_{name}"), control));
        }
    }
    if palette_y + palette_height + 18. * scale <= y {
        let mut caption = label(
            style,
            "#editor_label",
            [viewport[0], 12. * scale],
            [0., y - 18. * scale],
        );
        caption["text_alignment"] = json!("center");
        caption["bindings"] = json!([{"binding_name":"#editor_label"}]);
        controls.push(named("touch_selection", caption));
    }
}
