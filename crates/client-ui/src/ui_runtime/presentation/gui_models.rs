//! Native GUI geometry follows JSON-UI controls, not fixed-size baked thumbnails.

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use assets::{ItemVisualDefinitionRoute, RuntimeAssets, RuntimeEntityAssets};
use render_model::UiTexturePage;
use ui::{UiMesh, UiNode, UiVisual};

use super::{IconRef, UiPresentationError, UiPresentationRuntime, item_gui, player_preview};

mod atlas;
mod block_models;
mod fire;
mod held;
mod live_player;
mod pack_equipment;
mod sources;
use sources::modulated;
pub(super) use sources::{ordinary_cube_sheet, sheet_faces};
#[cfg(any(test, feature = "test-support"))]
pub mod test_support;
#[cfg(test)]
mod tests;

/// Reserved dynamic page offsets: original skin, then bounded model-source atlases.
pub(super) const SKIN_PAGE: usize = render_model::UI_PLAYER_SKIN_PAGE_OFFSET;
pub(super) const MODEL_PAGE: usize = render_model::UI_MODEL_ATLAS_PAGE_OFFSET;
pub(super) const MODEL_PAGES: usize = render_model::MAX_UI_MODEL_ATLAS_PAGES;

/// Swaps sprites that have native models for their geometry. The player preview mesh is built
/// at most once, and only when a node shows the preview icon. Translucent optional models
/// retain their original sprite so faded controls keep a single layer.
fn replace_model_icons<'a>(
    nodes: &mut [UiNode],
    preview: Option<IconKey>,
    optional_models: &BTreeSet<IconKey>,
    player: impl FnOnce() -> Option<Arc<UiMesh>>,
    models: impl Fn(&IconKey) -> Option<&'a Arc<UiMesh>>,
) {
    let mut build_player = Some(player);
    let mut player = None;
    for node in nodes {
        let (key, color, glint) = match node.visual() {
            UiVisual::Sprite {
                texture_page,
                uv,
                color,
            } => ((*texture_page, *uv), *color, false),
            UiVisual::GlintSprite {
                texture_page,
                uv,
                color,
            } => ((*texture_page, *uv), *color, true),
            _ => continue,
        };
        if color[3] < u8::MAX && optional_models.contains(&key) {
            continue;
        }
        let mesh = if preview == Some(key) {
            if let Some(build) = build_player.take() {
                player = build();
            }
            player.as_ref()
        } else {
            models(&key)
        };
        let Some(mesh) = mesh.and_then(|mesh| modulated(mesh, color, glint)) else {
            continue;
        };
        *node = node.clone().with_visual(UiVisual::Mesh(mesh));
    }
}

pub(super) type IconKey = (u16, [u16; 4]);
pub(super) fn icon_key(icon: IconRef) -> IconKey {
    (icon.page, icon.uv)
}

#[derive(Default)]
pub(super) struct GuiModels {
    pub(super) enabled: bool,
    pub(super) pages: Vec<UiTexturePage>,
    pub(super) skin: Option<UiTexturePage>,
    pub(super) pack_equipment: pack_equipment::PackEquipment,
    live_player: live_player::LivePlayer,
    models: BTreeMap<IconKey, Arc<UiMesh>>,
    optional_models: BTreeSet<IconKey>,
    required_pages: usize,
    textures: BTreeMap<atlas::TextureKey, IconRef>,
    held: BTreeMap<assets::ItemVisualKey, player_preview::PreviewHeldModel>,
    fire: fire::FireAtlas,
}

impl GuiModels {
    /// Drops optional block meshes and restores required pages. Returns whether anything was dropped;
    /// callers reinstall fire frames after a discard.
    fn discard_optional_models(&mut self, first: usize) -> bool {
        if self.optional_models.is_empty() {
            return false;
        }
        self.pages.truncate(self.required_pages);
        for key in std::mem::take(&mut self.optional_models) {
            self.models.remove(&key);
        }
        self.textures.retain(|_, icon| {
            let page = usize::from(icon.page);
            page < first + self.required_pages || page >= first + MODEL_PAGES
        });
        self.fire.frames.clear();
        self.fire.pages_start = None;
        true
    }
}

