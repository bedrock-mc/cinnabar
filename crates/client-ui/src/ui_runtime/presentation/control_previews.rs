//! Isolated pack-control snapshots painted before their enclosing extension surface.

use std::{collections::BTreeMap, sync::Arc};

use json_ui::{Catalog, CollectionItem, Context, DataSource, Scalar};
use mod_ui::PanelUiOwner;
use serde_json::{Value, json};
use ui::{
    FONT_DESIGN_PIXEL_TEXELS, UiNode,
    mod_panel::preview::{ControlPreview, MAX_PREVIEW_EXTENT, RENDERER},
};
use ui_engine::{
    cached_screen::CachedScreen,
    engine::{
        Canvas, FormEngine, ScreenArt,
        screen::{CustomDraw, CustomRenderer, PaintError, PaintFrame},
    },
};

use super::UiPresentationRuntime;
use super::forms::preview_renderer::PreviewRenderer;
use ui::mod_panel::MAX_SURFACE_NODES;

#[derive(Default)]
pub(super) struct Previews {
    owner: Option<PanelUiOwner>,
    session: u64,
    base: Option<Arc<Catalog>>,
    font: Option<Arc<assets::RuntimeFontCatalog>>,
    source: BTreeMap<String, ControlPreview>,
    catalog: Option<Arc<Catalog>>,
    entries: BTreeMap<String, Entry>,
    source_px: f32,
}

struct Entry {
    reference: String,
    canvas: [f64; 2],
    data: Arc<DataSource>,
    screen: CachedScreen,
    nodes: Vec<UiNode>,
    size: Option<[f32; 2]>,
    context: Option<Context>,
    ready: bool,
    needs_player: bool,
}

impl UiPresentationRuntime {
    /// Whether a currently painted preview resolves to the shared player-model renderer.
    pub fn control_preview_needs_player(&self, session: u64) -> bool {
        let Some((owner, declarations)) = self.screens.mod_ui.control_previews() else {
            return false;
        };
        self.control_previews.session == session
            && self.control_previews.owner == Some(owner)
            && self.control_previews.source == *declarations
            && self
                .control_previews
                .entries
                .values()
                .any(|entry| entry.needs_player)
    }
    /// Last completed preview measurements, scoped to this exact active UI session.
    pub fn mod_control_preview_sizes(
        &self,
        owner: PanelUiOwner,
        session: u64,
    ) -> BTreeMap<String, [f32; 2]> {
        let Some((active, declarations)) = self.screens.mod_ui.control_previews() else {
            return BTreeMap::new();
        };
        if active != owner
            || self.control_previews.owner != Some(owner)
            || self.control_previews.session != session
            || self.control_previews.source != *declarations
        {
            return BTreeMap::new();
        }
        let unit = self.control_previews.source_px / FONT_DESIGN_PIXEL_TEXELS as f32;
        self.control_previews
            .entries
            .iter()
            .filter_map(|(id, entry)| {
                entry
                    .size
                    .map(|size| (id.clone(), size.map(|value| value * unit)))
            })
            .collect()
    }
}

impl Previews {
    /// Legacy surfaces keep the direct panel paint path when no previews are declared.
    pub(super) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    /// One immutable derived catalog per owner/source, never a write to the engine's catalog.
    pub(super) fn sync(
        &mut self,
        selected: Option<(PanelUiOwner, &BTreeMap<String, ControlPreview>)>,
        session: u64,
        base: &Arc<Catalog>,
        font: &Arc<assets::RuntimeFontCatalog>,
    ) {
        let Some((owner, source)) = selected.filter(|(_, source)| !source.is_empty()) else {
            *self = Self::default();
            return;
        };
        if self.owner == Some(owner)
            && self.session == session
            && self.source == *source
            && self
                .base
                .as_ref()
                .is_some_and(|previous| Arc::ptr_eq(previous, base))
        {
            if !self.font.as_ref().is_some_and(|old| Arc::ptr_eq(old, font)) {
                self.font = Some(Arc::clone(font));
                for entry in self.entries.values_mut() {
                    entry.screen.invalidate_textures();
                }
            }
            return;
        }
        self.owner = Some(owner);
        self.session = session;
        self.source = source.clone();
        self.base = Some(Arc::clone(base));
        self.font = Some(Arc::clone(font));
        self.entries.clear();
        let mut document =
            serde_json::Map::from_iter([("namespace".into(), json!("cinnabar_control_previews"))]);
        for (index, (id, spec)) in source.iter().enumerate() {
            let name = format!("preview_{index}");
            let canvas = spec.size.map(|size| if size > 0.0 { size } else { 1024.0 });
            let mut body: serde_json::Map<String, Value> =
                spec.variables.clone().into_iter().collect();
            body.insert("anchor_from".into(), json!("top_left"));
            body.insert("anchor_to".into(), json!("top_left"));
            body.insert("offset".into(), json!([0, 0]));
            document.insert(
                name.clone(),
                json!({"type":"panel","size":canvas,
                "anchor_from":"top_left","anchor_to":"top_left","offset":[MAX_PREVIEW_EXTENT,MAX_PREVIEW_EXTENT],
                "controls":[{format!("content@{}",spec.reference):Value::Object(body)}]}),
            );
            let mut data = DataSource::new();
            for (name, value) in &spec.globals {
                data.set_global(name, scalar(value));
            }
            for (name, rows) in &spec.collections {
                data.set_collection(
                    name,
                    rows.iter()
                        .map(|row| {
                            row.iter()
                                .fold(CollectionItem::default(), |item, (name, value)| {
                                    item.with(name.as_str(), scalar(value))
                                })
                        })
                        .collect(),
                );
            }
            self.entries.insert(
                id.clone(),
                Entry {
                    reference: format!("cinnabar_control_previews.{name}"),
                    canvas,
                    data: Arc::new(data),
                    screen: CachedScreen::default(),
                    nodes: Vec::new(),
                    size: None,
                    context: None,
                    ready: false,
                    needs_player: false,
                },
            );
        }
        let mut catalog = (**base).clone().with_paint_transforms();
        let text = Value::Object(document).to_string();
        catalog.apply_pack([
            (
                "ui/_ui_defs.json",
                &br#"{"ui_defs":["ui/cinnabar_control_previews.json"]}"#[..],
            ),
            ("ui/cinnabar_control_previews.json", text.as_bytes()),
        ]);
        self.catalog = Some(Arc::new(catalog));
    }

