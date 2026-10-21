//! Canonical card output, prepared once and shared by gameplay and panel previews.

use super::{ModWidgets, template, text_plate};
use crate::{ModUi, PanelUiOwner, RowIcons};
use json_ui::{DataSource, DrawNode, Scalar, ViewState};
use std::sync::Arc;
use ui::{IconRef, UiNode, mod_hud::Hud};
use ui_engine::{
    cached_screen::CachedScreen,
    engine::{
        CachedFragment, CachedFragmentTextures, FormEngine, ScreenArt, item_renderer::ItemRenderer,
        pixel_snap, screen::PaintFrame,
    },
};

pub(crate) struct CardPaint {
    pub(super) source: Option<(PanelUiOwner, usize)>,
    reference: String,
    screen: CachedScreen,
    row_start: usize,
    data: Arc<DataSource>,
    revision: u64,
    icon_keys: Vec<String>,
    resolved: Vec<Option<IconRef>>,
    icons: Vec<IconRef>,
    laid: Option<Arc<[DrawNode]>>,
    pub(crate) nodes: Arc<[UiNode]>,
    pub(crate) textures: CachedFragmentTextures,
    pub(crate) bounds: Option<ui::UiRect>,
    pub(crate) px: f32,
    /// Moves gameplay-snapped artwork back to the preview's local card origin.
    pub(crate) preview_offset: [f32; 2],
    metrics: Option<[u64; 9]>,
    language: [usize; 3],
    prepared_revision: Option<u64>,
    origin: Option<[f64; 2]>,
    pub(super) gameplay: CachedFragment,
    #[cfg(any(test, feature = "test-support"))]
    pub(super) paints: usize,
}

impl CardPaint {
    /// Replaces only this card's bindings, preserving sibling cards' prepared output.
    pub(super) fn publish(&mut self, card: &ui::mod_hud::Card, index: usize) {
        let mut data = DataSource::new();
        template::card_data(&mut data, card, index, self.row_start, [0.; 2]);
        self.data = Arc::new(data);
        self.revision += 1;
        self.resolved.clear();
    }
}

/// Allocates cache slots and binding names only when the admitted card topology changes.
pub(super) fn entries(hud: &Hud) -> Vec<CardPaint> {
    let mut row_start = 0;
    hud.cards
        .iter()
        .enumerate()
        .map(|(index, card)| {
            let start = row_start;
            row_start += card.rows.len();
            let mut data = DataSource::new();
            template::card_data(&mut data, card, index, start, [0.; 2]);
            CardPaint {
                source: None,
                reference: format!("cinnabar_personal_hud.card_{index}"),
                screen: CachedScreen::default(),
                row_start: start,
                data: Arc::new(data),
                revision: 0,
                icon_keys: (start..row_start)
                    .map(|row| format!("#row_{row}_icon"))
                    .collect(),
                resolved: Vec::new(),
                icons: Vec::new(),
                laid: None,
                nodes: Arc::from([]),
                textures: CachedFragmentTextures::default(),
                bounds: None,
                px: 1.,
                preview_offset: [0.; 2],
                metrics: None,
                language: [0; 3],
                prepared_revision: None,
                origin: None,
                gameplay: CachedFragment::default(),
                #[cfg(any(test, feature = "test-support"))]
                paints: 0,
            }
        })
        .collect()
}

/// Uses a stable reference viewport with room for glyph and shadow overflow on every edge.
fn canvas_size(card: &ui::mod_hud::Card) -> [f64; 2] {
    template::dimensions(card).map(|axis| axis + 2. * template::CANVAS_INSET)
}

