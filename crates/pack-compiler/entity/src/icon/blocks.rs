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

    /// Selects the carried pose: walls have two arms and barrels face south while closed.
    pub(super) fn icon_state(&self, visual: BlockVisualId) -> BlockVisualId {
        let Some(record) = self.records.get(visual.0 as usize) else {
            return visual;
        };
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
        if !block.is_known() || block.support() != VisualSupport::Exact {
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
        let material = self.carried_material(visual)?;
        material.overlay.is_none().then(|| material.path.into())
    }

    /// Loads carried model artwork with its declared animation frame and overlay tint.
    pub(super) fn carried_model_tile(
        &self,
        root: &Path,
        visual: BlockVisualId,
    ) -> Result<Option<IconSprite>, AssetError> {
        let Some(material) = self.carried_material(visual) else {
            return Ok(None);
        };
        super::carried::model_tile(self, root, &material.key, material.path, material.overlay)
    }

    /// Selects the carried down-face material at the block's world variant.
    fn carried_material(&self, visual: BlockVisualId) -> Option<CarriedMaterial<'_>> {
        let pack = self.pack.as_ref()?;
        let record = self.records.get(visual.0 as usize)?;
        let key = resolve_carried_down_key(&pack.blocks, record)?;
        // The world key's variant stands in for the block's variant.
        let variant = resolve_texture_key(&pack.blocks, record, BlockFace::Down)
            .key
            .and_then(|world| pack.terrain.get_for_model_record(&world, record))
            .map_or(0, |(_, variant)| variant as usize);
        let (path, overlay) = pack.terrain.get_clamped_carried(&key, variant)?;
        Some(CarriedMaterial { key, path, overlay })
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
mod tests {
    use super::*;

    #[test]
    fn glass_panes_are_models_while_metal_bars_remain_flat() {
        let blocks = IconBlocks::read(tempfile::tempdir().unwrap().path()).unwrap();
        let world = RuntimeAssets::diagnostic();
        let panes: Vec<_> = blocks
            .records
            .iter()
            .filter(|record| record.model_family == ModelFamily::Pane)
            .collect();
        assert!(!panes.is_empty());
        for record in panes {
            assert_eq!(
                blocks.is_flat(&world, BlockVisualId(record.sequential_id)),
                record.name.ends_with("_bars"),
                "{}",
                record.name
            );
        }
    }

    #[test]
    fn crystal_and_spike_items_use_the_flat_inventory_variant() {
        let records = read_registry_for_protocol(
            assets::pinned_block_registry_bytes(),
            assets::active_content_registry_protocol(),
        )
        .unwrap();
        let selected: Vec<_> = records
            .iter()
            .filter(|record| {
                assets::facts::block_render_fact(&record.name).kind
                    == assets::facts::BlockRenderKind::Crystal
                    || record.traits.contains(BlockTraits::DRIPSTONE_OFFSET)
            })
            .collect();
        assert!(!selected.is_empty());
        let mut catalog = serde_json::Map::new();
        for record in &selected {
            catalog.insert(
                record.name.strip_prefix("minecraft:").unwrap().into(),
                serde_json::json!({"textures": "tile"}),
            );
        }
        let root = tempfile::tempdir().unwrap();
        write_pack(
            root.path(),
            &serde_json::Value::Object(catalog).to_string(),
            r#"{"texture_data":{"tile":{"textures":["textures/blocks/inventory","textures/blocks/placed"]}}}"#,
            "[]",
        );
        let blocks = IconBlocks::read(root.path()).unwrap();
        let world = RuntimeAssets::diagnostic();
        for record in selected {
            let visual = BlockVisualId(record.sequential_id);
            assert!(blocks.is_flat(&world, visual), "{}", record.name);
            assert_eq!(
                blocks.texture_path(visual).as_deref(),
                Some("textures/blocks/inventory"),
                "{} {}",
                record.name,
                record.canonical_state
            );
        }
    }

    #[test]
    fn candles_keep_their_carried_icons_without_world_model_geometry() {
        let blocks = IconBlocks {
            records: assets::read_registry_for_protocol(
                assets::pinned_block_registry_bytes(),
                assets::active_content_registry_protocol(),
            )
            .unwrap(),
            pack: None,
        };
        let world = RuntimeAssets::diagnostic();
        for name in [
            "minecraft:candle",
            "minecraft:magenta_candle",
            "minecraft:bell",
        ] {
            let record = blocks
                .records
                .iter()
                .find(|record| record.name.as_ref() == name)
                .unwrap();
            assert!(blocks.is_flat(&world, BlockVisualId(record.sequential_id)));
        }
    }

    /// Barrier declares world textures, while light blocks declare only carried ones; both items
    /// draw that texture flat.
    #[test]
    fn barrier_and_light_block_items_draw_their_carried_texture_flat() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("textures")).unwrap();
        for (path, json) in [
            (
                "blocks.json",
                r#"{"barrier":{"textures":"barrier"},
                    "light_block_0":{"carried_textures":"light_block_0"},
                    "light_block_15":{"carried_textures":"light_block_15"},
                    "stone":{"textures":"stone"}}"#,
            ),
            (
                "textures/terrain_texture.json",
                r#"{"texture_data":{
                    "barrier":{"textures":"textures/blocks/barrier"},
                    "light_block_0":{"textures":"textures/items/light_block_0"},
                    "light_block_15":{"textures":"textures/items/light_block_15"},
                    "stone":{"textures":"textures/blocks/stone"}}}"#,
            ),
            ("textures/flipbook_textures.json", "[]"),
        ] {
            std::fs::write(root.path().join(path), json).unwrap();
        }
        let blocks = IconBlocks::read(root.path()).unwrap();
        let world = RuntimeAssets::diagnostic();
        let flat_textures = |name: &str| {
            let states: Vec<_> = blocks
                .records
                .iter()
                .filter(|record| record.name.as_ref() == name)
                .map(|record| BlockVisualId(record.sequential_id))
                .collect();
            assert!(!states.is_empty(), "{name} is not registered");
            states
                .into_iter()
                .map(|state| {
                    blocks
                        .is_flat(&world, state)
                        .then(|| blocks.texture_path(state))
                        .flatten()
                })
                .collect::<Vec<_>>()
        };
        for (name, texture) in [
            ("minecraft:barrier", "textures/blocks/barrier"),
            ("minecraft:light_block_0", "textures/items/light_block_0"),
            ("minecraft:light_block_15", "textures/items/light_block_15"),
        ] {
            assert!(
                flat_textures(name)
                    .iter()
                    .all(|path| path.as_deref() == Some(texture)),
                "{name} must draw {texture} flat"
            );
        }
        assert!(flat_textures("minecraft:stone").iter().all(Option::is_none));
    }

    /// Egg and small creature blocks keep their carried art independent of placed geometry.
    #[test]
    fn eggs_and_small_creatures_use_the_carried_sprite() {
        let root = tempfile::tempdir().unwrap();
        write_pack(
            root.path(),
            r#"{
                "turtle_egg":{"textures":"world","carried_textures":"carried"},
                "sniffer_egg":{"textures":"world","carried_textures":"carried"},
                "frog_spawn":{"textures":"carried"}
            }"#,
            r#"{"texture_data":{
                "world":{"textures":"textures/blocks/world"},
                "carried":{"textures":"textures/items/carried"}
            }}"#,
            "[]",
        );
        let blocks = IconBlocks::read(root.path()).unwrap();
        let world = RuntimeAssets::diagnostic();
        for name in ["turtle_egg", "sniffer_egg", "frog_spawn"] {
            let states: Vec<_> = blocks
                .records
                .iter()
                .filter(|record| record.name.as_ref() == format!("minecraft:{name}"))
                .collect();
            assert!(!states.is_empty(), "registered block {name}");
            for record in states {
                let visual = BlockVisualId(record.sequential_id);
                assert!(
                    blocks.is_flat(&world, visual),
                    "{name} uses its carried sprite"
                );
                assert_eq!(
                    blocks.texture_path(visual).as_deref(),
                    Some("textures/items/carried")
                );
            }
        }
    }

    #[test]
    fn leaf_litter_uses_a_flat_carried_icon_for_every_state() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        std::fs::create_dir_all(root.join("textures")).unwrap();
        std::fs::write(
            root.join("blocks.json"),
            r#"{"leaf_litter":{"textures":"ground","carried_textures":"carried"}}"#,
        )
        .unwrap();
        std::fs::write(root.join("textures/terrain_texture.json"),
            r#"{"texture_data":{"ground":{"textures":"textures/blocks/litter"},"carried":{"textures":"textures/items/litter"}}}"#).unwrap();
        std::fs::write(root.join("textures/flipbook_textures.json"), "[]").unwrap();
        let blocks = IconBlocks::read(root).unwrap();
        let world = RuntimeAssets::diagnostic();
        let records: Vec<_> = blocks
            .records
            .iter()
            .filter(|record| record.name.as_ref() == "minecraft:leaf_litter")
            .collect();
        assert!(!records.is_empty());
        for record in records {
            assert!(
                blocks.is_flat(&world, BlockVisualId(record.sequential_id)),
                "{}",
                record.canonical_state
            );
            assert_eq!(
                blocks
                    .texture_path(BlockVisualId(record.sequential_id))
                    .as_deref(),
                Some("textures/items/litter")
            );
        }
    }

    /// Inventory furnace faces use authored slots regardless of the placed state's direction.
    #[test]
    fn furnace_inventory_faces_show_the_unrotated_front() {
        let root = tempfile::tempdir().unwrap();
        write_pack(
            root.path(),
            r#"{
                "furnace":{"textures":{"up":"top","down":"top","north":"side","south":"front","west":"side","east":"side"}},
                "blast_furnace":{"textures":{"up":"top","down":"top","north":"side","south":"front","west":"side","east":"side"}},
                "smoker":{"textures":{"up":"top","down":"top","north":"side","south":"front","west":"side","east":"side"}}
            }"#,
            r#"{"texture_data":{
                "top":{"textures":"textures/blocks/top"},
                "side":{"textures":"textures/blocks/side"},
                "front":{"textures":"textures/blocks/front"}
            }}"#,
            "[]",
        );
        for (name, pixel) in [
            ("top", [19, 31, 47, 255]),
            ("side", [61, 79, 97, 255]),
            ("front", [101, 127, 151, 255]),
        ] {
            image::RgbaImage::from_pixel(16, 16, image::Rgba(pixel))
                .save(root.path().join(format!("textures/blocks/{name}.png")))
                .unwrap();
        }
        let blocks = IconBlocks::read(root.path()).unwrap();
        for name in [
            "minecraft:furnace",
            "minecraft:blast_furnace",
            "minecraft:smoker",
        ] {
            let states: Vec<_> = blocks
                .records
                .iter()
                .filter(|r| r.name.as_ref() == name)
                .collect();
            assert!(!states.is_empty(), "registered block {name}");
            for record in states {
                let tiles = blocks
                    .carried_cube_tiles(root.path(), BlockVisualId(record.sequential_id))
                    .unwrap()
                    .unwrap();
                assert_eq!(
                    &tiles[BlockFace::South as usize].rgba8[..4],
                    &[101, 127, 151, 255],
                    "front of {name} in {}",
                    record.canonical_state
                );
                assert_eq!(
                    &tiles[BlockFace::West as usize].rgba8[..4],
                    &[61, 79, 97, 255]
                );
                assert_eq!(
                    &tiles[BlockFace::Up as usize].rgba8[..4],
                    &[19, 31, 47, 255]
                );
            }
        }
    }

    #[test]
    fn barrel_inventory_faces_use_the_closed_south_facing_pose() {
        let root = tempfile::tempdir().unwrap();
        write_pack(
            root.path(),
            r#"{"barrel":{"textures":{"up":"side","down":"side","north":"top","south":"bottom","west":"side","east":"side"}}}"#,
            r#"{"texture_data":{"top":{"textures":"textures/blocks/top"},"bottom":{"textures":"textures/blocks/bottom"},"side":{"textures":"textures/blocks/side"}}}"#,
            "[]",
        );
        for (name, pixel) in [
            ("top", [180, 30, 40, 255]),
            ("bottom", [30, 180, 40, 255]),
            ("side", [30, 40, 180, 255]),
        ] {
            image::RgbaImage::from_pixel(16, 16, image::Rgba(pixel))
                .save(root.path().join(format!("textures/blocks/{name}.png")))
                .unwrap();
        }
        let blocks = IconBlocks::read(root.path()).unwrap();
        let mut seen = 0;
        for record in blocks
            .records
            .iter()
            .filter(|record| record.name.as_ref() == "minecraft:barrel")
        {
            let tiles = blocks
                .carried_cube_tiles(root.path(), BlockVisualId(record.sequential_id))
                .unwrap()
                .unwrap();
            assert_eq!(
                &tiles[BlockFace::South as usize].rgba8[..4],
                &[180, 30, 40, 255],
                "{}",
                record.canonical_state
            );
            assert_eq!(
                &tiles[BlockFace::Up as usize].rgba8[..4],
                &[30, 40, 180, 255]
            );
            seen += 1;
        }
        assert!(seen > 1);
    }

    #[test]
    fn partial_furnace_carried_faces_keep_the_world_fallback() {
        let root = tempfile::tempdir().unwrap();
        write_pack(
            root.path(),
            r#"{"furnace":{"textures":"side","carried_textures":{"up":"top"}}}"#,
            r#"{"texture_data":{"side":{"textures":"textures/blocks/side"},"top":{"textures":"textures/blocks/top"}}}"#,
            "[]",
        );
        for (name, color) in [("side", [61, 79, 97, 255]), ("top", [19, 31, 47, 255])] {
            image::RgbaImage::from_pixel(16, 16, image::Rgba(color))
                .save(root.path().join(format!("textures/blocks/{name}.png")))
                .unwrap();
        }
        let blocks = IconBlocks::read(root.path()).unwrap();
        for record in blocks
            .records
            .iter()
            .filter(|r| r.name.as_ref() == "minecraft:furnace")
        {
            let tiles = blocks
                .carried_cube_tiles(root.path(), BlockVisualId(record.sequential_id))
                .unwrap()
                .expect("missing carried faces fall back independently");
            for face in BlockFace::ALL {
                let expected = if face == BlockFace::Up {
                    [19, 31, 47, 255]
                } else {
                    [61, 79, 97, 255]
                };
                assert_eq!(&tiles[face as usize].rgba8[..4], &expected);
            }
        }
    }

    /// Writes the block and texture catalogs needed to load icon test fixtures.
    fn write_pack(root: &Path, blocks: &str, terrain: &str, flipbooks: &str) {
        std::fs::create_dir_all(root.join("textures/blocks")).unwrap();
        std::fs::write(root.join("blocks.json"), blocks).unwrap();
        std::fs::write(root.join("textures/terrain_texture.json"), terrain).unwrap();
        std::fs::write(root.join("textures/flipbook_textures.json"), flipbooks).unwrap();
    }

    /// A flat icon shows one frame of a flipbook strip, while other textures keep their size.
    #[test]
    fn flat_flipbook_icons_show_their_first_frame() {
        let root = tempfile::tempdir().unwrap();
        write_pack(
            root.path(),
            r#"{"sculk_vein":{"textures":"sculk_vein"}}"#,
            r#"{"texture_data":{"sculk_vein":{"textures":"textures/blocks/sculk_vein"}}}"#,
            r#"[{"flipbook_texture":"textures/blocks/sculk_vein","atlas_tile":"sculk_vein","ticks_per_frame":2}]"#,
        );
        for name in ["sculk_vein", "tall"] {
            image::RgbaImage::new(16, 64)
                .save(root.path().join(format!("textures/blocks/{name}.png")))
                .unwrap();
        }
        let blocks = IconBlocks::read(root.path()).unwrap();
        let size = |path: &str| {
            let sprite = blocks.flat_sprite(root.path(), path).unwrap().unwrap();
            [sprite.width, sprite.height]
        };
        assert_eq!(size("textures/blocks/sculk_vein"), [16, 16]);
        assert_eq!(size("textures/blocks/tall"), [16, 64]);
    }

    /// A model texture past the carrier's side is skipped rather than failing its decode.
    #[test]
    fn oversized_model_textures_are_skipped() {
        let root = tempfile::tempdir().unwrap();
        image::RgbaImage::new(512, 512)
            .save(root.path().join("head.png"))
            .unwrap();
        assert!(
            IconBlocks::model_texture(root.path(), "head")
                .unwrap()
                .is_none()
        );
    }
}
