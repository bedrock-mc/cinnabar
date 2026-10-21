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
    fit: FitCache,
}

#[derive(Clone, Copy, PartialEq)]
struct FitKey {
    ink: [f32; 2],
    node: [f32; 2],
    ui_scale: f32,
    fraction: f32,
    max_scale: f32,
}

#[derive(Default)]
struct FitCache {
    value: Option<(FitKey, f32)>,
    #[cfg(test)]
    computations: usize,
}

impl FitCache {
    /// Fits painted dimensions in both axes; unchanged sizes and parameters reuse the multiplier.
    fn scale(&mut self, key: FitKey) -> f32 {
        if let Some((previous, scale)) = self.value
            && previous == key
        {
            return scale;
        }
        let scale = ((key.node[0] / key.ink[0]).min(key.node[1] / key.ink[1]) * key.fraction
            / key.ui_scale)
            .min(key.max_scale);
        self.value = Some((key, scale));
        #[cfg(test)]
        {
            self.computations += 1;
        }
        scale
    }
}

impl Previews {
    /// Prepares only shown, intersecting preview nodes, after collection virtualization.
    #[expect(
        clippy::too_many_arguments,
        reason = "preparation borrows the selected owner, engine, frame and item atlas"
    )]
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
            // The emitter already culls against transformed clips and preserves moving nodes.
            if (node.alpha <= 0. && node.anim.is_none()) || !node.shown(view) {
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
                        fit: FitCache::default(),
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
        "fit" => "#fit",
        "max_scale" => "#max_scale",
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
        if !entry.textures.matches(canvas.textures()) {
            return Ok(true);
        }
        let scale = property(node.data, "scale")
            .and_then(Value::as_f64)
            .unwrap_or(1.) as f32;
        if !scale.is_finite() || !(hud_card::MIN_SCALE..=hud_card::MAX_SCALE).contains(&scale) {
            return Ok(true);
        }
        let fit = match property(node.data, "fit") {
            None => None,
            Some(value) => {
                let Some(fit) = value.as_f64().filter(|fit| *fit > 0. && *fit <= 1.) else {
                    return Ok(true);
                };
                Some(fit as f32)
            }
        };
        let max_scale = match property(node.data, "max_scale") {
            None => hud_card::MAX_SCALE,
            Some(value) => {
                let Some(scale) = value.as_f64().filter(|scale| {
                    (f64::from(hud_card::MIN_SCALE)..=f64::from(hud_card::MAX_SCALE))
                        .contains(scale)
                }) else {
                    return Ok(true);
                };
                scale as f32
            }
        };
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
        let Some(bounds) = entry.bounds else {
            return Ok(true);
        };
        let source_size = [bounds.width(), bounds.height()];
        let node_size = [node.dest[2] - node.dest[0], node.dest[3] - node.dest[1]];
        let scale = fit.map_or(scale, |fraction| {
            instance.fit.scale(FitKey {
                ink: source_size,
                node: node_size,
                ui_scale: canvas.px() / entry.px,
                fraction,
                max_scale,
            })
        });
        let ratio = canvas.px() / entry.px * scale;
        let size = source_size.map(|axis| axis * ratio);
        let origin: [f32; 2] = if fit.is_some() {
            std::array::from_fn(|axis| {
                node.dest[axis] + (node_size[axis] - size[axis]) * alignment[axis]
            })
        } else {
            let geometry =
                crate::mod_widgets::template::dimensions(&source.content.cards[instance.card])
                    .map(|axis| axis as f32 * canvas.px() * scale);
            [
                node.dest[0]
                    + (node_size[0] - geometry[0]) * alignment[0]
                    + (bounds.min().x() + entry.preview_offset[0]) * ratio,
                node.dest[1]
                    + (node_size[1] - geometry[1]) * alignment[1]
                    + (bounds.min().y() + entry.preview_offset[1]) * ratio,
            ]
        };
        let dest = [
            origin[0],
            origin[1],
            origin[0] + size[0],
            origin[1] + size[1],
        ];
        canvas.paint_cached_fragment(
            &mut instance.replay,
            &entry.nodes,
            source_size,
            dest,
            node.dest,
            f32::from(node.alpha([255; 4])[3]) / 255.,
        )?;
        Ok(true)
    }
    /// Pins every visible preview's texture requirements before replay, including shared cards.
    fn textures<'s>(&'s self, _: &dyn Fn(&str) -> bool, texture: &mut dyn FnMut(&'s str)) {
        let Some(source) = self.sources.get(self.owner) else {
            return;
        };
        for instance in self.previews.nodes.values() {
            if let Some(entry) = source.cards.get(instance.card) {
                for path in entry.textures.paths() {
                    texture(path);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