impl UiPresentationRuntime {
    /// Retains the original face and equipment texels; the GPU projects them at each control's
    /// physical size. Unsupported GUI tessellators retain their explicitly provisional thumbnail.
    pub fn set_gui_models(
        &mut self,
        world: &RuntimeAssets,
        entities: &RuntimeEntityAssets,
    ) -> Result<(), UiPresentationError> {
        let Some(icons) = self.icon_catalog.as_deref() else {
            return Ok(());
        };
        if icons.source_manifest_sha256() != entities.source_manifest_sha256()
            || (!world.is_diagnostic()
                && world.provenance().source_manifest_sha256 != entities.source_manifest_sha256())
        {
            return Err(UiPresentationError::InvalidFontTexture);
        }
        let first = (self.textures.dynamic_start() + MODEL_PAGE) as u16;
        let mut atlas = atlas::Atlas::new(first, MODEL_PAGES);
        let mut sources = BTreeMap::new();
        let ordinary = render_model::equipment::blocks::collect(world, entities);
        for (visual, index) in ordinary.by_visual {
            sources.insert(visual, ordinary.sheets[index].clone());
        }
        // Native carried masks/colors (not the biome-tinted world faces) override ordinary cubes.
        for sheet in icons.block_sheets() {
            sources.insert(
                sheet.visual.0,
                icons.sprites()[sheet.sprite as usize].clone(),
            );
        }
        let mut meshes = BTreeMap::new();
        let mut held_sources = BTreeMap::new();
        for (visual, sprite) in &sources {
            if !ordinary_cube_sheet(&sprite.rgba8) {
                continue;
            }
            let icon = atlas.insert([sprite.width, sprite.height], &sprite.rgba8)?;
            held_sources.insert(*visual, icon);
            if let Some(mesh) = item_gui::cube(sheet_faces(icon)) {
                meshes.insert(*visual, mesh);
            }
        }
        let mut models = BTreeMap::new();
        for entry in icons.entries() {
            let Some(definition) = entities.item_visuals().iter().find(|visual| {
                visual.key.identifier == entry.identifier && visual.key.metadata == entry.metadata
            }) else {
                continue;
            };
            let ItemVisualDefinitionRoute::BlockItem { block_visual } = definition.route else {
                continue;
            };
            let Some(mesh) = meshes.get(&block_visual.0) else {
                continue;
            };
            let Some(icon) = self
                .icon_refs
                .as_deref()
                .and_then(|refs| refs.get(entry.sprite as usize))
            else {
                continue;
            };
            models
                .entry(icon_key(*icon))
                .or_insert_with(|| Arc::clone(mesh));
        }
        if let Some(equipment) = self.equipment_catalog.as_deref() {
            for texture in equipment.textures() {
                atlas.insert([texture.width, texture.height], &texture.rgba8)?;
            }
            if let Some(binding) = equipment.binding(item_gui::SHIELD_IDENTIFIER)
                && let Some(geometry) = entities
                    .geometries()
                    .iter()
                    .find(|geometry| geometry.identifier == binding.geometry.identifier)
                && let Some(texture) = equipment.texture(&binding.texture.identifier)
                && let Some(icon) = self.item_icon(item_gui::SHIELD_IDENTIFIER, 0)
            {
                let source = atlas.insert([texture.width, texture.height], &texture.rgba8)?;
                if let Some(mesh) = item_gui::shield(geometry, texture, source) {
                    models.insert(icon_key(icon), mesh);
                }
            }
        }
        let held = held::prepare(
            &mut atlas,
            entities,
            icons,
            self.icon_refs
                .as_deref()
                .ok_or(UiPresentationError::InvalidFontTexture)?,
            self.equipment_catalog.as_deref(),
            &held_sources,
        )?;
        let required_pages = atlas.page_count();
        let mut optional_models = BTreeSet::new();
        if let Some(refs) = self.icon_refs.as_deref() {
            for thumbnail in icons.block_models() {
                if let Some(icon) = refs.get(thumbnail.sprite as usize)
                    && !models.contains_key(&icon_key(*icon))
                    && let Some(mesh) = block_models::mesh(world, thumbnail.visual, &mut atlas)
                {
                    models.insert(icon_key(*icon), mesh);
                    optional_models.insert(icon_key(*icon));
                }
            }
        }
        let (pages, mut textures) = atlas.finish()?;
        if let Some(refs) = self.icon_refs.as_deref() {
            for (sprite, icon) in icons.sprites().iter().zip(refs) {
                textures
                    .entry(atlas::key([sprite.width, sprite.height], &sprite.rgba8))
                    .or_insert(*icon);
            }
        }
        bevy::log::info!(
            models = models.len(),
            source_pages = pages.len(),
            "native GUI item geometry prepared"
        );
        self.gui_models.pages = pages;
        self.gui_models.models = models;
        self.gui_models.optional_models = optional_models;
        self.gui_models.required_pages = required_pages;
        self.gui_models.textures = textures;
        self.gui_models.held = held;
        self.gui_models.fire.pages_start = None;
        self.install_gui_fire()?;
        self.gui_models.enabled = true;
        self.place_pack_armor();
        Ok(())
    }

