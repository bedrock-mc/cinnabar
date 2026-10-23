//! Block facts icon baking needs beyond the world carrier: registry families, the pack's carried
//! textures, and the canonical state an item is drawn from.
//!
//! Vanilla's item renderer draws a block item flat when it cannot tessellate
//! its shape, and then shows the carried texture, down face, at the
//! block's variant.

use std::{borrow::Cow, path::Path, sync::Arc};

use assets::registry::BlockTraits;
use assets::{
    AssetError, BlockFace, BlockFlags, BlockVisualId, IconSprite, ModelFamily, NetworkIdMode,
    RegistryRecord, RuntimeAssets, VisualKind, VisualSupport, read_registry_for_protocol,
};

use pack_compile_input::image::decode_texture;
use pack_compile_input::pack::{
    PackSources, read_pack, resolve_carried_down_key, resolve_carried_face_key,
    resolve_carried_fallback_key, resolve_texture_key,
};
use pack_compile_input::static_texture_path;

/// Registry families whose vanilla block shapes render as flat item icons.
const FLAT_FAMILIES: [ModelFamily; 10] = [
    ModelFamily::Cross,
    ModelFamily::Rail,
    ModelFamily::Torch,
    ModelFamily::Lever,
    ModelFamily::Vine,
    ModelFamily::GlowLichen,
    ModelFamily::SculkVein,
    ModelFamily::Aquatic,
    ModelFamily::FlowerBed,
    ModelFamily::ResinClump,
];

/// Blocks whose vanilla block shape the item renderer cannot tessellate, so their items show
/// the carried texture flat: barrier and the light blocks.
const FLAT_TRAITS: BlockTraits = BlockTraits::INVISIBLE.union(BlockTraits::LIGHT_BLOCK);

pub(super) struct IconBlocks {
    records: Box<[RegistryRecord]>,
    /// `None` when the pack lacks the block and terrain catalogs.
    pack: Option<PackSources>,
}

/// A resolved terrain destination retains its identity alongside the shared image source.
struct CarriedMaterial<'a> {
    key: Cow<'a, str>,
    path: &'a str,
    overlay: Option<&'a str>,
}

impl IconBlocks {
    /// The registry identifier of a compiled block visual.
    pub(super) fn name(&self, visual: BlockVisualId) -> Option<&str> {
        self.records
            .get(visual.0 as usize)
            .map(|record| record.name.as_ref())
    }

    /// Chest GUI faces are inventory terrain tiles even though the placed chest is entity-drawn.
    pub(super) fn inventory_cube_tiles(
        &self,
        root: &Path,
        visual: BlockVisualId,
    ) -> Result<Option<[IconSprite; 6]>, AssetError> {
        let renderer = assets::facts::BlockEntityFacts::pinned()
            .get(visual.0)
            .map(|backing| backing.entity.renderer);
        if !matches!(
            renderer,
            Some(
                assets::facts::BlockEntityRenderer::Chest
                    | assets::facts::BlockEntityRenderer::EnderChest
            )
        ) {
            return Ok(None);
        }
        self.tiles(root, visual, true, true)
    }

    pub(super) fn read(root: &Path) -> Result<Self, AssetError> {
        let records = read_registry_for_protocol(assets::pinned_block_registry_bytes(), 2193)?;
        let has_catalogs = root.join("blocks.json").is_file()
            && root.join("textures/terrain_texture.json").is_file();
        Ok(Self {
            records,
            pack: has_catalogs.then(|| read_pack(root)).transpose()?,
        })
    }

    pub(super) fn is_flat(&self, world: &RuntimeAssets, visual: BlockVisualId) -> bool {
        let Some(record) = self.records.get(visual.0 as usize) else {
            return false;
        };
        let kind = world.resolve(NetworkIdMode::Sequential, visual.0).kind();
        assets::facts::block_render_fact(&record.name).kind
            == assets::facts::BlockRenderKind::LeafLitter
            || FLAT_FAMILIES.contains(&record.model_family)
            || record.traits.intersects(FLAT_TRAITS)
            || (kind == VisualKind::Cross && record.model_family != ModelFamily::Crop)
            || (matches!(
                record.model_family,
                ModelFamily::Unknown | ModelFamily::Pane
            ) && assets::facts::block_render_fact(&record.name).flat_icon)
    }

