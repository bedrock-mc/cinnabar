use std::collections::{BTreeMap, BTreeSet, HashMap};

use serde::Serialize;
use sha2::{Digest, Sha256};

use assets::{AssetError, TextureMip, TextureRef};

use crate::{
    FlipbookSource, PackSources,
    image::{build_texture_mip_chain, diagnostic_pixels, normalize_texture_tile},
};

#[cfg(test)]
const TEXTURE_PAGE_BIT: u32 = 1 << 31;
#[cfg(test)]
const TEXTURE_LAYER_MASK: u32 = 0x7ff;
const MAX_LAYERS_PER_PAGE: u32 = 2_048;
const MAX_TEXTURE_PAGES: u32 = 2;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct AnimationLimits {
    pub max_layers_per_page: u32,
    pub max_pages: u32,
}

impl AnimationLimits {
    fn validate(self) -> Result<Self, AssetError> {
        if self.max_layers_per_page == 0
            || self.max_layers_per_page > MAX_LAYERS_PER_PAGE
            || self.max_pages == 0
            || self.max_pages > MAX_TEXTURE_PAGES
        {
            return Err(AssetError::InvalidAnimationLimits {
                max_layers_per_page: self.max_layers_per_page,
                max_pages: self.max_pages,
            });
        }
        Ok(self)
    }

    fn capacity(self) -> usize {
        self.max_layers_per_page as usize * self.max_pages as usize
    }
}

