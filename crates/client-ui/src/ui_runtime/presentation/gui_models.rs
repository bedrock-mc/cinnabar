//! Native GUI geometry follows JSON-UI controls, not fixed-size baked thumbnails.

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use assets::{ItemVisualDefinitionRoute, RuntimeAssets, RuntimeEntityAssets};
use render_model::UiTexturePage;
use ui::{
    RetainedDraw, SafeArea, TextEffects, UiLimits, UiMesh, UiNode, UiRect, UiScale, UiVisual,
};

use {
    super::{UiPresentationError, UiPresentationRuntime, item_gui, player_preview},
    ui::IconRef,
};

mod atlas;
mod block_models;
mod cosmetic_particles;
mod cosmetic_previews;
mod fire;
mod held;
mod live_player;
mod pack_equipment;
mod sources;
use sources::{ModulatedMeshes, gui_cube_sheet, modulated};
pub(super) use sources::{ordinary_cube_sheet, sheet_faces};
#[cfg(test)]
mod cube_budget;
#[cfg(any(test, feature = "test-support"))]
pub mod test_support;
#[cfg(test)]
mod tests;

/// Reserved dynamic page offsets: original skin, then bounded model-source atlases.
pub(super) const SKIN_PAGE: usize = render_model::UI_PLAYER_SKIN_PAGE_OFFSET;
pub(super) const MODEL_PAGE: usize = render_model::UI_MODEL_ATLAS_PAGE_OFFSET;
pub(super) const MODEL_PAGES: usize = render_model::MAX_UI_MODEL_ATLAS_PAGES;

/// Swaps sprites that have native models for their geometry. The player preview mesh is built
/// at most once, and only when a node `preview` targets shows the preview icon. Models with
/// overlapping faces retain their original sprite when faded so the control's opacity applies
/// once. `modulate` tints a mesh as [`modulated`] does.
fn replace_model_icons<'a>(
    nodes: &mut [UiNode],
    preview: Option<&PreviewTargets>,
    faded_thumbnails: &BTreeSet<IconKey>,
    player: impl FnOnce() -> Option<Arc<UiMesh>>,
    models: impl Fn(&IconKey) -> Option<&'a Arc<UiMesh>>,
    mut modulate: impl FnMut(&Arc<UiMesh>, [u8; 4], bool) -> Option<Arc<UiMesh>>,
) {
    let mut build_player = Some(player);
    let mut player = None;
    for (index, node) in nodes.iter_mut().enumerate() {
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
        if color[3] < u8::MAX && faded_thumbnails.contains(&key) {
            continue;
        }
        let mesh = if preview.is_some_and(|preview| preview.matches(index, key)) {
            if let Some(build) = build_player.take() {
                player = build();
            }
            player.as_ref()
        } else {
            models(&key)
        };
        let Some(mesh) = mesh.and_then(|mesh| modulate(mesh, color, glint)) else {
            continue;
        };
        *node = node.clone().with_visual(UiVisual::Mesh(mesh));
    }
}

pub(super) type IconKey = (u16, [u16; 4]);

/// The player-preview placeholders one mesh replaces: the HUD's own doll, or every other
/// screen's. The two pose the model differently, so neither may draw the other's mesh.
#[derive(Clone, Debug)]
struct PreviewTargets {
    key: IconKey,
    /// Frame node indices the HUD painted.
    hud: std::ops::Range<usize>,
    in_hud: bool,
}

impl PreviewTargets {
    /// Whether the node at `index` showing `key` is one of these placeholders.
    fn matches(&self, index: usize, key: IconKey) -> bool {
        key == self.key && self.hud.contains(&index) == self.in_hud
    }

    /// Whether the node at `index` still shows one of these placeholders.
    fn shows(&self, index: usize, visual: &UiVisual) -> bool {
        match visual {
            UiVisual::Sprite {
                texture_page, uv, ..
            }
            | UiVisual::GlintSprite {
                texture_page, uv, ..
            } => self.matches(index, (*texture_page, *uv)),
            _ => false,
        }
    }
}