    /// Selects carried poses for wall arms, closed barrels and upright unpowered rods.
    pub(super) fn icon_state(&self, visual: BlockVisualId) -> BlockVisualId {
        let Some(record) = self.records.get(visual.0 as usize) else {
            return visual;
        };
        if pack_compile_input::lightning_rod::is_record(record) {
            return self
                .records
                .iter()
                .find(|candidate| {
                    candidate.name == record.name
                        && pack_compile_input::lightning_rod::state(candidate) == Some((1, 0))
                })
                .map_or(visual, |candidate| BlockVisualId(candidate.sequential_id));
        }
        if assets::facts::block_render_fact(&record.name).kind
            == assets::facts::BlockRenderKind::Barrel
        {
            return self
                .records
                .iter()
                .find(|candidate| {
                    if candidate.name != record.name {
                        return false;
                    }
                    let Ok(state) =
                        serde_json::from_str::<serde_json::Value>(&candidate.canonical_state)
                    else {
                        return false;
                    };
                    use assets::facts::BlockProperty;
                    state[BlockProperty::FacingDirection.name()]["value"] == 3
                        && state[BlockProperty::OpenBit.name()]["value"] == 0
                })
                .map_or(visual, |candidate| BlockVisualId(candidate.sequential_id));
        }
        if record.model_family != ModelFamily::Wall {
            return visual;
        }
        let wanted = |state: &str| {
            let Ok(state) = serde_json::from_str::<serde_json::Value>(state) else {
                return false;
            };
            let value = |key: &str| state.get(key).and_then(|entry| entry.get("value")).cloned();
            value(assets::facts::BlockProperty::WallPostBit.name()) == Some(1.into())
                && ["east", "west"].iter().all(|side| {
                    value(&format!("wall_connection_type_{side}")) == Some("short".into())
                })
                && ["north", "south"].iter().all(|side| {
                    value(&format!("wall_connection_type_{side}")) == Some("none".into())
                })
        };
        self.records
            .iter()
            .find(|candidate| candidate.name == record.name && wanted(&candidate.canonical_state))
            .map_or(visual, |candidate| BlockVisualId(candidate.sequential_id))
    }

    /// Full cube item geometry is independent of terrain occlusion, material and animation.
    pub(super) fn is_carried_cube(&self, world: &RuntimeAssets, visual: BlockVisualId) -> bool {
        let block = world.resolve(NetworkIdMode::Sequential, visual.0);
        if !block.is_known() {
            return false;
        }
        if block.support() == VisualSupport::VanillaFallback
            && self.records.get(visual.0 as usize).is_some_and(|record| {
                record
                    .traits
                    .contains(BlockTraits::PISTON | BlockTraits::BODY_RELATIVE_FACING)
            })
        {
            return true;
        }
        if block.support() != VisualSupport::Exact {
            return false;
        }
        match block.kind() {
            VisualKind::Cube => block.flags().contains(BlockFlags::CUBE_GEOMETRY),
            VisualKind::Model => {
                assets::gui_item::full_cube_model_quads(world, visual).is_some()
                    || block
                        .model_template()
                        .and_then(|id| world.model_templates().get(id as usize))
                        .is_some_and(|template| {
                            template.flags & assets::MODEL_TEMPLATE_FLAG_TRANSPARENT_CUBE != 0
                        })
            }
            _ => false,
        }
    }

    /// A cube defaults to its ordinary pack faces when it has no carried override.
    pub(super) fn carried_cube_tiles(
        &self,
        root: &Path,
        visual: BlockVisualId,
    ) -> Result<Option<[IconSprite; 6]>, AssetError> {
        self.tiles(root, self.icon_state(visual), true, true)
    }

    /// Resolves explicit carried faces for shapes whose world model cannot supply an icon.
    pub(super) fn carried_tiles(
        &self,
        root: &Path,
        visual: BlockVisualId,
    ) -> Result<Option<[IconSprite; 6]>, AssetError> {
        self.tiles(root, visual, false, true)
    }

    /// Resolves the placed face artwork without replacing it with carried-only attachments.
    pub(super) fn terrain_tiles(
        &self,
        root: &Path,
        visual: BlockVisualId,
    ) -> Result<Option<[IconSprite; 6]>, AssetError> {
        self.tiles(root, visual, true, false)
    }

