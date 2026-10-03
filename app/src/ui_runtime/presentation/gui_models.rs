//! Native GUI geometry follows JSON-UI controls, not fixed-size baked thumbnails.

use std::{collections::BTreeMap, sync::Arc};

use assets::{ItemVisualDefinitionRoute, RuntimeAssets, RuntimeEntityAssets};
use render::UiTexturePage;
use ui::{UiMesh, UiNode, UiVisual};

use super::{IconRef, UiPresentationError, UiPresentationRuntime, item_gui, player_preview};

mod atlas;
mod held;
mod live_player;
#[cfg(test)]
mod tests;

/// Reserved dynamic page offsets: original skin, then bounded model-source atlases.
pub(super) const SKIN_PAGE: usize = render::UI_PLAYER_SKIN_PAGE_OFFSET;
pub(super) const MODEL_PAGE: usize = render::UI_MODEL_ATLAS_PAGE_OFFSET;
pub(super) const MODEL_PAGES: usize = render::MAX_UI_MODEL_ATLAS_PAGES;

pub(super) type IconKey = (u16, [u16; 4]);
pub(super) fn icon_key(icon: IconRef) -> IconKey {
    (icon.page, icon.uv)
}

#[derive(Default)]
pub(super) struct GuiModels {
    pub(super) enabled: bool,
    pub(super) pages: Vec<UiTexturePage>,
    pub(super) skin: Option<UiTexturePage>,
    live_player: live_player::LivePlayer,
    models: BTreeMap<IconKey, Arc<UiMesh>>,
    textures: BTreeMap<atlas::TextureKey, IconRef>,
    held: BTreeMap<assets::ItemVisualKey, player_preview::PreviewHeldModel>,
}

impl UiPresentationRuntime {
    /// Retains the original face and equipment texels; the GPU projects them at each control's
    /// physical size. Unsupported GUI tessellators retain their explicitly provisional thumbnail.
    pub(crate) fn set_gui_models(
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
        let ordinary = crate::presentation::equipment::blocks::collect(world, entities);
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
            self.equipment_catalog.as_deref(),
            &held_sources,
        )?;
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
        self.gui_models.textures = textures;
        self.gui_models.held = held;
        self.gui_models.enabled = true;
        self.rebuild_dynamic_textures();
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
        let player = self.gui_player_mesh();
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
            let mesh = if preview == Some(key) {
                player.as_ref()
            } else {
                self.gui_models
                    .models
                    .get(&key)
                    .or_else(|| self.session_icons.models.get(&key))
            };
            let Some(mesh) = mesh.and_then(|mesh| modulated(mesh, color, glint)) else {
                continue;
            };
            *node = node.clone().with_visual(UiVisual::Mesh(mesh));
        }
    }

    fn gui_player_mesh(&self) -> Option<Arc<UiMesh>> {
        let skin = self.gui_models.skin.as_ref()?;
        let [width, height] = skin.dimensions();
        let skin = IconRef {
            page: (self.textures.dynamic_start() + SKIN_PAGE) as u16,
            uv: [0, 0, width.try_into().ok()?, height.try_into().ok()?],
            glint: false,
        };
        let texture = |texture: &player_preview::PreviewTexture| {
            self.gui_models
                .textures
                .get(&atlas::key([texture.width, texture.height], &texture.rgba))
                .copied()
        };
        let armor = self
            .player_preview_gear
            .armor
            .each_ref()
            .map(|worn| worn.as_ref().and_then(texture));
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
        player_preview::geometry::mesh_with_body(
            (self.player_preview_view == player_preview::PreviewView::Hud)
                .then_some((
                    self.gui_models.live_player.vertices.as_slice(),
                    &self.gui_models.live_player.parts,
                ))
                .filter(|(vertices, _)| !vertices.is_empty()),
            self.player_preview_pose.unwrap_or_default(),
            self.player_preview_view,
            self.player_preview_bob,
            skin,
            &self.player_preview_gear,
            armor,
            held,
            true, // Current desktop default is the native Fancy material group.
        )
    }
}

/// Special translucent GUI tessellators (for example beacon) are not ordinary cubes.
pub(super) fn ordinary_cube_sheet(rgba8: &[u8]) -> bool {
    rgba8.chunks_exact(4).all(|pixel| pixel[3] == 255)
}

pub(super) fn sheet_faces(icon: IconRef) -> [IconRef; 6] {
    let side = assets::BLOCK_ITEM_FACE_SIDE;
    let columns = usize::from(assets::BLOCK_ITEM_SHEET_GRID[0]);
    std::array::from_fn(|face| {
        let left = icon.uv[0] + (face % columns) as u16 * side;
        let top = icon.uv[1] + (face / columns) as u16 * side;
        IconRef {
            uv: [left, top, left + side, top + side],
            ..icon
        }
    })
}

fn modulated(mesh: &Arc<UiMesh>, color: [u8; 4], glint: bool) -> Option<Arc<UiMesh>> {
    if color == [255; 4] && !glint {
        return Some(Arc::clone(mesh));
    }
    let mut vertices = mesh.vertices().to_vec();
    for vertex in &mut vertices {
        for (channel, factor) in vertex.color.iter_mut().zip(color) {
            *channel = (u16::from(*channel) * u16::from(factor) / 255) as u8;
        }
        if glint {
            vertex.style_flags |= ui::UI_STYLE_GLINT;
        }
    }
    UiMesh::new(
        vertices.into(),
        mesh.indices().into(),
        mesh.batches().into(),
    )
    .ok()
    .map(Arc::new)
}