    /// Reuses the retained page's digest when its pixels have not changed.
    pub(super) fn set_gui_skin(&mut self, skin: &[u8]) -> Option<[u8; 32]> {
        if !self.gui_models.enabled {
            return None;
        }
        let side = (skin.len() / 4).isqrt();
        if side == 0 || side * side * 4 != skin.len() {
            return None;
        }
        if let Some(page) = self
            .gui_models
            .skin
            .as_ref()
            .filter(|page| page.pixels() == skin)
        {
            return Some(page.identity());
        }
        let page = UiTexturePage::owned([side as u32; 2], Arc::from(skin)).ok()?;
        let identity = page.identity();
        self.gui_models.skin = Some(page);
        self.preview_dirty = true;
        Some(identity)
    }

    /// Conversion happens after JSON-UI has resolved positions, visibility and clipping. The
    /// replacement keeps node identity, parents, bounds and ordering intact, including the cursor.
    pub(super) fn apply_gui_models(&self, nodes: &mut [UiNode]) {
        if !self.gui_models.enabled {
            return;
        }
        let preview = self.player_preview_icon.map(icon_key);
        replace_model_icons(
            nodes,
            preview,
            &self.gui_models.optional_models,
            || self.gui_player_mesh(),
            |key| {
                self.gui_models
                    .models
                    .get(key)
                    .or_else(|| self.session_icons.models.get(key))
            },
        );
    }

    fn gui_player_mesh(&self) -> Option<Arc<UiMesh>> {
        let skin = self.gui_models.skin.as_ref()?;
        let [width, height] = skin.dimensions();
        let skin = IconRef {
            page: (self.textures.dynamic_start() + SKIN_PAGE) as u16,
            uv: [0, 0, width.try_into().ok()?, height.try_into().ok()?],
            glint: false,
        };
        let armor = std::array::from_fn(|slot| {
            let texture = self.player_preview_gear.armor[slot].as_ref()?;
            self.gui_models.pack_equipment.region(slot).or_else(|| {
                let key = atlas::key([texture.width, texture.height], &texture.rgba);
                self.gui_models.textures.get(&key).copied()
            })
        });
        let held = self.player_preview_gear.hands.each_ref().map(|hand| {
            let hand = hand.as_ref()?;
            let (identifier, metadata) = Self::item_icon_key(
                &hand.identifier,
                hand.metadata,
                hand.charged_projectile.as_deref(),
                None,
            );
            self.gui_models
                .held
                .get(&assets::ItemVisualKey {
                    identifier: identifier.into(),
                    metadata,
                })
                .or_else(|| {
                    self.gui_models.held.get(&assets::ItemVisualKey {
                        identifier: identifier.into(),
                        metadata: 0,
                    })
                })
        });
        let unposed = [None; 6];
        let body = if self.player_preview_view == player_preview::PreviewView::Hud {
            (self.player_preview_view == player_preview::PreviewView::Hud)
                .then_some((
                    self.gui_models.live_player.vertices.as_slice(),
                    &self.gui_models.live_player.parts,
                ))
                .filter(|(vertices, _)| !vertices.is_empty())
        } else {
            self.menu_preview_model
                .vertices
                .as_deref()
                .map(|vertices| (vertices, &unposed))
        };
        let cape = self.menu_preview_model.cape_key.as_deref().and_then(|key| {
            self.menu_artwork_icon(key)
                .map(|icon| (player_preview::cape::rest_vertices(), icon))
        });
        player_preview::geometry::mesh_with_cape(
            body,
            self.player_preview_pose.unwrap_or_default(),
            self.player_preview_view,
            self.player_preview_bob,
            skin,
            &self.player_preview_gear,
            armor,
            held,
            true, // Current desktop default is the native Fancy material group.
            self.gui_fire_frame(),
            if self.player_preview_view == player_preview::PreviewView::Hud {
                self.gui_models.live_player.overlay_color
            } else {
                [0.0; 4]
            },
            cape,
        )
    }
}
