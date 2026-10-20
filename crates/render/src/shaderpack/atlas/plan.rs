//! Chooses the atlas sprites, their stored resolution and every remap slot's rectangle.

use super::pack::{PackedAtlas, SpriteRect, pack_sprites};
use super::{
    ATLAS_HEADER_SLOTS, JAVA_MIPMAP_LEVELS, STATIC_SPRITE_SLOTS, animation_sprite_slot,
    opaque_sprite_slot, static_sprite_slot,
};
use assets::{Animation, Material, TextureArray, TextureRef};
use std::collections::{BTreeMap, BTreeSet};

/// Side of the generated sprite that every absent texture's slots name.
pub(crate) const MISSING_SPRITE_SIDE: u32 = assets::TILE_SIZE;
/// Sprite key of the missing sprite; slot zero is the header, never a texture.
const MISSING_SPRITE_KEY: u32 = 0;
/// Remap words pack an x/y or width/height pair into one `u32`.
const REMAP_FIELD_BITS: u32 = 16;

/// UV-grid exponent and largest source dimension of one carrier reference, as
/// `RuntimeAssets::texture_source_grid` and `texture_source_size` report them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LayerLayout {
    pub grid: u8,
    pub source_side: u32,
}

/// Enhanced authored maps resolved to shared carrier sprites. Conflicting material
/// overrides stay neutral because one sprite cannot hold multiple authored surfaces.
#[cfg_attr(
    not(any(feature = "enhanced", test)),
    allow(dead_code, reason = "only Enhanced builds carry authored maps")
)]
pub(crate) struct AuthoredPbr<'a> {
    pub normal_pages: [&'a TextureArray; 2],
    pub mer_pages: [&'a TextureArray; 2],
    pub refs: Vec<u32>,
}

#[cfg(any(feature = "enhanced", test))]
impl<'a> AuthoredPbr<'a> {
    /// Resolves agreeing material/frame overrides once before assembling sprite mips.
    pub fn new(
        normal_pages: [&'a TextureArray; 2],
        mer_pages: [&'a TextureArray; 2],
        words: &[u32],
    ) -> Self {
        use render_scene::EnhancedTextureAssets;

        let (carrier, overrides) =
            words.split_at(words.len().min(EnhancedTextureAssets::REFERENCE_BASE_WORDS));
        let mut refs = carrier.to_vec();
        let mut shared = BTreeMap::new();
        for &[_, texture, authored] in overrides
            .as_chunks::<{ EnhancedTextureAssets::REFERENCE_OVERRIDE_WORDS }>()
            .0
        {
            let Ok(texture) = TextureRef::from_raw(texture) else {
                continue;
            };
            let slot =
                texture.page() as usize * assets::MAX_TEXTURE_LAYERS + texture.layer() as usize;
            shared
                .entry(slot)
                .and_modify(|selected| {
                    if *selected != authored {
                        *selected = u32::MAX;
                    }
                })
                .or_insert(authored);
        }
        for (slot, authored) in shared {
            refs[slot] = authored;
        }
        Self {
            normal_pages,
            mer_pages,
            refs,
        }
    }
}

/// Plain atlas sources, so planning and pixel assembly run without a GPU or runtime blob.
pub(crate) struct AtlasInputs<'a> {
    /// Physical carrier pages with their admitted mip chains, in page order.
    pub pages: &'a [&'a TextureArray],
    pub materials: &'a [Material],
    pub animations: &'a [Animation],
    pub animation_frames: &'a [TextureRef],
    pub layout: &'a dyn Fn(TextureRef) -> LayerLayout,
    pub authored: Option<AuthoredPbr<'a>>,
}

impl AtlasInputs<'_> {
    /// Physical page holding `reference`'s layer, or `None` for absent pages and layers.
    pub fn layer_page(&self, reference: TextureRef) -> Option<&TextureArray> {
        let page = *self.pages.get(reference.page() as usize)?;
        (reference.layer() < page.layers && !page.mips.is_empty()).then_some(page)
    }

    /// Stored side for `reference` before overflow scaling; absent layers use the missing sprite.
    fn base_side(&self, reference: TextureRef) -> u32 {
        self.layer_page(reference)
            .map_or(MISSING_SPRITE_SIDE, |page| {
                base_sprite_side(page.mips[0].size, (self.layout)(reference))
            })
    }
}

/// Pixels a sprite holds; absent references render as the missing sprite.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SpriteContent {
    Missing,
    /// The region of a physical layer the shaders sample; `opaque` forces alpha to 255.
    Layer {
        reference: TextureRef,
        opaque: bool,
    },
}

