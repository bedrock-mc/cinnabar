//! Cosmetic playback shared by wardrobe tiles and menu player renderers.
use std::{collections::HashMap, sync::Arc};

use cosmetics::Item;
use render_model::{
    ActorVertex,
    hats::{self, Motion},
};
use ui::{IconRef, UiBlendMode, UiLimits, UiMesh, UiMeshBatch, UiMeshVertex, UiNode, UiVisual};

use super::super::UiPresentationRuntime;

#[derive(Default)]
pub(super) struct Previews {
    items: HashMap<Item, Preview>,
    active: Vec<(Item, Option<IconRef>)>,
    catalog: Option<(u64, Vec<Item>)>,
}

struct Preview {
    start: f64,
    motion: Motion,
    vertices: Arc<[ActorVertex]>,
    gallery: Option<(IconRef, Arc<UiMesh>)>,
    texture_key: String,
    thumbnail_key: String,
}

impl Previews {
    /// Returns the motion already sampled for this frame, or the item's initial preview pose.
    pub(super) fn motion(&self, item: Item) -> Motion {
        self.items.get(&item).map_or_else(
            || hats::rig(item).preview_motion(0.0),
            |preview| preview.motion,
        )
    }

    /// Uses the cached authored pose, retaining the rest mesh until the first preview frame.
    pub(super) fn vertices(&self, item: Item) -> &[ActorVertex] {
        self.items
            .get(&item)
            .map_or(&hats::rig(item).rest_vertices, |p| &p.vertices)
    }

    /// Advances active items only, retaining static geometry and unchanged gallery meshes.
    fn sync(&mut self, active: &[(Item, Option<IconRef>)], now: f64) {
        self.items
            .retain(|item, _| active.iter().any(|(active, _)| item == active));
        for &(item, icon) in active {
            let model = hats::rig(item);
            let preview = self.items.entry(item).or_insert_with(|| Preview {
                start: now,
                motion: model.preview_motion(0.0),
                vertices: Arc::clone(&model.rest_vertices),
                gallery: None,
                texture_key: item.texture_key(),
                thumbnail_key: item.thumbnail_key(),
            });
            let motion = model.preview_motion(now - preview.start);
            if preview.motion != motion {
                preview.vertices = model.preview_vertices(motion).into();
                preview.motion = motion;
                preview.gallery = None;
            }
            if let Some(icon) = icon {
                if preview.gallery.as_ref().is_none_or(|(old, _)| *old != icon) {
                    preview.gallery =
                        gallery_mesh(item, &preview.vertices, icon).map(|mesh| (icon, mesh));
                }
            } else {
                preview.gallery = None;
            }
        }
    }
}

impl UiPresentationRuntime {
    /// Advances character motion only when a painted menu renderer actually exposes its carrier.
    pub(in crate::ui_runtime::presentation) fn sync_visible_cosmetic_previews(
        &mut self,
        nodes: &[UiNode],
    ) {
        let player_shown = self.player_preview_icon.is_some_and(|icon| {
            nodes.iter().any(|node| {
                matches!(node.visual(), UiVisual::Sprite {texture_page, uv, ..}
                if (*texture_page, *uv) == (icon.page, icon.uv))
            })
        });
        self.sync_cosmetic_previews(
            player_shown
                && self.player_preview_view != super::super::player_preview::PreviewView::Hud,
        );
        self.sync_player_particles(player_shown);
    }