pub(super) fn icon_key(icon: IconRef) -> IconKey {
    (icon.page, icon.uv)
}

#[derive(Default)]
pub(super) struct GuiModels {
    pub(super) enabled: bool,
    pub(super) sources: PreparedGuiModels,
    pub(super) skin: Option<UiTexturePage>,
    pub(super) pack_equipment: pack_equipment::PackEquipment,
    live_player: live_player::LivePlayer,
    cosmetics: cosmetic_previews::Previews,
    pub(super) particles: cosmetic_particles::PreviewParticles,
    pub(super) wearing_elytra: bool,
    selected_held: [Option<held::SelectedModel>; 2],
    fire: fire::FireAtlas,
    /// Tinted copies of `models`, kept while frames draw them.
    modulated: std::sync::Mutex<ModulatedMeshes>,
}

/// Static item meshes and atlas pages prepared off the menu frame.
#[derive(Default)]
pub struct PreparedGuiModels {
    pub(super) color_masks: super::color_icons::MaskRefs,
    pub(super) pages: Vec<UiTexturePage>,
    pub(super) cape: Option<render_model::cape::CapeRig>,
    pub(super) models: BTreeMap<IconKey, Arc<UiMesh>>,
    pub(super) optional_models: BTreeSet<IconKey>,
    pub(super) faded_thumbnails: BTreeSet<IconKey>,
    pub(super) required_pages: usize,
    pub(super) textures: BTreeMap<atlas::TextureKey, IconRef>,
    held: BTreeMap<assets::ItemVisualKey, held::HeldModel>,
    pub(super) hand_pivots: Option<[[f32; 3]; 2]>,
}

impl GuiModels {
    /// Clears selected hand models when their equipment catalog changes.
    pub(super) fn clear_selected_hands(&mut self) {
        self.selected_held = [None, None];
    }

    /// Drops optional block meshes and restores required pages. Returns whether anything was dropped;
    /// callers reinstall fire frames after a discard.
    fn discard_optional_models(&mut self, first: usize) -> bool {
        if self.sources.optional_models.is_empty() {
            return false;
        }
        self.sources.pages.truncate(self.sources.required_pages);
        for key in std::mem::take(&mut self.sources.optional_models) {
            self.sources.models.remove(&key);
            self.sources.faded_thumbnails.remove(&key);
        }
        self.sources.textures.retain(|_, icon| {
            let page = usize::from(icon.page);
            page < first + self.sources.required_pages || page >= first + MODEL_PAGES
        });
        self.fire.frames.clear();
        self.fire.pages_start = None;
        true
    }
}

/// A sheet can become a cube only when an existing carried model spans the full cube.
/// Shortened models keep the shaped-model path even when their source texels are transparent.
fn cube_shape_admitted(
    ordinary: &render_model::equipment::blocks::BlockSheets,
    visual: u32,
) -> bool {
    !ordinary.models.get(&visual).is_some_and(|vertices| {
        vertices.iter().any(|vertex| {
            vertex
                .position
                .iter()
                .any(|coordinate| coordinate.abs() != 0.5)
        })
    })
}

/// Immutable UI addresses used to prepare item geometry on a worker.
pub struct GuiModelPreparation {
    icons: Option<Arc<assets::RuntimeIconCatalog>>,
    icon_refs: Option<Box<[IconRef]>>,
    first: u16,
    shield: Option<IconRef>,
}

