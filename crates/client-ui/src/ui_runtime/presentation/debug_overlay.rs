//! The requested Java-style F3 developer overlay, based on the supplied Java
//! screenshot: two corner-aligned columns with separate translucent row strips.
//! This developer feature is independent of the vanilla Bedrock parity gates.

use std::sync::{Arc, OnceLock};

use assets::RuntimeFontCatalog;
use json_ui::{
    BindState, DataSource, EmptyLibrary, FormRender, LayoutEnv, MeasureCache, ResolvedControl,
    Scalar, ViewState,
};
use serde_json::{Value, json};
pub(super) mod paint;
pub(super) mod visibility;
pub(super) mod visible;
use paint::PaintedOverlay;

use super::bounded_visible_text;
use super::{FONT_DESIGN_PIXEL_TEXELS, TEXT_LINE_HEIGHT_64, UiPresentationRuntime};

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

/// The F3 screen's bindings and last layout, so changed lines rebind and re-lay out alone.
#[derive(Default)]
pub(super) struct OverlayCache {
    binding: BindState,
    measures: MeasureCache,
    laid: Option<LaidOverlay>,
    font: Option<Arc<RuntimeFontCatalog>>, // the font `measures` were taken with
    widths: [Vec<f64>; 2],
    pub(super) painted: PaintedOverlay,
    /// Bind+layout passes run, for cache tests.
    #[cfg(test)]
    pub(super) passes: usize,
    #[cfg(test)]
    pub(super) paints: usize,
}

impl OverlayCache {
    /// Drops cached measures and layout when the font is swapped, since glyph widths may differ.
    pub(super) fn retain_font(&mut self, font: &Arc<RuntimeFontCatalog>) {
        if !self
            .font
            .as_ref()
            .is_some_and(|kept| Arc::ptr_eq(kept, font))
        {
            *self = Self {
                font: Some(Arc::clone(font)),
                ..Self::default()
            };
        }
    }

    /// Whether the retained layout was bound from these displayed lines.
    pub(super) fn matches_lines(&self, lines: &DebugLines) -> bool {
        self.laid
            .as_ref()
            .is_some_and(|laid| visible::same_lines(&laid.lines, lines, laid.root))
    }

    #[cfg(test)]
    pub(super) fn into_render(self) -> Option<FormRender> {
        self.laid.map(|laid| laid.render)
    }
}

struct LaidOverlay {
    lines: DebugLines,
    root: [f64; 2],
    scale: f32,
    render: FormRender,
}

impl UiPresentationRuntime {
    pub fn set_debug_lines(&mut self, lines: Option<DebugLines>) {
        if self.debug_lines != lines {
            self.debug_lines = lines;
        }
    }

    /// Borrows the current diagnostic publication without copying its strings.
    pub fn debug_lines(&self) -> Option<&DebugLines> {
        self.debug_lines.as_ref()
    }

    /// Publishes changed diagnostics and returns the preceding buffers to the caller.
    pub fn swap_debug_lines(&mut self, lines: &mut Option<DebugLines>) -> bool {
        if self.debug_lines == *lines {
            return false;
        }
        std::mem::swap(&mut self.debug_lines, lines);
        true
    }
}

/// Bind and lay out the built-in JSON-UI screen using the same font measurer and
/// renderer as ordinary screens. Long rows keep a single line with ellipsis.
pub(super) fn render<'a>(
    cache: &'a mut OverlayCache,
    lines: &DebugLines,
    (root, scale): ([f64; 2], f32),
    env: &LayoutEnv,
) -> &'a FormRender {
    let same_frame = cache
        .laid
        .as_ref()
        .is_some_and(|laid| laid.root == root && laid.scale == scale);
    if same_frame
        && cache
            .laid
            .as_ref()
            .is_some_and(|laid| visible::same_lines(&laid.lines, lines, root))
    {
        return &cache.laid.as_ref().expect("checked above").render;
    }
    cache.painted.key = None;
    #[cfg(feature = "tracy")]
    let _span = bevy::log::info_span!("ui.f3.rebind_layout").entered();
    let previous = cache
        .laid
        .as_ref()
        .filter(|_| same_frame)
        .map(|laid| &laid.lines);
    let data = Arc::new(data_source(lines, root, env, previous, &mut cache.widths));
    let bound = match cache.laid.take() {
        Some(laid) => {
            let mut bound = laid.render.bound;
            if !same_frame {
                cache.measures = MeasureCache::default();
            }
            json_ui::rebind(
                template(),
                &data,
                &EmptyLibrary,
                &mut cache.binding,
                &mut bound,
                &mut cache.measures,
            );
            bound
        }
        None => {
            cache.measures = MeasureCache::default();
            json_ui::bind_incremental(template(), &data, &EmptyLibrary, &mut cache.binding)
        }
    };
    #[cfg(test)]
    {
        cache.passes += 1;
    }
    let render = {
        #[cfg(feature = "tracy")]
        let _span = bevy::log::info_span!("ui.f3.layout").entered();
        json_ui::render_bound_cached(bound, root, env, &ViewState::default(), &mut cache.measures)
    };
    &cache
        .laid
        .insert(LaidOverlay {
            lines: lines.clone(),
            root,
            scale,
            render,
        })
        .render
}

/// Row text, visibility and width bindings for every line that fits the root height.
fn data_source(
    lines: &DebugLines,
    root: [f64; 2],
    env: &LayoutEnv,
    previous: Option<&DebugLines>,
    widths: &mut [Vec<f64>; 2],
) -> DataSource {
    #[cfg(feature = "tracy")]
    let _span = bevy::log::info_span!("ui.f3.bindings_measure").entered();
    let row_limit = visible::row_limit(root);
    let columns = [&lines.left, &lines.right];
    for (column_index, column) in columns.into_iter().enumerate() {
        let old_column = previous.map(|lines| [&lines.left, &lines.right][column_index]);
        widths[column_index].resize(column.len().min(row_limit), 0.0);
        for (index, line) in column.iter().take(row_limit).enumerate() {
            if old_column
                .and_then(|old| old.get(index))
                .map(|line| bounded_visible_text(line))
                == Some(bounded_visible_text(line))
            {
                continue;
            }
            widths[column_index][index] = if line.is_empty() {
                0.0
            } else {
                env.text.extent(bounded_visible_text(line))[0] + 2.0 * STRIP_PADDING
            };
        }
    }
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
    data
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