/// Display and frame-store squares of one animation whose table entry is usable.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AnimationLayout {
    pub display: SpriteRect,
    /// Frame-store square of each frame, in animation frame order.
    pub frames: Vec<(TextureRef, SpriteRect)>,
}

impl AnimationLayout {
    /// Frame-store square holding `reference`, as selected by `select_animation_frames`.
    pub fn frame(&self, reference: TextureRef) -> Option<SpriteRect> {
        self.frames
            .iter()
            .find(|(frame, _)| *frame == reference)
            .map(|&(_, rect)| rect)
    }
}

/// Complete placement of one atlas generation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AtlasPlan {
    pub atlas: PackedAtlas,
    /// Atlas sprites, parallel to `atlas.rects`.
    pub contents: Vec<SpriteContent>,
    /// Frame store for animation frames, parallel to `frame_contents`; absent without animations.
    pub frames: Option<PackedAtlas>,
    pub frame_contents: Vec<SpriteContent>,
    pub animations: Vec<Option<AnimationLayout>>,
    pub mip_levels: u32,
    /// Every sprite is stored at `1 / 2^scale` of its source resolution (at least one texel).
    pub scale: u32,
    /// Two words per slot: header `[width, height]`, then `[x | y << 16, w | h << 16]`.
    pub remap: Vec<u32>,
}

/// Side of the stored sprite: the physical region the shaders sample
/// (`page_side >> grid`), halved while it still covers the source resolution, as a power of two.
pub(crate) fn base_sprite_side(page_side: u32, layout: LayerLayout) -> u32 {
    let region = page_side
        .checked_shr(u32::from(layout.grid))
        .unwrap_or(0)
        .max(1);
    let mut side = 1 << region.ilog2();
    while side > 1 && side / 2 >= layout.source_side.max(1) {
        side /= 2;
    }
    side
}

/// Display sprite key of a usable animation and the frame-store key of each of its frames.
struct AnimationRequest {
    display: u32,
    frames: Vec<(TextureRef, u32)>,
}

/// Sprites before overflow scaling.
struct Requests {
    atlas: Vec<(u32, u32, SpriteContent)>,
    frames: Vec<(u32, u32, SpriteContent)>,
    animations: Vec<Option<AnimationRequest>>,
}

fn requests(inputs: &AtlasInputs<'_>) -> Requests {
    let mut atlas = vec![(
        MISSING_SPRITE_KEY,
        MISSING_SPRITE_SIDE,
        SpriteContent::Missing,
    )];
    let live = inputs
        .materials
        .iter()
        .map(|material| material.texture)
        .chain(inputs.animation_frames.iter().copied())
        .filter(|&reference| inputs.layer_page(reference).is_some())
        .collect::<BTreeSet<_>>();
    let overlays = inputs
        .materials
        .iter()
        .filter(|material| material.flags & assets::MATERIAL_FLAG_OVERLAY_MASK != 0)
        .map(|material| material.texture)
        .filter(|&reference| inputs.layer_page(reference).is_some())
        .collect::<BTreeSet<_>>();
    for &reference in &live {
        atlas.push((
            static_sprite_slot(reference.page(), reference.layer()),
            inputs.base_side(reference),
            SpriteContent::Layer {
                reference,
                opaque: false,
            },
        ));
    }
    for &reference in &overlays {
        atlas.push((
            opaque_sprite_slot(reference.page(), reference.layer()),
            inputs.base_side(reference),
            SpriteContent::Layer {
                reference,
                opaque: true,
            },
        ));
    }
    let mut frame_keys = BTreeMap::new();
    let mut frames = Vec::new();
    let animations = inputs
        .animations
        .iter()
        .enumerate()
        .map(|(index, animation)| {
            let references = animation_frames(animation, inputs.animation_frames)?;
            let side = references
                .iter()
                .map(|&reference| inputs.base_side(reference))
                .max()?;
            let display = animation_sprite_slot(u32::try_from(index).ok()?);
            atlas.push((
                display,
                side,
                SpriteContent::Layer {
                    reference: references[0],
                    opaque: false,
                },
            ));
            let keys = references
                .iter()
                .map(|&reference| {
                    let next = u32::try_from(frame_keys.len()).expect("frames fit u32 keys");
                    let key = *frame_keys.entry((reference, side)).or_insert_with(|| {
                        frames.push((
                            next,
                            side,
                            SpriteContent::Layer {
                                reference,
                                opaque: false,
                            },
                        ));
                        next
                    });
                    (reference, key)
                })
                .collect();
            Some(AnimationRequest {
                display,
                frames: keys,
            })
        })
        .collect();
    Requests {
        atlas,
        frames,
        animations,
    }
}