    /// Paints with the existing screen renderers before the outer engine holds atlas locks.
    pub(super) fn paint(
        &mut self,
        engine: &FormEngine,
        context: &Context,
        frame: &mut PaintFrame<'_>,
    ) {
        let Some(catalog) = self.catalog.as_ref() else {
            return;
        };
        self.source_px = frame.metrics.scale.get() * FONT_DESIGN_PIXEL_TEXELS as f32;
        for entry in self.entries.values_mut() {
            if entry.context.as_ref() != Some(context) {
                let resolution = json_ui::resolve_with_limits(
                    catalog,
                    &entry.reference,
                    context,
                    32,
                    MAX_SURFACE_NODES,
                );
                entry.context = Some(context.clone());
                entry.ready = false;
                entry.needs_player = false;
                let Some(tree) = resolution
                    .control
                    .filter(|_| resolution.diagnostics.is_empty())
                else {
                    entry.nodes.clear();
                    entry.size = None;
                    continue;
                };
                entry.screen = CachedScreen::from_resolved(
                    &entry.reference,
                    Arc::clone(catalog),
                    context.clone(),
                    Arc::new(tree),
                )
                .with_node_limit(MAX_SURFACE_NODES);
                entry.ready = true;
            }
            if !entry.ready {
                continue;
            }
            let mut nodes = std::mem::take(&mut entry.nodes);
            nodes.clear();
            let mut next = 1;
            let px = frame.metrics.scale.get() * FONT_DESIGN_PIXEL_TEXELS as f32;
            let content = entry
                .canvas
                .map(|size| (size + 2.0 * MAX_PREVIEW_EXTENT) as f32 * px);
            let mut renderers: Vec<&mut dyn CustomRenderer> = frame
                .renderers
                .iter_mut()
                .map(|renderer| &mut **renderer as &mut dyn CustomRenderer)
                .collect();
            let mut target = PaintFrame {
                layouts: &mut *frame.layouts,
                font: frame.font,
                metrics: frame.metrics,
                solid_page: frame.solid_page,
                safe_area: ui::SafeArea::ZERO,
                content,
                translate: frame.translate,
                language: frame.language,
                now: frame.now,
                nodes: &mut nodes,
                next: &mut next,
                renderers: &mut renderers,
            };
            let result = engine.draw(
                ScreenArt {
                    nearest_sampling: true,
                    ..Default::default()
                },
                &mut target,
                |env, root| {
                    entry
                        .screen
                        .render(
                            &entry.reference,
                            catalog,
                            context,
                            Arc::clone(&entry.data),
                            (root, px, frame.language),
                            env,
                        )
                        .filter(|render| {
                            render.nodes.len() <= MAX_SURFACE_NODES
                                && render.hits.len() <= MAX_SURFACE_NODES
                        })
                },
            );
            entry.size = result.ok().flatten().and_then(|_| {
                ui_engine::engine::normalize_fragment(
                    &mut nodes,
                    [0.0, 0.0, content[0], content[1]],
                )
                .ok()
                .flatten()
                .map(|size| size.map(|value| value / px))
            });
            entry.nodes = nodes;
            entry.needs_player = entry.screen.nodes().iter().any(|node|matches!(&node.draw,json_ui::Draw::Custom {renderer,..} if PreviewRenderer::handles(renderer)));
        }
    }

    /// A renderer over completed inert snapshots; it neither resolves nor binds while painting.
    pub(super) fn renderer(&self) -> Renderer<'_> {
        Renderer {
            entries: &self.entries,
            px: self.source_px,
        }
    }
}

/// Preserves the binder's scalar kinds for expressions and collection values.
fn scalar(value: &Value) -> Scalar {
    match value {
        Value::Bool(value) => Scalar::Bool(*value),
        Value::Number(value) => Scalar::Num(value.as_f64().unwrap_or_default()),
        Value::String(value) => Scalar::Text(value.clone()),
        _ => Scalar::Json(value.clone()),
    }
}

pub(super) struct Renderer<'a> {
    entries: &'a BTreeMap<String, Entry>,
    px: f32,
}

impl CustomRenderer for Renderer<'_> {
    fn paint(
        &mut self,
        node: &CustomDraw<'_>,
        canvas: &mut Canvas<'_>,
    ) -> Result<bool, PaintError> {
        if node.renderer != RENDERER {
            return Ok(false);
        }
        if let Some(entry) = node
            .data
            .get("preview_id")
            .and_then(Value::as_str)
            .and_then(|id| self.entries.get(id))
            && let Some(size) = entry.size
        {
            canvas.paint_fragment(
                &entry.nodes,
                size.map(|value| value * self.px),
                node.dest,
                f32::from(node.alpha([255; 4])[3]) / 255.0,
            )?;
        }
        Ok(true)
    }
}

#[cfg(test)]
mod tests;