impl GuiModelPreparation {
    /// Builds static item sources against the menu's existing atlas addresses.
    pub fn prepare(
        self,
        world: &RuntimeAssets,
        entities: &RuntimeEntityAssets,
        equipment: Option<&assets::RuntimeEquipmentCatalog>,
    ) -> Result<PreparedGuiModels, UiPresentationError> {
        let Some(icons) = self.icons.as_deref() else {
            return Ok(PreparedGuiModels::default());
        };
        if icons.source_manifest_sha256() != entities.source_manifest_sha256()
            || (!world.is_diagnostic()
                && world.provenance().source_manifest_sha256 != entities.source_manifest_sha256())
        {
            return Err(UiPresentationError::InvalidFontTexture);
        }
        let first = self.first;
        let mut atlas = atlas::Atlas::new(first, MODEL_PAGES);
        let mut sources = BTreeMap::new();
        let ordinary = render_model::equipment::blocks::collect(world, entities);
        for (&visual, &index) in &ordinary.by_visual {
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
            if !cube_shape_admitted(&ordinary, *visual) || !ordinary_cube_sheet(&sprite.rgba8) {
                continue;
            }
            let icon = atlas.insert([sprite.width, sprite.height], &sprite.rgba8)?;
            held_sources.insert(*visual, icon);
            if let Some(mesh) = item_gui::cube(sheet_faces(icon)) {
                meshes.insert(*visual, mesh);
            }
        }
        let mut models = BTreeMap::new();
        if let Some(equipment) = equipment {
            for texture in equipment.textures() {
                atlas.insert([texture.width, texture.height], &texture.rgba8)?;
            }
            if let Some(binding) = equipment
                .bindings_for(assets::gui_item::SHIELD_IDENTIFIER)
                .iter()
                .rev()
                .find(|binding| player_preview::selection::unrestricted(binding, Some(entities)))
                && let Some(geometry) = entities
                    .geometries()
                    .iter()
                    .find(|geometry| geometry.identifier == binding.geometry.identifier)
                && let Some(texture) = equipment.texture(&binding.texture.identifier)
                && let Some(icon) = self.shield
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
            equipment,
            &held_sources,
        )?;
        let required_pages = atlas.page_count();
        let mut optional_models = BTreeSet::new();
        let mut faded_thumbnails = BTreeSet::new();
        for entry in icons.entries() {
            let Some(definition) = entities.item_visuals().iter().find(|visual| {
                visual.key.identifier == entry.identifier && visual.key.metadata == entry.metadata
            }) else {
                continue;
            };
            let ItemVisualDefinitionRoute::BlockItem { block_visual } = definition.route else {
                continue;
            };
            let Some(icon) = self
                .icon_refs
                .as_deref()
                .and_then(|refs| refs.get(entry.sprite as usize))
            else {
                continue;
            };
            let key = icon_key(*icon);
            if models.contains_key(&key) {
                continue;
            }
            // Native cube meshes bake directional lighting; preserve unshaded thumbnails.
            let block = world.resolve(assets::NetworkIdMode::Sequential, block_visual.0);
            if assets::BlockFace::ALL.into_iter().any(|face| {
                world
                    .materials()
                    .get(block.face(face).material_id() as usize)
                    .is_some_and(|material| {
                        material.flags & assets::MATERIAL_FLAG_DISABLE_FACE_DIMMING != 0
                    })
            }) {
                continue;
            }
            let mesh = if let Some(mesh) = meshes.get(&block_visual.0) {
                Arc::clone(mesh)
            } else {
                // World model thumbnails retain full-resolution tiles and directional UVs.
                // Only authored carried sheets define the canonical cube shortcut.
                if icons
                    .block_sheets()
                    .binary_search_by_key(&block_visual.0, |sheet| sheet.visual.0)
                    .is_err()
                {
                    continue;
                }
                let Some(sprite) = sources.get(&block_visual.0) else {
                    continue;
                };
                if !cube_shape_admitted(&ordinary, block_visual.0) {
                    continue;
                }
                // Normalize alpha-tested faces using their material mode. Fractional blended
                // faces retain the thumbnail's composited texels.
                let Some(sprite) = gui_cube_sheet(world, block_visual, sprite) else {
                    continue;
                };
                // Optional sources follow required textures so discarding their meshes can
                // reclaim their pages. A full atlas leaves the original thumbnail intact.
                let Some(mesh) = atlas.try_insert(
                    [([sprite.width, sprite.height], sprite.rgba8.as_ref())],
                    |icons| item_gui::cutout_cube(sheet_faces(icons[0])),
                ) else {
                    continue;
                };
                meshes.insert(block_visual.0, Arc::clone(&mesh));
                mesh
            };
            models.insert(key, mesh);
            if !held_sources.contains_key(&block_visual.0) {
                optional_models.insert(key);
                faded_thumbnails.insert(key);
            }
        }
        if let Some(refs) = self.icon_refs.as_deref() {
            for thumbnail in icons.block_models() {
                if let Some(icon) = refs.get(thumbnail.sprite as usize)
                    && !models.contains_key(&icon_key(*icon))
                    && let Some(mesh) = block_models::mesh(world, thumbnail.visual, &mut atlas)
                {
                    models.insert(icon_key(*icon), mesh);
                    optional_models.insert(icon_key(*icon));
                    faded_thumbnails.insert(icon_key(*icon));
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
        tracing::info!(
            models = models.len(),
            source_pages = pages.len(),
            "native GUI item geometry prepared"
        );
        for hat in cosmetics::Item::all() {
            render_model::hats::rig(hat);
        }
        Ok(PreparedGuiModels {
            color_masks: super::color_icons::prepare(
                entities,
                icons,
                self.icon_refs.as_deref().unwrap_or_default(),
            ),
            pages,
            models,
            optional_models,
            faded_thumbnails,
            required_pages,
            textures,
            held,
            hand_pivots: held::player_hands(entities),
            cape: render_model::cape::CapeRig::resolve(entities),
        })
    }
}

impl UiPresentationRuntime {
    /// Captures stable atlas addresses without preparing any item geometry.
    pub fn gui_model_preparation(&self) -> GuiModelPreparation {
        GuiModelPreparation {
            icons: self.icon_catalog.clone(),
            icon_refs: self.icon_refs.clone(),
            first: (self.textures.dynamic_start() + MODEL_PAGE) as u16,
            shield: self.item_icon(assets::gui_item::SHIELD_IDENTIFIER, 0),
        }
    }

    /// Prepares item sources immediately for callers outside deferred startup.
    pub fn set_gui_models(
        &mut self,
        world: &RuntimeAssets,
        entities: &RuntimeEntityAssets,
    ) -> Result<(), UiPresentationError> {
        let prepared = self.gui_model_preparation().prepare(
            world,
            entities,
            self.equipment_catalog.as_deref(),
        )?;
        self.install_gui_model_sources(prepared)
    }

    /// Enables the menu's live avatar without preparing inventory item geometry.
    pub fn enable_player_preview(&mut self) {
        self.gui_models.enabled = true;
    }

    /// Installs item sources prepared on a worker without replacing the live menu avatar.
    pub fn install_gui_model_sources(
        &mut self,
        sources: PreparedGuiModels,
    ) -> Result<(), UiPresentationError> {
        self.gui_models.sources = sources;
        self.gui_models.clear_selected_hands();
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
    #[cfg(test)]
    pub(super) fn apply_gui_models(&self, nodes: &mut [UiNode]) {
        let width = nodes
            .iter()
            .map(|node| node.bounds().max().x())
            .fold(1.0, f32::max);
        let height = nodes
            .iter()
            .map(|node| node.bounds().max().y())
            .fold(1.0, f32::max);
        self.apply_gui_models_in(
            nodes,
            super::rect(0.0, 0.0, width, height).unwrap(),
            SafeArea::ZERO,
            TextEffects::default(),
            0..0,
        );
    }

    /// Admits player geometry against the actual frame, measuring coalescing only near batch limits.
    /// `hud` holds the nodes the HUD painted, whose doll keeps its own pose.
    pub(super) fn apply_gui_models_in(
        &self,
        nodes: &mut [UiNode],
        viewport: UiRect,
        safe_area: SafeArea,
        effects: TextEffects<'_>,
        hud: std::ops::Range<usize>,
    ) {
        if !self.gui_models.enabled {
            return;
        }
        let mut modulated_meshes = self
            .gui_models
            .modulated
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        replace_model_icons(
            nodes,
            None,
            &self.gui_models.sources.faded_thumbnails,
            || None,
            |key| {
                self.gui_models
                    .sources
                    .models
                    .get(key)
                    .or_else(|| self.session_icons.models.get(key))
            },
            |mesh, color, glint| modulated_meshes.get(mesh, color, glint),
        );
        modulated_meshes.retain_drawn();
        drop(modulated_meshes);
        if let Some(key) = self.player_preview_icon.map(icon_key) {
            // Screens draw over the HUD, so their doll is admitted first.
            for (in_hud, view) in [
                (false, self.player_preview_view),
                (true, self.hud_preview_view),
            ] {
                let targets = PreviewTargets {
                    key,
                    hud: hud.clone(),
                    in_hud,
                };
                self.apply_gui_player_model_in(nodes, viewport, safe_area, effects, &targets, view);
            }
        }
        self.apply_cosmetic_gallery_models(nodes);
    }

    /// Admits the equipped character before optional gallery meshes consume the remaining budget.
    fn apply_gui_player_model_in(
        &self,
        nodes: &mut [UiNode],
        viewport: UiRect,
        safe_area: SafeArea,
        effects: TextEffects<'_>,
        targets: &PreviewTargets,
        view: player_preview::PreviewView,
    ) {
        let is_preview = |index: usize, visual: &UiVisual| targets.shows(index, visual);
        if !nodes
            .iter()
            .enumerate()
            .any(|(index, node)| is_preview(index, node.visual()))
        {
            return;
        }
        let Some(sources) = self.gui_player_sources(view) else {
            return;
        };
        let Some((body_indices, body_batches)) = sources.counts() else {
            return;
        };
        let held = self.preview_held_models([true; 2]);
        let instances = nodes
            .iter()
            .enumerate()
            .filter(|(index, node)| is_preview(*index, node.visual()))
            .count();
        let baseline_batches = std::cell::OnceCell::new();
        let admitted = [[true; 2], [true, false], [false, true], [false; 2]]
            .into_iter()
            .find(|mask| {
                let mut indices = body_indices;
                let mut batches = body_batches;
                let mut hand_batches = 0usize;
                for (index, model) in held.iter().enumerate() {
                    if mask[index]
                        && let Some(model) = model
                    {
                        let Some(next) = indices.checked_add(model.vertices.len()) else {
                            return false;
                        };
                        indices = next;
                        batches += 1;
                        hand_batches += 1;
                    }
                }
                if UiMesh::check_size(indices, indices, batches).is_err() {
                    return false;
                }
                let prospective = ui::UiTree::check_visual_budget_with_mesh(
                    nodes
                        .iter()
                        .enumerate()
                        .filter(|(index, node)| !is_preview(*index, node.visual()))
                        .map(|(_, node)| node.visual()),
                    indices,
                    batches,
                    instances,
                );
                match prospective {
                    Ok(bound) if bound <= UiLimits::MAX_DRAW_BATCHES => true,
                    Ok(_) => {
                        let actual = baseline_batches.get_or_init(|| {
                            let probe = sources.batch_probe()?;
                            let mut baseline = nodes.to_vec();
                            replace_model_icons(
                                &mut baseline,
                                Some(targets),
                                &self.gui_models.sources.faded_thumbnails,
                                || Some(probe),
                                |_| None,
                                modulated,
                            );
                            RetainedDraw::build(
                                &baseline,
                                viewport,
                                UiScale::default(),
                                safe_area,
                                effects,
                            )
                            .ok()
                            .map(|draw| draw.draw_list().batches.len())
                        });
                        actual.is_some_and(|actual| {
                            hand_batches
                                .checked_mul(instances)
                                .and_then(|extra| actual.checked_add(extra))
                                .is_some_and(|total| total <= UiLimits::MAX_DRAW_BATCHES)
                        })
                    }
                    Err(_) => false,
                }
            });
        let player = admitted.and_then(|mask| {
            let models =
                std::array::from_fn(|index| mask[index].then(|| held[index].clone()).flatten());
            let has_held = models.iter().any(Option::is_some);
            self.project_gui_player_mesh(&sources, models, view)
                .or_else(|| {
                    has_held
                        .then(|| self.project_gui_player_mesh(&sources, [const { None }; 2], view))
                        .flatten()
                })
        });
        replace_model_icons(
            nodes,
            Some(targets),
            &self.gui_models.sources.faded_thumbnails,
            || player,
            |_| None,
            modulated,
        );
    }

    #[cfg(test)]
    fn gui_player_mesh(&self) -> Option<Arc<UiMesh>> {
        self.gui_player_mesh_with_held([true; 2])
    }

    /// Omits held geometry when it would exhaust the surrounding inventory or HUD frame.
    #[cfg(test)]
    fn gui_player_mesh_with_held(&self, include_held: [bool; 2]) -> Option<Arc<UiMesh>> {
        self.gui_player_mesh_with_models(self.preview_held_models(include_held))
    }

    /// Resolves cached source geometry without projecting or allocating UI vertices.
    fn preview_held_models(
        &self,
        include_held: [bool; 2],
    ) -> [Option<player_preview::PreviewHeldModel>; 2] {
        std::array::from_fn(|index| {
            let hand = &self.player_preview_gear.hands[index];
            if !include_held[index] {
                return None;
            }
            let hand = hand.as_ref()?;
            let pack_model_suppressed = !self.player_preview_gear.show_custom_item_models
                && self
                    .gui_models
                    .pack_equipment
                    .source
                    .as_ref()
                    .is_some_and(|pack| {
                        self.gui_models.selected_held[index]
                            .as_ref()
                            .is_some_and(|selected| Arc::ptr_eq(&selected.artwork, pack))
                    });
            if let Some(selected) = self.gui_models.selected_held[index]
                .as_ref()
                .filter(|_| !pack_model_suppressed)
                && selected.ready
                && selected.item.identifier.as_ref() == hand.identifier.as_ref()
                && selected.item.metadata == hand.metadata
            {
                return Some(
                    selected
                        .model
                        .as_ref()?
                        .with_grip(hand.grip)
                        .with_glint(hand.glint),
                );
            }
            let (identifier, metadata) = Self::item_icon_key(
                &hand.identifier,
                hand.metadata,
                hand.use_animation,
                hand.charged_projectile,
                None,
            );
            let held = |metadata| {
                self.gui_models.sources.held.get(&assets::ItemVisualKey {
                    identifier: identifier.into(),
                    metadata,
                })
            };
            let base = (!pack_model_suppressed)
                .then(|| held(metadata).or_else(|| held(0)))
                .flatten();
            let model = if !pack_model_suppressed
                && let Some((source, vertices)) =
                    self.session_icons.held_block(identifier, metadata)
            {
                player_preview::PreviewHeldModel {
                    source,
                    vertices,
                    hand_pivots: self.gui_models.sources.hand_pivots?,
                    placements: [player_preview::PreviewHeldPlacement::Block; 2],
                }
            } else if base.is_none_or(held::HeldModel::is_sprite)
                && self.session_item_icon(identifier, metadata).is_some()
            {
                let (source, vertices) = self.session_icons.held_sprite(identifier, metadata)?;
                match base {
                    Some(base) => base.with_sprite(source, vertices),
                    None => player_preview::PreviewHeldModel {
                        source,
                        vertices,
                        hand_pivots: self.gui_models.sources.hand_pivots?,
                        placements: [player_preview::PreviewHeldPlacement::Sprite {
                            hand_equipped: false,
                        }; 2],
                    },
                }
            } else {
                base?.model(self.icon_catalog.as_deref()?)?
            };
            Some(model.with_grip(hand.grip).with_glint(hand.glint))
        })
    }

    /// Builds a test preview directly from prepared source layers and selected hand models.
    #[cfg(test)]
    fn gui_player_mesh_with_models(
        &self,
        held: [Option<player_preview::PreviewHeldModel>; 2],
    ) -> Option<Arc<UiMesh>> {
        let view = self.player_preview_view;
        self.project_gui_player_mesh(&self.gui_player_sources(view)?, held, view)
    }

    /// Resolves unprojected skin, cape, armor and flame sources once for this frame.
    fn gui_player_sources(
        &self,
        view: player_preview::PreviewView,
    ) -> Option<player_preview::geometry::MeshSources<'_>> {
        let skin = self.gui_models.skin.as_ref()?;
        let [width, height] = skin.dimensions();
        let skin = IconRef {
            color_mask: None,
            page: (self.textures.dynamic_start() + SKIN_PAGE) as u16,
            uv: [0, 0, width.try_into().ok()?, height.try_into().ok()?],
            glint: false,
        };
        let armor = std::array::from_fn(|slot| {
            let texture = self.player_preview_gear.armor[slot].as_ref()?;
            self.gui_models
                .pack_equipment
                .region(slot)
                .or_else(|| {
                    let key = atlas::key([texture.width, texture.height], &texture.rgba);
                    self.gui_models.sources.textures.get(&key).copied()
                })
                .map(|icon| icon.with_glint(self.player_preview_gear.armor_glint[slot]))
        });
        let unposed = [None; 6];
        // Only the HUD doll draws the live actor; other renderers pose the menu model.
        let live =
            (view == player_preview::PreviewView::Hud).then_some(&self.gui_models.live_player);
        let body = if let Some(live) = live {
            Some((live.vertices.as_slice(), &live.parts))
                .filter(|(vertices, _)| !vertices.is_empty())
        } else {
            self.menu_preview_model
                .vertices
                .as_deref()
                .map(|vertices| (vertices, &unposed))
        };
        let cape = self.menu_preview_model.cape_key.as_deref().and_then(|key| {
            if self.gui_models.wearing_elytra {
                return None;
            }
            let vertices = if let Some(live) = live {
                let vertices = live.cape_vertices.as_slice();
                if vertices.is_empty() {
                    return None;
                }
                vertices
            } else {
                player_preview::cape::rest_vertices()
            };
            self.menu_artwork_icon(key).map(|icon| (vertices, icon))
        });
        let mut sources = player_preview::geometry::MeshSources::new(
            body,
            skin,
            &self.player_preview_gear,
            armor,
            self.gui_fire_frame(view),
            cape,
        );
        for &hat in &self.menu_preview_model.cosmetics {
            if render_model::hats::rig(hat).is_ground_effect() {
                continue;
            }
            let Some(icon) = self.menu_artwork_icon(&hat.texture_key()) else {
                continue;
            };
            let vertices = if let Some(live) = live {
                live.hat_vertices.get(&hat).map_or(&[][..], Vec::as_slice)
            } else {
                self.gui_models.cosmetics.vertices(hat)
            };
            sources = sources.with_attachment(vertices, icon);
        }
        // Particles follow the view they were last placed for.
        if view == self.player_preview_view {
            sources = sources.with_particles(self.gui_models.particles.mesh.as_deref());
        }
        Some(sources)
    }

    /// Projects the prepared body and only the hand models admitted for this frame.
    fn project_gui_player_mesh(
        &self,
        sources: &player_preview::geometry::MeshSources<'_>,
        held: [Option<player_preview::PreviewHeldModel>; 2],
        view: player_preview::PreviewView,
    ) -> Option<Arc<UiMesh>> {
        player_preview::geometry::mesh_from_sources(
            sources,
            self.player_preview_pose.unwrap_or_default(),
            view,
            self.player_preview_bob,
            held.each_ref().map(Option::as_ref),
            true, // Current desktop default is the native Fancy material group.
            if view == player_preview::PreviewView::Hud {
                self.gui_models.live_player.overlay_color
            } else {
                [0.0; 4]
            },
        )
    }
}