    /// Keeps equipped previews running outside a world and limits tile work to the visible gallery.
    pub(super) fn sync_cosmetic_previews(&mut self, player_shown: bool) {
        let mut active = std::mem::take(&mut self.gui_models.cosmetics.active);
        active.clear();
        active.extend(
            self.menu_preview_model
                .cosmetics
                .iter()
                .filter(|_| player_shown)
                .map(|&item| (item, None)),
        );
        let gallery = self.menu_view.as_ref().is_some_and(|view| {
            view.visible
                && view.screen == launcher::menu::MenuScreen::DressingRoom
                && view.dressing_room.section
                    == launcher::dressing_room::DressingRoomSection::Cosmetics
        });
        if gallery {
            let previews = &mut self.gui_models.cosmetics;
            let generation = Item::generation();
            if previews
                .catalog
                .as_ref()
                .is_none_or(|(old, _)| *old != generation)
            {
                previews.catalog = Some((generation, Item::all()));
            }
            let items = &previews.catalog.as_ref().expect("current catalog").1;
            for &index in &self.menu_cosmetic_thumbnail_indices {
                let Some(&item) = items.get(index) else {
                    continue;
                };
                let key = previews.items.get(&item).map_or_else(
                    || std::borrow::Cow::Owned(item.texture_key()),
                    |preview| std::borrow::Cow::Borrowed(preview.texture_key.as_str()),
                );
                let icon = self.menu_artwork.refs.get(key.as_ref()).copied();
                if let Some((_, old)) = active.iter_mut().find(|(active, _)| *active == item) {
                    *old = icon;
                } else {
                    active.push((item, icon));
                }
            }
        }
        self.gui_models.cosmetics.sync(&active, self.menu_seconds);
        self.gui_models.cosmetics.active = active;
    }

    /// Replaces visible thumbnail carriers without changing their bounds, clipping or selection marks.
    pub(super) fn apply_cosmetic_gallery_models(&self, nodes: &mut [UiNode]) {
        for index in 0..nodes.len() {
            let UiVisual::Sprite {
                texture_page,
                uv,
                color,
            } = nodes[index].visual()
            else {
                continue;
            };
            if color[3] < u8::MAX {
                continue;
            }
            let mesh = self
                .gui_models
                .cosmetics
                .items
                .values()
                .find_map(|preview| {
                    let (_, mesh) = preview.gallery.as_ref()?;
                    let icon = self.menu_artwork_icon(&preview.thumbnail_key)?;
                    ((*texture_page, *uv) == (icon.page, icon.uv)).then_some(mesh)
                });
            let Some(mesh) = mesh.and_then(|mesh| super::modulated(mesh, *color, false)) else {
                continue;
            };
            let budget = ui::UiTree::check_visual_budget_with_mesh(
                nodes
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| *i != index)
                    .map(|(_, node)| node.visual()),
                mesh.vertices().len(),
                mesh.batches().len(),
                1,
            );
            if budget.is_ok_and(|batches| batches <= UiLimits::MAX_DRAW_BATCHES) {
                nodes[index] = nodes[index].clone().with_visual(UiVisual::Mesh(mesh));
            }
        }
    }
}

/// Projects original texels and posed triangles using the worker-fitted gallery scale.
fn gallery_mesh(item: Item, source: &[ActorVertex], icon: IconRef) -> Option<Arc<UiMesh>> {
    let frame = hats::thumbnail::prepared_frame(item)?;
    let [left, top, right, bottom] = icon.uv.map(f32::from);
    let center = frame.center(source);
    let vertices: Vec<_> = source
        .iter()
        .map(|vertex| {
            let [width, height] = hats::thumbnail::SIZE;
            let [x, y, z] = frame.project(vertex.position, center, width, height);
            UiMeshVertex {
                position: [x, y],
                clip_z: z,
                clip_w: 1.0,
                uv: [
                    left + vertex.uv[0] * (right - left),
                    top + vertex.uv[1] * (bottom - top),
                ],
                color: [255; 4],
                model_light: 1.0,
                overlay_color: [0.0; 4],
                style_flags: 0,
                alpha_test: false,
            }
        })
        .collect();
    let end = u32::try_from(vertices.len()).ok()?;
    UiMesh::new(
        vertices.into(),
        (0..end).collect::<Vec<_>>().into(),
        Arc::from([UiMeshBatch {
            texture_page: icon.page,
            index_range: 0..end,
            blend: UiBlendMode::Alpha,
            depth_test: true,
            depth_write: true,
            alpha_cutoff: Some(0.5),
        }]),
    )
    .ok()
    .map(Arc::new)
}

#[cfg(test)]
mod tests;
