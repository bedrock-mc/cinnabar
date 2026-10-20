//! Guest-scoped HUD previews selected from the already virtualized panel layout.

use crate::{PanelUiOwner, mod_widgets::sources::HudSources};
use json_ui::{Draw, DrawNode, ViewState};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use ui::{IconRef, mod_panel::hud_card};
use ui_engine::engine::{
    CachedFragment, Canvas, FormEngine,
    screen::{CustomDraw, CustomRenderer, PaintError, PaintFrame},
};

#[derive(Default)]
pub(super) struct Previews {
    nodes: BTreeMap<String, Instance>,
    unknown: BTreeSet<String>,
    /// Distinct unknown selectors admitted to the bounded warning table.
    pub(super) unknown_ids: usize,
}

struct Instance {
    card: usize,
    live: bool,
    replay: CachedFragment,
}

impl Previews {
    /// Prepares only shown, intersecting preview nodes, after collection virtualization.
    pub(super) fn prepare(
        &mut self,
        nodes: &[DrawNode],
        view: &ViewState,
        owner: PanelUiOwner,
        sources: &mut HudSources,
        engine: &FormEngine,
        frame: &mut PaintFrame<'_>,
        icon: &dyn Fn(&str, u32) -> Option<IconRef>,
    ) {
        for instance in self.nodes.values_mut() {
            instance.live = false;
        }
        for node in nodes {
            if node.alpha <= 0.
                || !node.shown(view)
                || node.dest.w <= 0.
                || node.dest.h <= 0.
                || node.dest.x >= node.clip.x + node.clip.w
                || node.dest.y >= node.clip.y + node.clip.h
                || node.dest.x + node.dest.w <= node.clip.x
                || node.dest.y + node.dest.h <= node.clip.y
            {
                continue;
            }
            let Draw::Custom { renderer, data } = &node.draw else {
                continue;
            };
            if renderer != hud_card::RENDERER {
                continue;
            }
            let Some(id) = property(data, "card_id")
                .and_then(Value::as_str)
                .filter(|id| ui::mod_hud::valid_card_id(id))
            else {
                continue;
            };
            let Some(source) = sources.get_mut(owner) else {
                self.unknown(id);
                continue;
            };
            let Some(card) = source.card_index(id) else {
                self.unknown(id);
                continue;
            };
            if let Err(error) =
                source.prepare_card(card, engine, frame, &|_, id, metadata| icon(id, metadata))
            {
                tracing::warn!(%error, "HUD preview preparation failed");
                continue;
            }
            if !self.nodes.contains_key(node.key.as_str()) {
                self.nodes.insert(
                    node.key.clone(),
                    Instance {
                        card,
                        live: true,
                        replay: CachedFragment::default(),
                    },
                );
            }
            let instance = self
                .nodes
                .get_mut(node.key.as_str())
                .expect("preview instance installed");
            instance.card = card;
            instance.live = true;
        }
        self.nodes.retain(|_, instance| instance.live);
    }

    /// Warns once per local selector, then suppresses new warnings after the diagnostic budget.
    fn unknown(&mut self, id: &str) {
        if !self.unknown.contains(id)
            && self.unknown.len() < ui::mod_panel::MAX_SURFACE_NODES
            && self.unknown.insert(id.to_owned())
        {
            self.unknown_ids += 1;
            tracing::warn!(card_id = id, "unknown HUD preview card");
        }
    }

    /// Borrows completed cards without entering the engine's preparation or atlas locks.
    pub(super) fn renderer<'a>(
        &'a mut self,
        owner: PanelUiOwner,
        sources: &'a mut HudSources,
    ) -> Renderer<'a> {
        Renderer {
            previews: self,
            owner,
            sources,
        }
    }
}

/// Reads a live binding override before its authored fallback.
fn property<'a>(data: &'a BTreeMap<String, Value>, name: &str) -> Option<&'a Value> {
    // These names are fixed so the paint path never allocates a binding key.
    let bound = match name {
        "card_id" => "#card_id",
        "scale" => "#scale",
        "alignment" => "#alignment",
        _ => return None,
    };
    data.get(bound).or_else(|| data.get(name))
}

pub(super) struct Renderer<'a> {
    previews: &'a mut Previews,
    owner: PanelUiOwner,
    sources: &'a mut HudSources,
}

impl CustomRenderer for Renderer<'_> {
    /// Replays the owning guest's prepared artwork within this node's clip.
    fn paint(
        &mut self,
        node: &CustomDraw<'_>,
        canvas: &mut Canvas<'_>,
    ) -> Result<bool, PaintError> {
        if node.renderer != hud_card::RENDERER {
            return Ok(false);
        }
        let Some(instance) = self.previews.nodes.get_mut(node.key) else {
            return Ok(true);
        };
        let Some(source) = self.sources.get_mut(self.owner) else {
            return Ok(true);
        };
        let Some(entry) = source.cards.get(instance.card) else {
            return Ok(true);
        };
        let scale = property(node.data, "scale")
            .and_then(Value::as_f64)
            .unwrap_or(1.) as f32;
        if !scale.is_finite() || !(hud_card::MIN_SCALE..=hud_card::MAX_SCALE).contains(&scale) {
            return Ok(true);
        }
        let mut alignment = [0.5; 2];
        if let Some(value) = property(node.data, "alignment") {
            let Some(values) = value.as_array().filter(|values| values.len() == 2) else {
                return Ok(true);
            };
            for (axis, value) in alignment.iter_mut().zip(values) {
                let Some(value) = value
                    .as_f64()
                    .filter(|value| value.is_finite() && (0.0..=1.0).contains(value))
                else {
                    return Ok(true);
                };
                *axis = value as f32;
            }
        }
        let size = entry.size.map(|axis| axis / entry.px * canvas.px() * scale);
        let origin = [
            node.dest[0] + (node.dest[2] - node.dest[0] - size[0]) * alignment[0],
            node.dest[1] + (node.dest[3] - node.dest[1] - size[1]) * alignment[1],
        ];
        let dest = [
            origin[0],
            origin[1],
            origin[0] + size[0],
            origin[1] + size[1],
        ];
        canvas.paint_cached_fragment(
            &mut instance.replay,
            &entry.nodes,
            entry.size,
            dest,
            node.dest,
            f32::from(node.alpha([255; 4])[3]) / 255.,
        )?;
        Ok(true)
    }
}
