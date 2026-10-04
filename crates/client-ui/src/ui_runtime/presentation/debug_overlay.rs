//! The requested Java-style F3 developer overlay, based on the supplied Java
//! screenshot: two corner-aligned columns with separate translucent row strips.
//! This developer feature is independent of the vanilla Bedrock parity gates.

use std::sync::{Arc, OnceLock};

use json_ui::{DataSource, FormRender, LayoutEnv, ResolvedControl, Scalar};
use serde_json::{Value, json};
use ui::UiScale;

use super::bounded_visible_text;
use super::{FONT_DESIGN_PIXEL_TEXELS, TEXT_LINE_HEIGHT_64, TextMetrics, UiPresentationRuntime};

const STRIP_COLOR: [u8; 4] = [80, 80, 80, 144];
const INSET: f64 = 2.0;
const STRIP_PADDING: f64 = 1.0;
const TEXT_OFFSET_Y: f64 = 1.0;
const COLUMN_GAP: f64 = 8.0;
const MAX_LINES_PER_COLUMN: usize = 40;
/// Protect the position column when an unusually long GPU or driver name needs
/// truncation. Short right columns still give unused space back to the left.
const LEFT_COLUMN_SHARE: f64 = 0.6;
const LINE_HEIGHT: f64 = TEXT_LINE_HEIGHT_64 as f64 / 64.0 / FONT_DESIGN_PIXEL_TEXELS as f64;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DebugLines {
    pub left: Vec<String>,
    pub right: Vec<String>,
}

impl UiPresentationRuntime {
    pub fn set_debug_lines(&mut self, lines: Option<DebugLines>) {
        self.debug_lines = lines;
    }
}

/// Reserve the full row budget so changing values or target properties never
/// resize the text. Only viewport height, DPI and GUI scale affect font size;
/// horizontal overflow is truncated instead of shrinking the whole overlay.
pub(super) fn fitted_metrics(mut metrics: TextMetrics, content_height: f32) -> TextMetrics {
    let px = f64::from(metrics.scale.get() * FONT_DESIGN_PIXEL_TEXELS as f32);
    let height = (2.0 * INSET + LINE_HEIGHT * MAX_LINES_PER_COLUMN as f64) * px;
    let factor = (f64::from(content_height) / height).clamp(0.5, 1.0) as f32;
    metrics.scale = UiScale::new_display(metrics.scale.get() * factor).unwrap_or(metrics.scale);
    metrics
}

/// Bind and lay out the built-in JSON-UI screen using the same font measurer and
/// renderer as ordinary screens. Long rows keep a single line with ellipsis.
pub(super) fn render(lines: &DebugLines, root: [f64; 2], env: &LayoutEnv) -> FormRender {
    let row_limit = (((root[1] - 2.0 * INSET) / LINE_HEIGHT).max(0.0) + 1e-4).floor() as usize;
    let row_limit = row_limit.min(MAX_LINES_PER_COLUMN);
    let columns = [&lines.left, &lines.right];
    let widths: [Vec<f64>; 2] = columns.map(|column| {
        column
            .iter()
            .take(row_limit)
            .map(|line| {
                if line.is_empty() {
                    0.0
                } else {
                    env.text.extent(bounded_visible_text(line))[0] + 2.0 * STRIP_PADDING
                }
            })
            .collect()
    });
    let natural = widths
        .each_ref()
        .map(|column| column.iter().copied().fold(0.0, f64::max));
    let gap = if natural.iter().all(|width| *width > 0.0) {
        COLUMN_GAP
    } else {
        0.0
    };
    let available = (root[0] - 2.0 * INSET - gap).max(0.0);
    let mut limits = [natural[0].min(available * LEFT_COLUMN_SHARE), 0.0];
    limits[1] = natural[1].min(available - limits[0]);
    limits[0] = natural[0].min(available - limits[1]);
    let mut data = DataSource::new();
    data.set_strict(true);
    for (column_index, column) in columns.into_iter().enumerate() {
        for (index, line) in column.iter().take(row_limit).enumerate() {
            let width = widths[column_index][index].min(limits[column_index]);
            let name = format!("#debug_{column_index}_{index}");
            let visible = !line.is_empty() && width > 2.0 * STRIP_PADDING;
            data.set_global(format!("{name}_visible"), Scalar::Bool(visible));
            data.set_global(format!("{name}_width"), Scalar::Num(width));
            data.set_global(name, Scalar::Text(bounded_visible_text(line).to_owned()));
        }
    }
    let bound = json_ui::bind_shared(template(), &data, &json_ui::EmptyLibrary);
    json_ui::render_bound(bound, root, env, &Default::default())
}

/// Resolve once; changing diagnostics flow through bindings, with no direct
/// retained-node construction or interactive controls in this screen.
fn template() -> &'static Arc<ResolvedControl> {
    static TEMPLATE: OnceLock<Arc<ResolvedControl>> = OnceLock::new();
    TEMPLATE.get_or_init(|| {
        let controls: Vec<Value> = (0..2)
            .flat_map(|column| (0..MAX_LINES_PER_COLUMN).map(move |index| row(column, index)))
            .collect();
        let definition = json!({
            "namespace": "cinnabar_debug",
            "overlay": {
                "type": "panel", "size": ["100%", "100%"],
                "anchor_from": "top_left", "anchor_to": "top_left",
                "clips_children": true, "controls": controls
            }
        });
        let mut catalog = json_ui::Catalog::default();
        catalog.overlay_text("ui/cinnabar_debug.json", &definition.to_string());
        Arc::new(
            json_ui::resolve(
                &catalog,
                "cinnabar_debug.overlay",
                &json_ui::Context::empty(),
            )
            .control
            .expect("built-in F3 JSON-UI definition resolves"),
        )
    })
}

fn row(column: usize, index: usize) -> Value {
    let name = format!("debug_{column}_{index}");
    let binding = format!("#{name}");
    let right = column == 1;
    let anchor = if right { "top_right" } else { "top_left" };
    json!({
        (name): {
            "type": "panel", "size": [0, LINE_HEIGHT], "visible": false,
            "anchor_from": anchor, "anchor_to": anchor,
            "offset": [if right { -INSET } else { INSET }, INSET + index as f64 * LINE_HEIGHT],
            "clips_children": true,
            "bindings": [
                {"binding_name": format!("{binding}_visible"), "binding_name_override": "#visible"},
                {"binding_name": format!("{binding}_width"), "binding_name_override": "#size_binding_x_absolute"}
            ],
            "controls": [
                {"strip": {"type": "image", "size": ["100%", "100%"],
                    "color": STRIP_COLOR.map(|channel| f64::from(channel) / 255.0)}},
                {"line": {"type": "label", "size": [format!("100% - {}px", 2.0 * STRIP_PADDING), format!("100% - {TEXT_OFFSET_Y}px")],
                    "anchor_from": "top_left", "anchor_to": "top_left", "offset": [STRIP_PADDING, TEXT_OFFSET_Y],
                    "text": binding, "color": [1, 1, 1], "shadow": false,
                    "localize": false, "hide_hyphen": true,
                    "text_alignment": if right { "right" } else { "left" },
                    "bindings": [{"binding_name": binding}]}}
            ]
        }
    })
}
