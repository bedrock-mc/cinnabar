//! Block facts icon baking needs beyond the world carrier: registry families, the pack's carried
//! textures, and the canonical state an item is drawn from.
//!
//! Vanilla's item renderer draws a block item flat when it cannot tessellate
//! its shape, and then shows the carried texture, down face, at the
//! block's variant.

use std::{path::Path, sync::Arc};

use assets::registry::BlockTraits;
use assets::{
    AssetError, BlockFace, BlockFlags, BlockVisualId, IconSprite, ModelFamily, NetworkIdMode,
    RegistryRecord, RuntimeAssets, VisualKind, VisualSupport, read_registry_for_protocol,
};

use pack_compile_input::image::decode_texture;
use pack_compile_input::pack::{
    PackSources, read_pack, resolve_carried_down_key, resolve_carried_face_key, resolve_texture_key,
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
        self.tiles(root, visual, true)
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

    /// The state an item draws: a wall item shows a post with east and west arms.
    pub(super) fn icon_state(&self, visual: BlockVisualId) -> BlockVisualId {
        let Some(record) = self.records.get(visual.0 as usize) else {
            return visual;
        };
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
        self.tiles(root, visual, true)
    }

    /// Resolves explicit carried faces for shapes whose world model cannot supply an icon.
    pub(super) fn carried_tiles(
        &self,
        root: &Path,
        visual: BlockVisualId,
    ) -> Result<Option<[IconSprite; 6]>, AssetError> {
        self.tiles(root, visual, false)
    }

    fn tiles(
        &self,
        root: &Path,
        visual: BlockVisualId,
        world_fallback: bool,
    ) -> Result<Option<[IconSprite; 6]>, AssetError> {
        let (Some(pack), Some(record)) = (self.pack.as_ref(), self.records.get(visual.0 as usize))
        else {
            return Ok(None);
        };
        let mut tiles = Vec::with_capacity(BlockFace::ALL.len());
        for face in BlockFace::ALL {
            let world = resolve_texture_key(&pack.blocks, record, face).key;
            let variant = world
                .as_ref()
                .and_then(|key| pack.terrain.get_for_model_record(key, record))
                .map_or(0, |(_, variant)| variant as usize);
            let Some(key) = resolve_carried_face_key(&pack.blocks, record, face)
                .map(String::into_boxed_str)
                .or_else(|| world_fallback.then_some(world).flatten())
            else {
                return Ok(None);
            };
            let Some((path, overlay)) = pack.terrain.get_clamped_carried(&key, variant) else {
                return Ok(None);
            };
            let Some(tile) = super::carried::tile(
                root,
                path,
                overlay,
                pack_compile_input::respawn_anchor::is_record(record),
            )?
            else {
                return Ok(None);
            };
            tiles.push(tile);
        }
        Ok(tiles.try_into().ok())
    }

    /// The pack path of `visual`'s flat icon; `None` when absent, tinted, or unresolvable.
    pub(super) fn texture_path(&self, visual: BlockVisualId) -> Option<Box<str>> {
        let pack = self.pack.as_ref()?;
        let record = self.records.get(visual.0 as usize)?;
        let key = resolve_carried_down_key(&pack.blocks, record)?;
        // The world key's variant stands in for the block's variant.
        let variant = resolve_texture_key(&pack.blocks, record, BlockFace::Down)
            .key
            .and_then(|world| pack.terrain.get_for_model_record(&world, record))
            .map_or(0, |(_, variant)| variant as usize);
        pack.terrain
            .get_clamped_untinted(&key, variant)
            .map(Into::into)
    }

    /// A flat block icon; a flipbook texture shows its first frame, as the terrain atlas holds it.
    pub(super) fn flat_sprite(
        &self,
        root: &Path,
        path: &str,
    ) -> Result<Option<IconSprite>, AssetError> {
        let file = static_texture_path(root, path, path)?;
        if !file.is_file() {
            return Ok(None);
        }
        let mut decoded = decode_texture(&file, path)?;
        let stem = |texture: &str| {
            texture
                .strip_suffix(".png")
                .or_else(|| texture.strip_suffix(".tga"))
                .unwrap_or(texture)
                .to_owned()
        };
        if self.pack.as_ref().is_some_and(|pack| {
            pack.flipbooks
                .iter()
                .any(|flipbook| stem(&flipbook.texture_path) == stem(path))
        }) {
            pack_compile_input::image::retain_first_strip_frame(&mut decoded);
        }
        Ok(super::sprites::bounded_sprite(decoded).map(|(sprite, _)| sprite))
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