/// Frames of an animation whose clock arithmetic is defined; others show the missing sprite.
fn animation_frames<'a>(
    animation: &Animation,
    frames: &'a [TextureRef],
) -> Option<&'a [TextureRef]> {
    if animation.frame_count == 0 || animation.ticks_per_frame == 0 {
        return None;
    }
    let start = animation.frame_start as usize;
    frames.get(start..start.checked_add(animation.frame_count as usize)?)
}

/// Plans the atlas within `max_dimension`, halving every sprite until the atlas and frame
/// store both fit. Returns `None` only when one-texel sprites still do not fit.
pub(crate) fn plan_atlas(inputs: &AtlasInputs<'_>, max_dimension: u32) -> Option<AtlasPlan> {
    let requests = requests(inputs);
    let max_dimension = max_dimension.min(1 << (REMAP_FIELD_BITS - 1));
    let largest = requests
        .atlas
        .iter()
        .map(|&(_, side, _)| side)
        .max()
        .unwrap_or(1);
    (0..=largest.ilog2()).find_map(|scale| place(inputs, &requests, max_dimension, scale))
}

fn place(
    inputs: &AtlasInputs<'_>,
    requests: &Requests,
    max_dimension: u32,
    scale: u32,
) -> Option<AtlasPlan> {
    let scaled = |sprites: &[(u32, u32, SpriteContent)]| {
        sprites
            .iter()
            .map(|&(key, side, _)| (key, (side >> scale).max(1)))
            .collect::<Vec<_>>()
    };
    let atlas = pack_sprites(&scaled(&requests.atlas), max_dimension)?;
    let frames = if requests.frames.is_empty() {
        None
    } else {
        Some(pack_sprites(&scaled(&requests.frames), max_dimension)?)
    };
    let smallest = atlas.rects.iter().map(|rect| rect.side).min()?;
    let mip_levels = (JAVA_MIPMAP_LEVELS + 1).min(smallest.ilog2() + 1);
    let slots = ATLAS_HEADER_SLOTS
        + 2 * STATIC_SPRITE_SLOTS
        + u32::try_from(inputs.animations.len()).ok()?;
    let mut rect_of = vec![None; slots as usize];
    for (&(key, _, _), &rect) in requests.atlas.iter().zip(&atlas.rects) {
        rect_of[key as usize] = Some(rect);
    }
    let missing = rect_of[MISSING_SPRITE_KEY as usize].expect("the missing sprite is packed");
    let mut remap = vec![0; slots as usize * 2];
    remap[0] = atlas.width;
    remap[1] = atlas.height;
    let mut write = |slot: u32, rect: SpriteRect| {
        let at = slot as usize * 2;
        remap[at] = rect.x | (rect.y << REMAP_FIELD_BITS);
        remap[at + 1] = rect.side | (rect.side << REMAP_FIELD_BITS);
    };
    for slot in ATLAS_HEADER_SLOTS..ATLAS_HEADER_SLOTS + STATIC_SPRITE_SLOTS {
        let rect = rect_of[slot as usize].unwrap_or(missing);
        write(slot, rect);
        write(
            slot + STATIC_SPRITE_SLOTS,
            rect_of[(slot + STATIC_SPRITE_SLOTS) as usize].unwrap_or(rect),
        );
    }
    let animations = requests
        .animations
        .iter()
        .enumerate()
        .map(|(index, request)| {
            let slot = animation_sprite_slot(index as u32);
            let Some(AnimationRequest {
                display,
                frames: keys,
            }) = request
            else {
                write(slot, missing);
                return None;
            };
            let display = rect_of[*display as usize].expect("display sprites are packed");
            write(slot, display);
            let frames = frames.as_ref().expect("animations pack a frame store");
            Some(AnimationLayout {
                display,
                frames: keys
                    .iter()
                    .map(|&(reference, key)| (reference, frames.rects[key as usize]))
                    .collect(),
            })
        })
        .collect();
    Some(AtlasPlan {
        atlas,
        contents: requests
            .atlas
            .iter()
            .map(|&(_, _, content)| content)
            .collect(),
        frames,
        frame_contents: requests
            .frames
            .iter()
            .map(|&(_, _, content)| content)
            .collect(),
        animations,
        mip_levels,
        scale,
        remap,
    })
}
