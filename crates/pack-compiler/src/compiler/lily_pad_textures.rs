//! Fixed lily-pad atlas tint, isolated
//! from any untinted material which happens to share the same source image.

use assets::{MAX_TEXTURE_PAGES, MIP_COUNT, MODEL_TEMPLATE_FLAG_LILY_PAD, TILE_SIZE, TextureMip};
use {super::*, assets::BlockFace};

pub(super) struct Installed {
    pub pages: Box<[TexturePage]>,
    pub material_keys: Vec<(u32, Box<str>)>,
    pub fixed_tints: Vec<(Box<str>, [u8; 3])>,
}

pub(super) struct Inputs<'a> {
    pub pack: &'a PackSources,
    pub records: &'a [RegistryRecord],
    pub fallback: &'a visuals::fallback::FallbackInventory<'a>,
    pub descriptors: &'a BTreeMap<Descriptor, u32>,
    pub templates: &'a [ModelTemplate],
}

pub(super) fn install(
    inputs: Inputs<'_>,
    materials: &mut Vec<Material>,
    visuals: &mut [BlockVisual],
    quads: &mut [ModelQuad],
    mut pages: Vec<TexturePage>,
) -> Result<Installed, AssetError> {
    let Inputs {
        pack,
        records,
        fallback,
        descriptors,
        templates,
    } = inputs;
    let mut material_keys = Vec::new();
    let mut fixed_tints = Vec::new();
    for (template_id, template) in templates
        .iter()
        .enumerate()
        .filter(|(_, template)| template.flags == MODEL_TEMPLATE_FLAG_LILY_PAD)
    {
        let start = template.quad_start as usize;
        let original_id = quads
            .get(start)
            .ok_or_else(|| invalid("missing lily-pad top"))?
            .material;
        let record = records
            .iter()
            .find(|record| {
                visuals::lily_pad::is_record(record)
                    && visuals
                        .get(record.sequential_id as usize)
                        .is_some_and(|visual| visual.model_template == template_id as u32)
            })
            .ok_or_else(|| invalid("missing lily-pad template record"))?;
        let (descriptor, _) = descriptor_for(fallback, pack, record, BlockFace::Up)
            .filter(|(descriptor, _)| descriptors.get(descriptor) == Some(&original_id))
            .ok_or_else(|| invalid("missing lily-pad texture descriptor"))?;
        let (_, tint) = pack
            .terrain
            .fixed_tint_source(&descriptor.texture_key, descriptor.state_variant)
            .ok_or_else(|| invalid("unresolved lily-pad fixed tint"))?;
        let original = *materials
            .get(original_id as usize)
            .ok_or_else(|| invalid("missing lily-pad material"))?;
        if original.animation != NO_ANIMATION || materials.len() >= MAX_MATERIALS {
            return Err(invalid(
                "lily-pad copies exceed the static material contract",
            ));
        }
        let top_chain = copy_chain(&pages, original.texture, tint.unwrap_or([255; 3]))?;
        let top = materials.len() as u32;
        materials.push(Material {
            texture: append_layer(&mut pages, top_chain)?,
            ..original
        });
        let planes = quads
            .get_mut(start..start + 2)
            .ok_or_else(|| invalid("missing lily-pad reverse plane"))?;
        planes[0].material = top;
        planes[1].material = top;
        for visual in visuals.iter_mut().filter(|visual| {
            visual.kind == VisualKind::Model && visual.model_template == template_id as u32
        }) {
            visual.faces = [top; 6];
        }
        if let Some(tint) = tint {
            fixed_tints.push((descriptor.texture_key.clone(), tint));
        }
        material_keys.push((top, descriptor.texture_key));
    }
    Ok(Installed {
        pages: pages.into_boxed_slice(),
        material_keys,
        fixed_tints,
    })
}

fn copy_chain(
    pages: &[TexturePage],
    source: TextureRef,
    tint: [u8; 3],
) -> Result<Box<[TextureMip]>, AssetError> {
    let page = pages
        .get(source.page() as usize)
        .filter(|page| {
            source.layer() < page.texture.layers && page.texture.mips.len() == MIP_COUNT as usize
        })
        .ok_or_else(|| invalid("lily-pad source texture is outside its page"))?;
    page.texture
        .mips
        .iter()
        .map(|mip| {
            let bytes = (mip.size * mip.size * 4) as usize;
            let start = source.layer() as usize * bytes;
            let mut rgba8 = mip
                .rgba8
                .get(start..start + bytes)
                .ok_or_else(|| invalid("lily-pad source mip is truncated"))?
                .to_vec();
            // Vanilla atlas tinting multiplies RGB only.
            // UNORM bytes are truncated after tinting; holes keep their alpha.
            crate::apply_atlas_tint(&mut rgba8, tint);
            Ok(TextureMip {
                size: mip.size,
                rgba8: rgba8.into_boxed_slice(),
            })
        })
        .collect()
}

fn append_layer(
    pages: &mut Vec<TexturePage>,
    chain: Box<[TextureMip]>,
) -> Result<TextureRef, AssetError> {
    let index = pages
        .iter()
        .position(|page| page.texture.layers < MAX_TEXTURE_LAYERS as u32)
        .unwrap_or(pages.len());
    if index >= MAX_TEXTURE_PAGES {
        return Err(invalid("lily-pad textures exceed the page/layer budget"));
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
    let page = &mut pages[index];
    let texture = TextureRef::new(index as u32, page.texture.layers)?;
    for (destination, mip) in page.texture.mips.iter_mut().zip(&chain) {
        if destination.size != mip.size {
            return Err(invalid("lily-pad destination mip dimensions differ"));
        }
        let mut bytes = std::mem::take(&mut destination.rgba8).into_vec();
        bytes.extend_from_slice(&mip.rgba8);
        destination.rgba8 = bytes.into_boxed_slice();
    }
    page.texture.layers += 1;
    Ok(texture)
}

fn invalid(detail: &str) -> AssetError {
    AssetError::InvalidCompiledAssets {
        detail: detail.into(),
    }
}