impl ModWidgets {
    /// Prepares one requested card. Unchanged layout and paint inputs reuse the same node Arc.
    pub(crate) fn prepare_card(
        &mut self,
        index: usize,
        engine: &FormEngine,
        frame: &mut PaintFrame<'_>,
        icon: &dyn Fn(usize, &str, u32) -> Option<IconRef>,
    ) -> Result<(), ui_engine::engine::screen::PaintError> {
        let card = &mut self.content.cards[index];
        let entry = &mut self.cards[index];
        let mut changed = entry.resolved.len() != card.rows.len();
        entry.resolved.resize(card.rows.len(), None);
        for (local, row) in card.rows.iter().enumerate() {
            let resolved = row
                .item
                .as_deref()
                .and_then(|id| icon(entry.row_start + local, id, row.metadata));
            changed |= entry.resolved[local] != resolved;
            entry.resolved[local] = resolved;
        }
        if changed {
            entry.revision += 1;
            entry.icons.clear();
            let data = Arc::make_mut(&mut entry.data);
            for (key, resolved) in entry.icon_keys.iter().zip(&entry.resolved) {
                let value = resolved.map_or(Scalar::Json(serde_json::Value::Null), |icon| {
                    let index = entry.icons.len();
                    entry.icons.push(icon);
                    Scalar::Num(index as f64)
                });
                data.set_global(key, value);
            }
        }
        let mut changed_textures = entry
            .laid
            .as_ref()
            .is_some_and(|laid| engine.refresh_fragment_textures(&mut entry.textures, laid));
        let px = frame.metrics.scale.get() * ui::FONT_DESIGN_PIXEL_TEXELS as f32;
        let viewport = frame.content.map(|axis| f64::from(axis / px));
        let mut origin = template::origin(card, viewport);
        let key = [
            frame.font as *const _ as usize as u64,
            u64::from(px.to_bits()),
            u64::from(frame.metrics.gui_scale.to_bits()),
            u64::from(frame.metrics.dpi_scale.get().to_bits()),
            u64::from(frame.metrics.line_height_64),
            u64::from(frame.metrics.baseline_64),
            u64::from(frame.solid_page),
            engine as *const _ as usize as u64,
            engine.textures.metadata_revision(),
        ];
        if !changed
            && !changed_textures
            && entry.origin == Some(origin)
            && entry.metrics == Some(key)
            && entry.language == frame.language
            && entry.prepared_revision == Some(entry.revision)
        {
            return Ok(());
        }
        if entry.metrics != Some(key) {
            entry.screen.invalidate_textures();
        }
        let mut items = ItemRenderer {
            icons: &entry.icons,
            id_aux: &[],
        };
        let mut renderers: [&mut dyn ui_engine::engine::screen::CustomRenderer; 1] = [&mut items];
        let mut target = frame.with_renderers(&mut renderers);
        let language = target.language;
        let laid = engine
            .prepare(ScreenArt::default(), &mut target, |env, _| {
                if text_plate::measure_card(card, env.text) {
                    entry.revision += 1;
                    text_plate::data(Arc::make_mut(&mut entry.data), card, index, self.viewport);
                }
                origin = template::origin(card, viewport);
                entry.screen.render_shared_with(
                    &entry.reference,
                    &self.catalog,
                    crate::personal_context(),
                    Arc::clone(&entry.data),
                    (canvas_size(card), px, language),
                    env,
                    &ViewState::default(),
                )
            })
            .map(|render| Arc::clone(&render.nodes));
        let Some(laid) = laid else {
            entry.nodes = Arc::from([]);
            return Ok(());
        };
        changed_textures |= engine.refresh_fragment_textures(&mut entry.textures, &laid);
        if !changed
            && !changed_textures
            && entry.origin == Some(origin)
            && entry.metrics == Some(key)
            && entry
                .laid
                .as_ref()
                .is_some_and(|old| Arc::ptr_eq(old, &laid))
        {
            entry.prepared_revision = Some(entry.revision);
            entry.language = language;
            return Ok(());
        }
        let mut nodes = Vec::new();
        let mut next = 0;
        let mut output = PaintFrame {
            nodes: &mut nodes,
            next: &mut next,
            layouts: &mut *target.layouts,
            font: target.font,
            metrics: target.metrics,
            solid_page: target.solid_page,
            safe_area: ui::SafeArea::ZERO,
            content: canvas_size(card).map(|axis| axis as f32 * px),
            translate: target.translate,
            language,
            now: target.now,
            renderers: target.renderers,
        };
        let art = ScreenArt {
            pixel_snap_offset: origin.map(|axis| axis - template::CANVAS_INSET),
            ..Default::default()
        };
        engine.draw(art, &mut output, |env, _| {
            entry.screen.render_shared_with(
                &entry.reference,
                &self.catalog,
                crate::personal_context(),
                Arc::clone(&entry.data),
                (canvas_size(card), px, language),
                env,
                &ViewState::default(),
            )
        })?;
        let snapped_origin =
            pixel_snap::positioned([origin[0], origin[1], 0., 0.], output.metrics.gui_scale, px);
        let content = canvas_size(card).map(|axis| axis as f32 * px);
        entry.bounds =
            ui_engine::engine::normalize_fragment(&mut nodes, [0., 0., content[0], content[1]])?
                .map(|bounds| bounds.translated([-template::CANVAS_INSET as f32 * px; 2]))
                .transpose()
                .map_err(ui_engine::engine::screen::PaintError::Geometry)?;
        entry.nodes = nodes.into();
        entry.px = px;
        entry.preview_offset =
            std::array::from_fn(|axis| origin[axis] as f32 * px - snapped_origin[axis]);
        entry.laid = Some(laid);
        entry.metrics = Some(key);
        entry.origin = Some(origin);
        entry.prepared_revision = Some(entry.revision);
        entry.language = language;
        #[cfg(any(test, feature = "test-support"))]
        {
            entry.paints += 1;
        }
        Ok(())
    }
}