    fn tiles(
        &self,
        root: &Path,
        visual: BlockVisualId,
        world_fallback: bool,
        carried: bool,
    ) -> Result<Option<[IconSprite; 6]>, AssetError> {
        let (Some(pack), Some(record)) = (self.pack.as_ref(), self.records.get(visual.0 as usize))
        else {
            return Ok(None);
        };
        let mut tiles = Vec::with_capacity(BlockFace::ALL.len());
        for face in BlockFace::ALL {
            let (source_face, flip) = if carried
                && assets::facts::block_render_fact(&record.name).kind
                    == assets::facts::BlockRenderKind::Barrel
            {
                let Some(assets::facts::StateValue::Int(direction)) =
                    record.property(assets::facts::BlockProperty::FacingDirection)
                else {
                    return Ok(None);
                };
                let Some(transform) = usize::try_from(*direction).ok().and_then(|direction| {
                    pack_compile_input::barrel::face_transform(face, direction)
                }) else {
                    return Ok(None);
                };
                transform
            } else {
                (face, 0)
            };
            let world = resolve_texture_key(&pack.blocks, record, source_face).key;
            let variant = world
                .as_ref()
                .and_then(|key| pack.terrain.get_for_model_record(key, record))
                .map_or(0, |(_, variant)| variant as usize);
            let Some(key) = carried
                .then(|| resolve_carried_face_key(&pack.blocks, record, source_face))
                .flatten()
                .map(String::into_boxed_str)
                .or_else(|| {
                    if !world_fallback {
                        return None;
                    }
                    if matches!(
                        assets::facts::block_render_fact(&record.name).kind,
                        assets::facts::BlockRenderKind::Furnace
                            | assets::facts::BlockRenderKind::LitFurnace
                    ) {
                        resolve_carried_fallback_key(&pack.blocks, record, face)
                    } else {
                        world
                    }
                })
            else {
                return Ok(None);
            };
            let Some((path, overlay)) = pack.terrain.get_clamped_carried(&key, variant) else {
                return Ok(None);
            };
            let tile = if carried {
                super::carried::tile(
                    root,
                    path,
                    overlay,
                    pack_compile_input::respawn_anchor::is_record(record),
                )?
            } else {
                super::carried::model_tile(self, root, &key, path, overlay)?
            };
            let Some(tile) = tile else {
                return Ok(None);
            };
            tiles.push(super::carried::flip(tile, flip)?);
        }
        Ok(tiles.try_into().ok())
    }

    /// Retains a pane's flat carried artwork independently of its GUI plate.
    pub(super) fn held_texture_path(&self, visual: BlockVisualId) -> Option<Box<str>> {
        if self.records.get(visual.0 as usize)?.model_family != ModelFamily::Pane {
            return None;
        }
        self.texture_path(visual)
    }

    /// The pack path of `visual`'s flat icon; `None` when absent, tinted, or unresolvable.
    pub(super) fn texture_path(&self, visual: BlockVisualId) -> Option<Box<str>> {
        let material = self.carried_material(visual, None)?;
        material.overlay.is_none().then(|| material.path.into())
    }

    /// Loads carried model artwork with its declared animation frame and overlay tint.
    pub(super) fn carried_model_tile(
        &self,
        root: &Path,
        visual: BlockVisualId,
    ) -> Result<Option<IconSprite>, AssetError> {
        let Some(material) = self.carried_material(visual, None) else {
            return Ok(None);
        };
        super::carried::model_tile(self, root, &material.key, material.path, material.overlay)
    }

