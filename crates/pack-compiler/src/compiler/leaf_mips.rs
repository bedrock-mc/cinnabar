//! World-only native leaf mip copies. Never mutate the shared/carried layers.

use std::collections::BTreeMap;

use assets::{
    Animation, AssetError, MATERIAL_FLAG_NATIVE_LEAF_COLOUR, MAX_ANIMATION_FRAMES, MAX_ANIMATIONS,
    MAX_TEXTURE_LAYERS, MAX_TEXTURE_PAGES, MIP_COUNT, Material, NO_ANIMATION, TILE_SIZE,
    TextureArray, TextureMip, TexturePage, TextureRef, build_legacy_terrain_mip_chain,
};

pub(super) struct CompiledLeafTextures {
    pub(super) pages: Box<[TexturePage]>,
    pub(super) animations: Box<[Animation]>,
    pub(super) frames: Box<[TextureRef]>,
}

pub(super) fn install(
    materials: &mut [Material],
    pages: Box<[TexturePage]>,
    animations: Box<[Animation]>,
    frames: Box<[TextureRef]>,
) -> Result<CompiledLeafTextures, AssetError> {
    let mut builder = NativeLayers::new(&pages)?;
    let mut new_animations = animations.into_vec();
    let mut new_frames = frames.into_vec();
    let mut animation_copies = BTreeMap::new();
    for material in materials
        .iter_mut()
        .filter(|material| material.flags & MATERIAL_FLAG_NATIVE_LEAF_COLOUR != 0)
    {
        material.texture = builder.copy(material.texture)?;
        if material.animation == NO_ANIMATION {
            continue;
        }
        if let Some(&copy) = animation_copies.get(&material.animation) {
            material.animation = copy;
            continue;
        }
        let mut animation = *new_animations
            .get(material.animation as usize)
            .ok_or_else(|| invalid("world leaf animation is outside its table"))?;
        let start = animation.frame_start as usize;
        let end = start
            .checked_add(animation.frame_count as usize)
            .filter(|&end| end <= new_frames.len())
            .ok_or_else(|| invalid("world leaf animation timeline is outside its table"))?;
        if new_animations.len() >= MAX_ANIMATIONS
            || new_frames.len().saturating_add(end - start) > MAX_ANIMATION_FRAMES
        {
            return Err(invalid(
                "world leaf copies exceed the animation table bounds",
            ));
        }
        let mut timeline = Vec::with_capacity(end - start);
        for &texture in &new_frames[start..end] {
            timeline.push(builder.copy(texture)?);
        }
        animation.frame_start = new_frames.len() as u32;
        let copy = new_animations.len() as u32;
        new_frames.extend(timeline);
        new_animations.push(animation);
        animation_copies.insert(material.animation, copy);
        material.animation = copy;
    }
    let additions = builder.finish();
    Ok(CompiledLeafTextures {
        pages: append_layers(pages.into_vec(), additions)?,
        animations: new_animations.into_boxed_slice(),
        frames: new_frames.into_boxed_slice(),
    })
}

struct NativeLayers<'a> {
    pages: &'a [TexturePage],
    counts: Vec<usize>,
    copies: BTreeMap<TextureRef, TextureRef>,
    bases: BTreeMap<Box<[u8]>, TextureRef>,
    additions: Vec<(TextureRef, Box<[TextureMip]>)>,
}

impl<'a> NativeLayers<'a> {
    fn new(pages: &'a [TexturePage]) -> Result<Self, AssetError> {
        if pages.is_empty() || pages.len() > MAX_TEXTURE_PAGES {
            return Err(invalid("world leaf texture pages are outside their bound"));
        }
        for page in pages {
            if page.texture.layers as usize > MAX_TEXTURE_LAYERS
                || page.texture.mips.len() != MIP_COUNT as usize
                || page.texture.mips.iter().enumerate().any(|(level, mip)| {
                    let size = TILE_SIZE >> level;
                    mip.size != size
                        || mip.rgba8.len() != (size * size * 4 * page.texture.layers) as usize
                })
            {
                return Err(invalid("world leaf source page has noncanonical mips"));
            }
        }
        Ok(Self {
            pages,
            counts: pages
                .iter()
                .map(|page| page.texture.layers as usize)
                .collect(),
            copies: BTreeMap::new(),
            bases: BTreeMap::new(),
            additions: Vec::new(),
        })
    }

    fn copy(&mut self, source: TextureRef) -> Result<TextureRef, AssetError> {
        if let Some(&texture) = self.copies.get(&source) {
            return Ok(texture);
        }
        let page = self
            .pages
            .get(source.page() as usize)
            .filter(|page| source.layer() < page.texture.layers)
            .ok_or_else(|| invalid("world leaf texture is outside its source page"))?;
        let bytes_per_layer = (TILE_SIZE * TILE_SIZE * 4) as usize;
        let start = source.layer() as usize * bytes_per_layer;
        let base = &page.texture.mips[0].rgba8[start..start + bytes_per_layer];
        if let Some(&texture) = self.bases.get(base) {
            self.copies.insert(source, texture);
            return Ok(texture);
        }
        let page_index = self
            .counts
            .iter()
            .position(|&layers| layers < MAX_TEXTURE_LAYERS)
            .unwrap_or(self.counts.len());
        if page_index >= MAX_TEXTURE_PAGES {
            return Err(invalid(
                "world leaf copies exceed the texture page/layer budget",
            ));
        }
        if page_index == self.counts.len() {
            self.counts.push(0);
        }
        let texture = TextureRef::new(page_index as u32, self.counts[page_index] as u32)?;
        let mips = build_legacy_terrain_mip_chain(base, TILE_SIZE)?;
        self.counts[page_index] += 1;
        self.bases.insert(base.into(), texture);
        self.copies.insert(source, texture);
        self.additions.push((texture, mips));
        Ok(texture)
    }

    fn finish(self) -> Vec<(TextureRef, Box<[TextureMip]>)> {
        self.additions
    }
}

fn append_layers(
    mut pages: Vec<TexturePage>,
    additions: Vec<(TextureRef, Box<[TextureMip]>)>,
) -> Result<Box<[TexturePage]>, AssetError> {
    // Append each page once. Its existing per-mip prefixes stay byte-identical.
    for index in 0..MAX_TEXTURE_PAGES {
        let layers = additions
            .iter()
            .filter(|(texture, _)| texture.page() as usize == index)
            .collect::<Vec<_>>();
        if layers.is_empty() {
            continue;
        }
        if index == pages.len() {
            pages.push(TexturePage::new(TextureArray {
                layers: 0,
                mips: (0..MIP_COUNT)
                    .map(|level| TextureMip {
                        size: TILE_SIZE >> level,
                        rgba8: Box::default(),
                    })
                    .collect(),
            }));
        }
        let page = pages
            .get_mut(index)
            .ok_or_else(|| invalid("world leaf destination page is missing"))?;
        for (level, mip) in page.texture.mips.iter_mut().enumerate() {
            let mut bytes = std::mem::take(&mut mip.rgba8).into_vec();
            for (_, chain) in &layers {
                bytes.extend_from_slice(&chain[level].rgba8);
            }
            mip.rgba8 = bytes.into_boxed_slice();
        }
        page.texture.layers += layers.len() as u32;
    }
    Ok(pages.into_boxed_slice())
}

fn invalid(detail: &str) -> AssetError {
    AssetError::InvalidCompiledAssets {
        detail: detail.into(),
    }
}

#[cfg(test)]
#[path = "leaf_mips_tests.rs"]
mod tests;