fn texture_ref_from_linear_index(
    index: usize,
    limits: AnimationLimits,
) -> Result<TextureRef, AssetError> {
    let limits = limits.validate()?;
    if index >= limits.capacity() {
        return Err(AssetError::TooManyAnimationTexturePages {
            required_layers: index.saturating_add(1),
            max_layers_per_page: limits.max_layers_per_page,
            max_pages: limits.max_pages,
        });
    }
    let page = index / limits.max_layers_per_page as usize;
    let layer = index % limits.max_layers_per_page as usize;
    TextureRef::new(page as u32, layer as u32)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DecodedImage {
    pub source_path: Box<str>,
    pub width: u32,
    pub height: u32,
    pub rgba8: Box<[u8]>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AnimationLayer {
    pub mips: Box<[TextureMip]>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AnimationDescriptor {
    pub source_path: Box<str>,
    pub atlas_tile: Box<str>,
    pub ticks_per_frame: u32,
    pub atlas_index: u32,
    pub atlas_tile_variant: u32,
    pub replicate: u32,
    pub blend_frames: bool,
    pub frame_start: u32,
    pub frame_count: u32,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct AnimationInventory {
    pub catalog_static_sources: u32,
    pub static_sources: u32,
    pub missing_static_sources: u32,
    pub non_tile_static_sources: u32,
    pub reachable_animations: u32,
    pub animation_sources: u32,
    pub physical_animation_frames: u32,
    pub timeline_frames: u32,
    pub unique_static_layers: u32,
    pub unique_animation_layers: u32,
    pub diagnostic_layers: u32,
    pub deduplicated_layers: u32,
    pub pages: u32,
    pub page_layers: Box<[u32]>,
    pub max_layers_per_page: u32,
    pub max_pages: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AnimationPlan {
    pub animations: Box<[AnimationDescriptor]>,
    pub frames: Box<[TextureRef]>,
    pub layers: Box<[AnimationLayer]>,
    pub static_refs: BTreeMap<Box<str>, TextureRef>,
    pub strip_first_refs: BTreeMap<Box<str>, TextureRef>,
    pub inventory: AnimationInventory,
    limits: AnimationLimits,
}

impl AnimationPlan {
    #[cfg(test)]
    pub(crate) fn timeline(&self, animation: &AnimationDescriptor) -> Option<&[TextureRef]> {
        let start = animation.frame_start as usize;
        let end = start.checked_add(animation.frame_count as usize)?;
        self.frames.get(start..end)
    }

    #[cfg(test)]
    pub(crate) fn layer(&self, texture: TextureRef) -> Option<&AnimationLayer> {
        if texture.raw() & !(TEXTURE_PAGE_BIT | TEXTURE_LAYER_MASK) != 0 {
            return None;
        }
        let index = texture.page() as usize * self.limits.max_layers_per_page as usize
            + texture.layer() as usize;
        self.layers.get(index)
    }
}

struct LayerDeduper {
    limits: AnimationLimits,
    layers: Vec<AnimationLayer>,
    candidates: HashMap<[u8; 32], Vec<usize>>,
}

impl LayerDeduper {
    fn new(limits: AnimationLimits) -> Result<Self, AssetError> {
        Ok(Self {
            limits: limits.validate()?,
            layers: Vec::new(),
            candidates: HashMap::new(),
        })
    }

    fn add(&mut self, mips: Box<[TextureMip]>) -> Result<TextureRef, AssetError> {
        let digest = mip_digest(&mips);
        if let Some(index) = self.candidates.get(&digest).and_then(|candidates| {
            candidates
                .iter()
                .copied()
                .find(|&index| self.layers[index].mips == mips)
        }) {
            return texture_ref_from_linear_index(index, self.limits);
        }
        let index = self.layers.len();
        let texture = texture_ref_from_linear_index(index, self.limits)?;
        self.layers.push(AnimationLayer { mips });
        self.candidates.entry(digest).or_default().push(index);
        Ok(texture)
    }
}

fn mip_digest(mips: &[TextureMip]) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update((mips.len() as u64).to_le_bytes());
    for mip in mips {
        hash.update(mip.size.to_le_bytes());
        hash.update((mip.rgba8.len() as u64).to_le_bytes());
        hash.update(&mip.rgba8);
    }
    hash.finalize().into()
}

pub(crate) fn compile_animation_plan(
    pack: &PackSources,
    decoded_images: &[DecodedImage],
    limits: AnimationLimits,
) -> Result<AnimationPlan, AssetError> {
    compile_animation_plan_selected(pack, decoded_images, limits, None)
}

pub(crate) fn compile_animation_plan_selected(
    pack: &PackSources,
    decoded_images: &[DecodedImage],
    limits: AnimationLimits,
    selected_atlas_tiles: Option<&BTreeSet<Box<str>>>,
) -> Result<AnimationPlan, AssetError> {
    let limits = limits.validate()?;
    let mut decoded = BTreeMap::<Box<str>, &DecodedImage>::new();
    for image in decoded_images {
        validate_decoded_image(image)?;
        if decoded.insert(image.source_path.clone(), image).is_some() {
            return Err(AssetError::DuplicateDecodedTexture {
                source_path: image.source_path.clone(),
            });
        }
    }
    let selected_flipbooks = pack
        .flipbooks
        .iter()
        .filter(|flipbook| {
            selected_atlas_tiles.is_none_or(|selected| selected.contains(&flipbook.atlas_tile))
        })
        .collect::<Vec<_>>();
    let animation_sources = selected_flipbooks
        .iter()
        .map(|flipbook| flipbook.texture_path.clone())
        .collect::<BTreeSet<_>>();
    let catalog_static_sources = pack
        .terrain
        .source_paths()
        .map(Box::<str>::from)
        .filter(|source_path| !animation_sources.contains(source_path))
        .collect::<BTreeSet<_>>();
    for source_path in &animation_sources {
        if !decoded.contains_key(source_path) {
            return Err(AssetError::MissingAnimationTexture {
                source_path: source_path.clone(),
            });
        }
    }

    let mut layers = LayerDeduper::new(limits)?;
    let diagnostic = build_texture_mip_chain(diagnostic_pixels())?;
    let diagnostic_ref = layers.add(diagnostic)?;
    debug_assert_eq!((diagnostic_ref.page(), diagnostic_ref.layer()), (0, 0));

    let mut static_refs = BTreeSet::new();
    let mut static_refs_by_source = BTreeMap::new();
    let mut static_sources = 0_usize;
    let mut non_tile_static_sources = 0_usize;
    for source_path in &catalog_static_sources {
        let Some(image) = decoded.get(source_path) else {
            continue;
        };
        if image.width != image.height
            || image.width < assets::TILE_SIZE
            || !image.width.is_power_of_two()
        {
            non_tile_static_sources += 1;
            continue;
        }
        static_sources += 1;
        let base = normalize_texture_tile(image.rgba8.clone(), image.width, source_path)?;
        let texture = layers.add(build_texture_mip_chain(base)?)?;
        static_refs.insert(texture);
        static_refs_by_source.insert(source_path.clone(), texture);
    }

    let mut physical_by_source = BTreeMap::<Box<str>, Box<[TextureRef]>>::new();
    let mut physical_animation_frames = 0_u32;
    let mut animation_refs = BTreeSet::new();
    for source_path in &animation_sources {
        let image = decoded
            .get(source_path)
            .expect("animation source presence checked above");
        let frames = slice_frames(image)?;
        physical_animation_frames = physical_animation_frames
            .checked_add(
                u32::try_from(frames.len()).map_err(|_| AssetError::BlobSizeOverflow {
                    section: "physical animation frame count",
                })?,
            )
            .ok_or(AssetError::BlobSizeOverflow {
                section: "physical animation frame count",
            })?;
        let mut refs = Vec::with_capacity(frames.len());
        for frame in frames {
            let texture = layers.add(build_texture_mip_chain(frame)?)?;
            animation_refs.insert(texture);
            refs.push(texture);
        }
        physical_by_source.insert(source_path.clone(), refs.into_boxed_slice());
    }

    let mut animations = Vec::with_capacity(selected_flipbooks.len());
    let mut timeline_frames = Vec::new();
    for (animation_index, flipbook) in selected_flipbooks.iter().enumerate() {
        let physical = physical_by_source
            .get(&flipbook.texture_path)
            .expect("animation source was compiled");
        let frame_start =
            u32::try_from(timeline_frames.len()).map_err(|_| AssetError::BlobSizeOverflow {
                section: "animation timeline frame offset",
            })?;
        if flipbook.frames.is_empty() {
            timeline_frames.extend_from_slice(physical);
        } else {
            for &frame in &flipbook.frames {
                let Some(&texture) = physical.get(frame as usize) else {
                    return Err(AssetError::FlipbookFrameOutOfRange {
                        animation: animation_index,
                        frame,
                        physical_frames: physical.len() as u32,
                    });
                };
                timeline_frames.push(texture);
            }
        }
        let frame_count =
            u32::try_from(timeline_frames.len() - frame_start as usize).map_err(|_| {
                AssetError::BlobSizeOverflow {
                    section: "animation timeline frame count",
                }
            })?;
        animations.push(descriptor(flipbook, frame_start, frame_count));
    }

    let deduplicated_layers = layers.layers.len();
    let page_count = deduplicated_layers.div_ceil(limits.max_layers_per_page as usize);
    let page_layers = (0..page_count)
        .map(|page| {
            let consumed = page * limits.max_layers_per_page as usize;
            u32::try_from((deduplicated_layers - consumed).min(limits.max_layers_per_page as usize))
                .expect("page layer limit fits u32")
        })
        .collect::<Vec<_>>();
    let missing_static_sources = catalog_static_sources
        .len()
        .checked_sub(static_sources + non_tile_static_sources)
        .ok_or(AssetError::BlobSizeOverflow {
            section: "missing static source count",
        })?;
    let inventory = AnimationInventory {
        catalog_static_sources: u32::try_from(catalog_static_sources.len()).map_err(|_| {
            AssetError::BlobSizeOverflow {
                section: "catalog static source count",
            }
        })?,
        static_sources: u32::try_from(static_sources).map_err(|_| {
            AssetError::BlobSizeOverflow {
                section: "static animation inventory source count",
            }
        })?,
        missing_static_sources: u32::try_from(missing_static_sources).map_err(|_| {
            AssetError::BlobSizeOverflow {
                section: "missing static source count",
            }
        })?,
        non_tile_static_sources: u32::try_from(non_tile_static_sources).map_err(|_| {
            AssetError::BlobSizeOverflow {
                section: "non-tile static source count",
            }
        })?,
        reachable_animations: u32::try_from(selected_flipbooks.len()).map_err(|_| {
            AssetError::BlobSizeOverflow {
                section: "reachable animation count",
            }
        })?,
        animation_sources: u32::try_from(animation_sources.len()).map_err(|_| {
            AssetError::BlobSizeOverflow {
                section: "animation source count",
            }
        })?,
        physical_animation_frames,
        timeline_frames: u32::try_from(timeline_frames.len()).map_err(|_| {
            AssetError::BlobSizeOverflow {
                section: "animation timeline frame count",
            }
        })?,
        unique_static_layers: u32::try_from(static_refs.len()).expect("bounded texture refs"),
        unique_animation_layers: u32::try_from(animation_refs.len()).expect("bounded texture refs"),
        diagnostic_layers: 1,
        deduplicated_layers: u32::try_from(deduplicated_layers).expect("bounded texture refs"),
        pages: u32::try_from(page_count).expect("bounded texture pages"),
        page_layers: page_layers.into_boxed_slice(),
        max_layers_per_page: limits.max_layers_per_page,
        max_pages: limits.max_pages,
    };
    let strip_first_refs = physical_by_source
        .iter()
        .filter_map(|(path, frames)| frames.first().copied().map(|frame| (path.clone(), frame)))
        .collect();

    Ok(AnimationPlan {
        animations: animations.into_boxed_slice(),
        frames: timeline_frames.into_boxed_slice(),
        layers: layers.layers.into_boxed_slice(),
        static_refs: static_refs_by_source,
        strip_first_refs,
        inventory,
        limits,
    })
}

fn descriptor(source: &FlipbookSource, frame_start: u32, frame_count: u32) -> AnimationDescriptor {
    AnimationDescriptor {
        source_path: source.texture_path.clone(),
        atlas_tile: source.atlas_tile.clone(),
        ticks_per_frame: source.ticks_per_frame,
        atlas_index: source.atlas_index,
        atlas_tile_variant: source.atlas_tile_variant,
        replicate: source.replicate,
        blend_frames: source.blend_frames,
        frame_start,
        frame_count,
    }
}

fn validate_decoded_image(image: &DecodedImage) -> Result<(), AssetError> {
    let expected = image
        .width
        .checked_mul(image.height)
        .and_then(|pixels| pixels.checked_mul(4))
        .and_then(|bytes| usize::try_from(bytes).ok())
        .ok_or(AssetError::BlobSizeOverflow {
            section: "decoded animation texture",
        })?;
    if image.rgba8.len() != expected {
        return Err(AssetError::AnimationTextureByteLength {
            source_path: image.source_path.clone(),
            actual: image.rgba8.len(),
            expected,
        });
    }
    Ok(())
}

fn slice_frames(image: &DecodedImage) -> Result<Vec<Box<[u8]>>, AssetError> {
    let frame_size = image.width.min(image.height);
    let long_side = image.width.max(image.height);
    if frame_size == 0 || !long_side.is_multiple_of(frame_size) {
        return Err(AssetError::AnimationTextureDimensions {
            source_path: image.source_path.clone(),
            width: image.width,
            height: image.height,
            detail: "strip must contain square frames in one horizontal or vertical row".into(),
        });
    }
    let frame_count = long_side / frame_size;
    if frame_count as usize > crate::MAX_FLIPBOOK_FRAMES {
        return Err(AssetError::AnimationTextureDimensions {
            source_path: image.source_path.clone(),
            width: image.width,
            height: image.height,
            detail: format!(
                "strip has {frame_count} frames, exceeding {}",
                crate::MAX_FLIPBOOK_FRAMES
            )
            .into(),
        });
    }
    let horizontal = image.width > image.height;
    let frame_bytes = (frame_size * frame_size * 4) as usize;
    let mut frames = Vec::with_capacity(frame_count as usize);
    for frame in 0..frame_count {
        let mut pixels = Vec::with_capacity(frame_bytes);
        for row in 0..frame_size {
            let x = if horizontal { frame * frame_size } else { 0 };
            let y = if horizontal {
                row
            } else {
                frame * frame_size + row
            };
            let start = ((y * image.width + x) * 4) as usize;
            let end = start + (frame_size * 4) as usize;
            pixels.extend_from_slice(&image.rgba8[start..end]);
        }
        frames.push(normalize_texture_tile(
            pixels.into_boxed_slice(),
            frame_size,
            &image.source_path,
        )?);
    }
    Ok(frames)
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path};

    use tempfile::TempDir;

    use super::{
        AnimationLimits, DecodedImage, compile_animation_plan, texture_ref_from_linear_index,
    };
    use crate::read_pack;
    use assets::TextureRef;
    use assets::{AssetError, MIP_COUNT, TILE_SIZE};

    fn write(path: impl AsRef<Path>, contents: impl AsRef<[u8]>) {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create fixture parent");
        }
        fs::write(path, contents).expect("write fixture");
    }

    fn synthetic_pack(terrain: &str, flipbooks: &str) -> (TempDir, crate::PackSources) {
        let directory = tempfile::tempdir().expect("create pack fixture");
        write(directory.path().join("blocks.json"), "{}");
        write(
            directory.path().join("textures/terrain_texture.json"),
            format!(r#"{{"texture_data":{{{terrain}}}}}"#),
        );
        write(
            directory.path().join("textures/flipbook_textures.json"),
            flipbooks,
        );
        let pack = read_pack(directory.path()).expect("read synthetic pack");
        (directory, pack)
    }

    fn solid_frame(colour: [u8; 4]) -> Vec<u8> {
        colour
            .into_iter()
            .cycle()
            .take((TILE_SIZE * TILE_SIZE * 4) as usize)
            .collect()
    }

    fn strip(path: &str, horizontal: bool, colours: &[[u8; 4]]) -> DecodedImage {
        let (width, height) = if horizontal {
            (TILE_SIZE * colours.len() as u32, TILE_SIZE)
        } else {
            (TILE_SIZE, TILE_SIZE * colours.len() as u32)
        };
        let mut rgba8 = vec![0; (width * height * 4) as usize];
        for (frame, colour) in colours.iter().copied().enumerate() {
            for y in 0..TILE_SIZE {
                for x in 0..TILE_SIZE {
                    let (target_x, target_y) = if horizontal {
                        (frame as u32 * TILE_SIZE + x, y)
                    } else {
                        (x, frame as u32 * TILE_SIZE + y)
                    };
                    let offset = ((target_y * width + target_x) * 4) as usize;
                    rgba8[offset..offset + 4].copy_from_slice(&colour);
                }
            }
        }
        DecodedImage {
            source_path: path.into(),
            width,
            height,
            rgba8: rgba8.into_boxed_slice(),
        }
    }

    fn static_image(path: &str, colour: [u8; 4]) -> DecodedImage {
        DecodedImage {
            source_path: path.into(),
            width: TILE_SIZE,
            height: TILE_SIZE,
            rgba8: solid_frame(colour).into_boxed_slice(),
        }
    }

    fn limits(layers: u32, pages: u32) -> AnimationLimits {
        AnimationLimits {
            max_layers_per_page: layers,
            max_pages: pages,
        }
    }

    fn first_pixel(plan: &super::AnimationPlan, texture: TextureRef) -> [u8; 4] {
        let layer = plan.layer(texture).expect("referenced layer");
        layer.mips[0].rgba8[..4].try_into().expect("RGBA pixel")
    }

    #[test]
    fn flipbook_vertical_horizontal_order_replication_variants_and_blending() {
        let (_directory, pack) = synthetic_pack(
            r#"
                "vertical":{"textures":"textures/blocks/vertical"},
                "horizontal":{"textures":"textures/blocks/horizontal"}
            "#,
            r#"[
                {
                    "flipbook_texture":"textures/blocks/vertical",
                    "atlas_tile":"vertical",
                    "ticks_per_frame":3,
                    "frames":[1,0,1],
                    "atlas_index":2,
                    "atlas_tile_variant":3,
                    "replicate":2,
                    "blend_frames":true
                },
                {
                    "flipbook_texture":"textures/blocks/horizontal",
                    "atlas_tile":"horizontal"
                }
            ]"#,
        );
        let images = [
            strip(
                "textures/blocks/vertical",
                false,
                &[[200, 10, 20, 255], [20, 200, 30, 255]],
            ),
            strip(
                "textures/blocks/horizontal",
                true,
                &[[30, 40, 200, 255], [220, 210, 20, 255]],
            ),
        ];

        let plan = compile_animation_plan(&pack, &images, limits(16, 2)).expect("compile plan");

        assert_eq!(plan.animations.len(), 2);
        let vertical = &plan.animations[0];
        assert_eq!(vertical.ticks_per_frame, 3);
        assert_eq!(vertical.atlas_index, 2);
        assert_eq!(vertical.atlas_tile_variant, 3);
        assert_eq!(
            vertical.replicate, 2,
            "replication remains spatial metadata"
        );
        assert!(vertical.blend_frames);
        let timeline = plan.timeline(vertical).expect("vertical timeline");
        assert_eq!(
            timeline.len(),
            3,
            "replication must not duplicate timeline frames"
        );
        assert_eq!(first_pixel(&plan, timeline[0]), [20, 200, 30, 255]);
        assert_eq!(first_pixel(&plan, timeline[1]), [200, 10, 20, 255]);
        assert_eq!(timeline[0], timeline[2]);

        let horizontal = &plan.animations[1];
        let timeline = plan.timeline(horizontal).expect("horizontal timeline");
        assert_eq!(timeline.len(), 2);
        assert_eq!(first_pixel(&plan, timeline[0]), [30, 40, 200, 255]);
        assert_eq!(first_pixel(&plan, timeline[1]), [220, 210, 20, 255]);
    }

    #[test]
    fn flipbook_mips_are_generated_per_frame_with_alpha_coverage() {
        let (_directory, pack) = synthetic_pack(
            r#""cutout":{"textures":"textures/blocks/cutout"}"#,
            r#"[{"flipbook_texture":"textures/blocks/cutout","atlas_tile":"cutout"}]"#,
        );
        let mut pixels = vec![[0, 0, 0, 0]; (TILE_SIZE * TILE_SIZE) as usize];
        for (index, pixel) in pixels.iter_mut().enumerate().step_by(2) {
            *pixel = [255, 255, 255, 255];
            assert_eq!(index % 2, 0);
        }
        let image = DecodedImage {
            source_path: "textures/blocks/cutout".into(),
            width: TILE_SIZE,
            height: TILE_SIZE,
            rgba8: pixels
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        };

        let plan = compile_animation_plan(&pack, &[image], limits(16, 2)).expect("compile plan");
        let texture = plan.frames[0];
        let layer = plan.layer(texture).expect("animation layer");
        assert_eq!(layer.mips.len(), MIP_COUNT as usize);
        assert_eq!(
            layer.mips.iter().map(|mip| mip.size).collect::<Vec<_>>(),
            [16, 8, 4, 2, 1]
        );
        assert!(
            layer.mips.last().expect("1x1 mip").rgba8[3] >= 128,
            "coverage correction must not erase a half-covered frame"
        );
    }

    #[test]
    fn flipbook_deduplicates_byte_identical_complete_mip_chains() {
        let (_directory, pack) = synthetic_pack(
            r#""same":{"textures":"textures/blocks/same"}"#,
            r#"[{"flipbook_texture":"textures/blocks/same","atlas_tile":"same"}]"#,
        );
        let image = strip(
            "textures/blocks/same",
            false,
            &[[80, 90, 100, 255], [80, 90, 100, 255]],
        );

        let plan = compile_animation_plan(&pack, &[image], limits(16, 2)).expect("compile plan");

        assert_eq!(plan.frames.len(), 2);
        assert_eq!(plan.frames[0], plan.frames[1]);
        assert_eq!(plan.inventory.physical_animation_frames, 2);
        assert_eq!(plan.inventory.unique_animation_layers, 1);
        assert_eq!(
            plan.inventory.deduplicated_layers, 2,
            "diagnostic + one frame"
        );
    }

    #[test]
    fn flipbook_rejects_physical_frame_index_out_of_range() {
        let (_directory, pack) = synthetic_pack(
            r#""short":{"textures":"textures/blocks/short"}"#,
            r#"[{
                "flipbook_texture":"textures/blocks/short",
                "atlas_tile":"short",
                "frames":[0,2]
            }]"#,
        );
        let image = strip(
            "textures/blocks/short",
            false,
            &[[1, 2, 3, 255], [4, 5, 6, 255]],
        );

        let error = compile_animation_plan(&pack, &[image], limits(16, 2))
            .expect_err("frame 2 is outside a two-frame strip");
        assert!(matches!(
            error,
            AssetError::FlipbookFrameOutOfRange {
                animation: 0,
                frame: 2,
                physical_frames: 2
            }
        ));
    }

    #[test]
    fn flipbook_inventory_counts_static_reachable_physical_and_deduplicated_layers() {
        let (_directory, pack) = synthetic_pack(
            r#"
                "still":{"textures":"textures/blocks/still"},
                "animated":{"textures":"textures/blocks/animated"}
            "#,
            r#"[
                {"flipbook_texture":"textures/blocks/animated","atlas_tile":"animated"},
                {
                    "flipbook_texture":"textures/blocks/animated",
                    "atlas_tile":"animated",
                    "atlas_index":1,
                    "frames":[1]
                }
            ]"#,
        );
        let images = [
            static_image("textures/blocks/still", [3, 4, 5, 255]),
            strip(
                "textures/blocks/animated",
                false,
                &[[10, 20, 30, 255], [40, 50, 60, 255]],
            ),
        ];

        let plan = compile_animation_plan(&pack, &images, limits(3, 2)).expect("compile plan");

        assert_eq!(plan.inventory.static_sources, 1);
        assert_eq!(plan.inventory.reachable_animations, 2);
        assert_eq!(plan.inventory.animation_sources, 1);
        assert_eq!(plan.inventory.physical_animation_frames, 2);
        assert_eq!(plan.inventory.timeline_frames, 3);
        assert_eq!(plan.inventory.unique_static_layers, 1);
        assert_eq!(plan.inventory.unique_animation_layers, 2);
        assert_eq!(plan.inventory.deduplicated_layers, 4);
        assert_eq!(plan.inventory.pages, 2);
        assert_eq!(plan.inventory.page_layers.as_ref(), [3, 1]);
    }

    #[test]
    fn texture_page_ref_encodes_page_and_layer_and_rolls_at_2048() {
        let limits = limits(2_048, 2);
        let last_first_page = texture_ref_from_linear_index(2_047, limits).expect("page zero");
        let first_second_page = texture_ref_from_linear_index(2_048, limits).expect("page one");

        assert_eq!(
            (last_first_page.page(), last_first_page.layer()),
            (0, 2_047)
        );
        assert_eq!(last_first_page.raw(), 2_047);
        assert_eq!(
            (first_second_page.page(), first_second_page.layer()),
            (1, 0)
        );
        assert_eq!(first_second_page.raw(), 1_u32 << 31);
        assert!(texture_ref_from_linear_index(4_096, limits).is_err());
    }

    #[test]
    fn texture_page_sequences_can_remain_on_or_cross_pages() {
        let (_directory, pack) = synthetic_pack(
            r#""animated":{"textures":"textures/blocks/animated"}"#,
            r#"[{"flipbook_texture":"textures/blocks/animated","atlas_tile":"animated"}]"#,
        );
        let image = strip(
            "textures/blocks/animated",
            false,
            &[[1, 10, 20, 255], [2, 20, 30, 255], [3, 30, 40, 255]],
        );

        let same_page = compile_animation_plan(&pack, std::slice::from_ref(&image), limits(4, 2))
            .expect("same-page plan");
        assert!(same_page.frames.iter().all(|texture| texture.page() == 0));

        let cross_page =
            compile_animation_plan(&pack, &[image], limits(2, 2)).expect("cross-page plan");
        assert_eq!(
            cross_page
                .frames
                .iter()
                .map(|texture| (texture.page(), texture.layer()))
                .collect::<Vec<_>>(),
            [(0, 1), (1, 0), (1, 1)]
        );
    }

    #[test]
    fn texture_page_rejects_a_required_third_page() {
        let (_directory, pack) = synthetic_pack(
            r#""animated":{"textures":"textures/blocks/animated"}"#,
            r#"[{"flipbook_texture":"textures/blocks/animated","atlas_tile":"animated"}]"#,
        );
        let image = strip(
            "textures/blocks/animated",
            false,
            &[
                [1, 1, 1, 255],
                [2, 2, 2, 255],
                [3, 3, 3, 255],
                [4, 4, 4, 255],
            ],
        );

        let error = compile_animation_plan(&pack, &[image], limits(2, 2))
            .expect_err("diagnostic plus four frames require a third page");
        assert!(matches!(
            error,
            AssetError::TooManyAnimationTexturePages {
                required_layers: 5,
                max_layers_per_page: 2,
                max_pages: 2
            }
        ));
    }
}