    /// Selects the carried down-face material at the block's world variant.
    fn carried_material(
        &self,
        visual: BlockVisualId,
        face: Option<BlockFace>,
    ) -> Option<CarriedMaterial<'_>> {
        let pack = self.pack.as_ref()?;
        let record = self.records.get(visual.0 as usize)?;
        let key = match face {
            Some(face) => resolve_carried_face_key(&pack.blocks, record, face)?.into(),
            None => resolve_carried_down_key(&pack.blocks, record)?,
        };
        // The world key's variant stands in for the block's variant.
        let variant = resolve_texture_key(&pack.blocks, record, face.unwrap_or(BlockFace::Down))
            .key
            .and_then(|world| pack.terrain.get_for_model_record(&world, record))
            .map_or(0, |(_, variant)| variant as usize);
        let (path, overlay) = pack.terrain.get_clamped_carried(&key, variant)?;
        Some(CarriedMaterial { key, path, overlay })
    }

    /// Loads one explicitly carried attachment with its animation frame and overlay tint.
    pub(super) fn attachment_tile(
        &self,
        root: &Path,
        visual: BlockVisualId,
        face: BlockFace,
    ) -> Result<Option<IconSprite>, AssetError> {
        let Some(material) = self.carried_material(visual, Some(face)) else {
            return Ok(None);
        };
        super::carried::model_tile(self, root, &material.key, material.path, material.overlay)
    }

    /// A flat block icon; a declared flipbook shows its first frame.
    pub(super) fn flat_sprite(
        &self,
        root: &Path,
        path: &str,
    ) -> Result<Option<IconSprite>, AssetError> {
        Ok(self
            .texture_frame(root, path, None)?
            .and_then(|decoded| super::sprites::bounded_sprite(decoded).map(|(sprite, _)| sprite)))
    }

    /// Model artwork retains source resolution after selecting its declared animation frame.
    pub(super) fn model_frame_sprite(
        &self,
        root: &Path,
        destination: &str,
        path: &str,
    ) -> Result<Option<IconSprite>, AssetError> {
        let Some(decoded) = self.texture_frame(root, path, Some(destination))? else {
            return Ok(None);
        };
        Ok(Some(IconSprite {
            width: decoded.width as u16,
            height: decoded.height as u16,
            rgba8: decoded.rgba8.into(),
        }))
    }

    /// Loads a declared texture frame before applying consumer-specific dimensions.
    fn texture_frame(
        &self,
        root: &Path,
        path: &str,
        destination: Option<&str>,
    ) -> Result<Option<pack_compile_input::image::DecodedTexture>, AssetError> {
        let file = static_texture_path(root, path, path)?;
        if !file.is_file() {
            return Ok(None);
        }
        let stem = |texture: &str| {
            texture
                .strip_suffix(".png")
                .or_else(|| texture.strip_suffix(".tga"))
                .unwrap_or(texture)
                .to_owned()
        };
        let flipbook = if let Some(pack) = self.pack.as_ref() {
            let selected = if let Some(destination) = destination {
                pack_compile_input::pack::select_flipbook_index(
                    pack.flipbooks
                        .iter()
                        .enumerate()
                        .filter(|(_, source)| {
                            source.atlas_tile.as_ref() == destination
                                && stem(&source.texture_path) == stem(path)
                        })
                        .map(|(index, source)| {
                            (index, source.atlas_index, source.atlas_tile_variant)
                        }),
                    None,
                    path,
                )?
            } else {
                None
            };
            selected
                .map(|index| (index, &pack.flipbooks[index], true))
                .or_else(|| {
                    pack.flipbooks
                        .iter()
                        .enumerate()
                        .filter(|(_, source)| stem(&source.texture_path) == stem(path))
                        .min_by_key(|(_, source)| source.replicate)
                        .map(|(index, source)| (index, source, false))
                })
        } else {
            None
        };
        let decoded = if let Some((index, flipbook, uses_timeline)) = flipbook {
            let first_frame = if uses_timeline {
                flipbook.frames.first().copied().unwrap_or(0)
            } else {
                0
            };
            let mut decoded = pack_compile_input::image::decode_model_texture(
                &file,
                path,
                Some((index, first_frame)),
            )?;
            if decoded.width == decoded.height {
                let (rgba8, side) = pack_compile_input::image::replicated_tile(
                    decoded.rgba8.into_vec(),
                    decoded.width,
                    if destination.is_some() {
                        flipbook.replicate
                    } else {
                        1
                    },
                );
                decoded.rgba8 = rgba8;
                decoded.width = side;
                decoded.height = side;
            }
            decoded
        } else if destination.is_some() {
            pack_compile_input::image::decode_model_texture(&file, path, None)?
        } else {
            decode_texture(&file, path)?
        };
        Ok(Some(decoded))
    }

    /// Loads a bounded sprite or an unresized square frame for carried-face baking.
    pub(super) fn sprite_with_strip_frame(
        root: &Path,
        path: &str,
        first_strip_frame: bool,
    ) -> Result<Option<IconSprite>, AssetError> {
        let file = static_texture_path(root, path, path)?;
        if !file.is_file() {
            return Ok(None);
        }
        let mut decoded = decode_texture(&file, path)?;
        if first_strip_frame {
            pack_compile_input::image::retain_first_strip_frame(&mut decoded);
            if decoded.width == decoded.height
                && decoded.width >= assets::TILE_SIZE
                && decoded.width.is_power_of_two()
            {
                return Ok(Some(IconSprite {
                    width: decoded.width as u16,
                    height: decoded.height as u16,
                    rgba8: Arc::from(decoded.rgba8),
                }));
            }
            return Ok(None);
        }
        Ok(super::sprites::bounded_sprite(decoded).map(|(sprite, _)| sprite))
    }

    /// A model texture to sample, as large as the block-entity carrier packs; `None` when absent
    /// or larger.
    pub(super) fn model_texture(root: &Path, path: &str) -> Result<Option<IconSprite>, AssetError> {
        let file = static_texture_path(root, path, path)?;
        if !file.is_file() {
            return Ok(None);
        }
        // Bound the size before decoding, whose allocation limit only fits carrier-sized art.
        let oversized =
            |(width, height): (u32, u32)| width.max(height) > crate::block_entity::MAX_TEXTURE_SIDE;
        if image::image_dimensions(&file).is_ok_and(oversized) {
            return Ok(None);
        }
        let decoded = decode_texture(&file, path)?;
        if oversized((decoded.width, decoded.height)) {
            return Ok(None);
        }
        Ok(Some(IconSprite {
            width: decoded.width as u16,
            height: decoded.height as u16,
            rgba8: decoded.rgba8.into(),
        }))
    }
}

#[cfg(test)]
#[path = "blocks/respawn_anchor_tests.rs"]
mod respawn_anchor_tests;

#[cfg(test)]
#[path = "blocks/piston_tests.rs"]
mod piston_tests;

#[cfg(test)]
#[path = "blocks/tests.rs"]
mod tests;

#[cfg(test)]
#[path = "blocks/lightning_rod_tests.rs"]
mod lightning_rod_tests;