impl ModUi {
    /// Paints admitted gameplay cards through the same canonical output used in panels.
    pub(super) fn paint_hud_cards(
        &mut self,
        engine: Option<&FormEngine>,
        frame: &mut PaintFrame<'_>,
        resolved: &RowIcons,
    ) {
        let Some(engine) = engine else {
            return;
        };
        let Some(widgets) = self.mod_widgets.as_mut() else {
            return;
        };
        let px = frame.metrics.scale.get() * ui::FONT_DESIGN_PIXEL_TEXELS as f32;
        let viewport = frame.content.map(|axis| f64::from(axis / px));
        if widgets.viewport != viewport {
            widgets.viewport = viewport;
            for (index, card) in widgets.content.cards.iter().enumerate() {
                text_plate::data(Arc::make_mut(&mut widgets.data), card, index, viewport);
            }
        }
        if !widgets.content.world_labels.is_empty() {
            super::world_labels::data(
                Arc::make_mut(&mut widgets.data),
                &widgets.content,
                viewport,
                widgets.projection,
            );
        }
        for index in 0..widgets.content.cards.len() {
            if !widgets.content.cards[index].gameplay_visible() {
                continue;
            }
            let merged_start = widgets.cards[index].row_start;
            let source_ref = widgets.cards[index].source;
            let (source, source_index) = if let Some((owner, source_index)) = source_ref {
                let Some(source) = self.hud_sources.get_mut(owner) else {
                    continue;
                };
                (source, source_index)
            } else {
                (&mut *widgets, index)
            };
            let source_start = source.cards[source_index].row_start;
            let result = source.prepare_card(source_index, engine, frame, &|row, _, _| {
                resolved
                    .as_slice()
                    .get(merged_start + row - source_start)
                    .copied()
                    .flatten()
            });
            if let Err(error) = result {
                tracing::warn!(%error, "HUD card preparation failed");
            }
        }
        let language = frame.language;
        let catalog = Arc::clone(&widgets.catalog);
        let data = Arc::clone(&widgets.data);
        // The gameplay screen contains only inert references; cards retain their own layouts.
        let mut screen = std::mem::take(&mut widgets.screen);
        let mut replay = Gameplay {
            widgets,
            sources: &mut self.hud_sources,
            viewport,
        };
        let mut renderers: [&mut dyn ui_engine::engine::screen::CustomRenderer; 1] = [&mut replay];
        let result = engine.draw(
            ScreenArt::default(),
            &mut frame.with_renderers(&mut renderers),
            |env, root| {
                screen.render_shared_with(
                    "cinnabar_personal_hud.gameplay",
                    &catalog,
                    crate::personal_context(),
                    data,
                    (root, px, language),
                    env,
                    &ViewState::default(),
                )
            },
        );
        replay.widgets.screen = screen;
        if let Err(error) = result {
            tracing::warn!(%error, "HUD card paint failed");
        }
    }
}

struct Gameplay<'a> {
    widgets: &'a mut ModWidgets,
    sources: &'a mut super::sources::HudSources,
    viewport: [f64; 2],
}

impl ui_engine::engine::screen::CustomRenderer for Gameplay<'_> {
    /// Places the shared canonical card at its current gameplay origin.
    fn paint(
        &mut self,
        node: &ui_engine::engine::screen::CustomDraw<'_>,
        canvas: &mut ui_engine::engine::Canvas<'_>,
    ) -> Result<bool, ui_engine::engine::screen::PaintError> {
        if node.renderer != "cinnabar_gameplay_card" {
            return Ok(false);
        }
        let Some(index) = node
            .data
            .get("card_index")
            .and_then(serde_json::Value::as_u64)
            .map(|n| n as usize)
        else {
            return Ok(true);
        };
        if !self.widgets.content.cards[index].gameplay_visible() {
            return Ok(true);
        }
        let source_ref = self.widgets.cards[index].source;
        let (source, index) = if let Some((owner, source_index)) = source_ref {
            let Some(source) = self.sources.get_mut(owner) else {
                return Ok(true);
            };
            (source, source_index)
        } else {
            (&mut *self.widgets, index)
        };
        let origin = template::origin(&source.content.cards[index], self.viewport)
            .map(|axis| axis as f32 * canvas.px());
        let entry = &mut source.cards[index];
        if !entry.textures.matches(canvas.textures()) {
            return Ok(true);
        }
        let Some(bounds) = entry.bounds else {
            return Ok(true);
        };
        let size = [bounds.width(), bounds.height()];
        let origin = [origin[0] + bounds.min().x(), origin[1] + bounds.min().y()];
        let dest = [
            origin[0],
            origin[1],
            origin[0] + size[0],
            origin[1] + size[1],
        ];
        canvas.paint_cached_fragment(
            &mut entry.gameplay,
            &entry.nodes,
            size,
            dest,
            canvas.screen(),
            1.,
        )?;
        Ok(true)
    }
    /// Keeps all visible cards' textures resident together before their cached artwork is replayed.
    fn textures<'s>(&'s self, _: &dyn Fn(&str) -> bool, texture: &mut dyn FnMut(&'s str)) {
        for (index, card) in self.widgets.content.cards.iter().enumerate() {
            if !card.gameplay_visible() {
                continue;
            }
            let entry = if let Some((owner, source_index)) = self.widgets.cards[index].source {
                let Some(source) = self.sources.get(owner) else {
                    continue;
                };
                &source.cards[source_index]
            } else {
                &self.widgets.cards[index]
            };
            for path in entry.textures.paths() {
                texture(path);
            }
        }
    }
}

#[cfg(test)]
mod tests;
