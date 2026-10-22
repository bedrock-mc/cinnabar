//! Touch-sized editing chrome remains independent of gameplay artwork and GUI scale.
use super::*;

const HANDLE_WIDTH: f64 = 44.;
const ACTION_WIDTH: f64 = 48.;
const TARGET_HEIGHT: f64 = 24.;
const HANDLE_PITCH: f64 = 26.;
const ACTIONS: &[(&str, &str)] = &[
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
    toolbar_columns: usize,
    compact: bool,
}
impl Layout {
    /// Wraps labeled handles and actions without changing the editable canvas.
    pub fn new(viewport: [f64; 2], count: usize, scale: f64) -> Self {
        let compact = viewport[1] < 160. * scale;
        let palette_columns = (((viewport[0] - 8. * scale) / (HANDLE_WIDTH * scale)).floor()
            as usize)
            .max(1)
            .min(count.max(1));
        let palette_width = palette_columns as f64 * HANDLE_WIDTH * scale;
        let palette = [
            ((viewport[0] - palette_width) * 0.5).max(0.),
            20. + if compact { 20. } else { 34. } * scale,
            palette_width,
            count.div_ceil(palette_columns) as f64 * HANDLE_PITCH * scale,
        ];
        let toolbar_columns = if viewport[0] < (ACTIONS.len() as f64 * ACTION_WIDTH + 136.) * scale
        {
            4.min(ACTIONS.len())
        } else {
            ACTIONS.len()
        }
        .min((((viewport[0] - 8. * scale) / (ACTION_WIDTH * scale)).floor() as usize).max(1));
        let width = (toolbar_columns as f64 * ACTION_WIDTH + 8.) * scale;
        let height = (ACTIONS.len().div_ceil(toolbar_columns) as f64 * TARGET_HEIGHT + 4.) * scale;
        Self {
            palette,
            toolbar: [
                ((viewport[0] - width) * 0.5).max(0.),
                viewport[1] - height - 4. * scale,
                width,
                height,
            ],
            palette_columns,
            toolbar_columns,
            compact,
        }
    }
}

/// Adds labeled drag handles and reversible actions with at least 48 logical-pixel targets.
pub(super) fn append(controls: &mut Vec<Value>, hud: &Hud, viewport: [f64; 2], scale: f64) {
    let mut style = hud.editor_style.unwrap_or_default();
    style.text_scale *= scale as f32;
    let layout = Layout::new(viewport, hud.cards.len(), scale);
    let compact = layout.compact;
    let title_y = 20. + if compact { 4. } else { 6. } * scale;
    let mut title = label(
        style,
        "Touch controls",
        [viewport[0], 16. * scale],
        [0., title_y],
    );
    title["text_alignment"] = json!("center");
    controls.push(named("touch_title", title));
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
        let x = left + (index % columns) as f64 * slot;
        let y = palette_y + (index / columns) as f64 * HANDLE_PITCH * scale;
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
        handle["bindings"] = json!([{"binding_name":format!("#hud_card_{index}_visible"),"binding_name_override":"#visible"}]);
        controls.push(named(&format!("move_{index}"), handle));
    }
    let [x, y, width, height] = layout.toolbar;
    let slot = ACTION_WIDTH * scale;
    let columns = layout.toolbar_columns;
    controls.push(named(
        "touch_toolbar_surface",
        rect([width, height], [x, y], [0.055, 0.065, 0.08, 0.97]),
    ));
    for (index, (action, text)) in ACTIONS.iter().copied().enumerate() {
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
                x + 4. * scale + (index % columns) as f64 * slot,
                y + (2. + (index / columns) as f64 * TARGET_HEIGHT) * scale,
            ],
            vec![named("text", textnode)],
        );
        if matches!(action, "undo" | "redo") {
            control["bindings"] = json!([
                {"binding_name":format!("#hud_can_{action}"),"binding_name_override":"#enabled"},
                {"binding_name":format!("#hud_{action}_alpha"),"binding_name_override":"#alpha"}]);
        }
        controls.push(named(&format!("touch_{action}"), control));
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
